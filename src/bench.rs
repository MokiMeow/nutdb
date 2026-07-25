//! Small, dependency-free end-to-end benchmarks.
//!
//! These are presentation measurements, not statistical microbenchmarks. Each
//! case exercises the real durability or TCP path and prints its conditions so
//! results are reproducible and comparisons remain honest.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::client::ClusterClient;
use crate::server::Server;
use crate::{MvccStore, Store};

const WRITE_COUNT: usize = 100;
const REPLICATED_WRITE_COUNT: usize = 30;
const READ_COUNT: usize = 100_000;
const WRITER_COUNT: usize = 100;
const RECOVERY_RECORDS: usize = 10_000;
const SAMPLES: usize = 3;

pub fn run() -> io::Result<()> {
    let dir = BenchDir::new()?;
    println!("nutdb benchmark (release build, sync_file durability)");
    println!(
        "conditions: median of {SAMPLES} samples; {WRITE_COUNT} single writes, \
         {REPLICATED_WRITE_COUNT} replicated writes, {READ_COUNT} MVCC reads, \
         {RECOVERY_RECORDS} recovery records per sample"
    );

    let mut measurements = Vec::new();
    for sample in 0..SAMPLES {
        let sample_dir = dir.0.join(format!("sample-{sample}"));
        fs::create_dir_all(&sample_dir)?;
        let (ungrouped, grouped) = write_throughput(&sample_dir)?;
        measurements.push(Measurement {
            ungrouped,
            grouped,
            mvcc_reads: mvcc_read_throughput(&sample_dir)?,
            replicated: replicated_latency(&sample_dir)?,
            recovery: recovery_time(&sample_dir)?,
        });
    }
    let ungrouped = median_duration(
        measurements
            .iter()
            .map(|measurement| measurement.ungrouped)
            .collect(),
    );
    let grouped = median_duration(
        measurements
            .iter()
            .map(|measurement| measurement.grouped)
            .collect(),
    );
    let mvcc_reads = median_f64(
        measurements
            .iter()
            .map(|measurement| measurement.mvcc_reads)
            .collect(),
    );
    let replicated = median_duration(
        measurements
            .iter()
            .map(|measurement| measurement.replicated)
            .collect(),
    );
    let recovery = median_duration(
        measurements
            .iter()
            .map(|measurement| measurement.recovery)
            .collect(),
    );

    println!("\n| benchmark | result |");
    println!("|---|---:|");
    println!("| single writes, fsync each | {:.0} ops/s |", rate(WRITE_COUNT, ungrouped));
    println!("| group commit, one fsync | {:.0} ops/s |", rate(WRITE_COUNT, grouped));
    println!("| MVCC snapshot reads with writer | {mvcc_reads:.0} ops/s |");
    println!(
        "| single write latency | {:.3} ms/op |",
        millis_per(ungrouped, WRITE_COUNT)
    );
    println!(
        "| 3-node replicated write latency | {:.3} ms/op |",
        millis_per(replicated, REPLICATED_WRITE_COUNT)
    );
    println!(
        "| reopen and replay {RECOVERY_RECORDS} records | {:.3} ms |",
        recovery.as_secs_f64() * 1_000.0
    );
    Ok(())
}

fn write_throughput(root: &std::path::Path) -> io::Result<(Duration, Duration)> {
    let mut store = Store::open(root.join("single.wal"))?;
    let started = Instant::now();
    for index in 0..WRITE_COUNT {
        store.set(format!("single-{index}"), "value")?;
    }
    let ungrouped = started.elapsed();

    let mut grouped_store = Store::open(root.join("grouped.wal"))?;
    let started = Instant::now();
    grouped_store.set_batch(
        (0..WRITE_COUNT).map(|index| (format!("grouped-{index}"), "value".to_owned())),
    )?;
    let grouped = started.elapsed();
    Ok((ungrouped, grouped))
}

fn mvcc_read_throughput(root: &std::path::Path) -> io::Result<f64> {
    let store = MvccStore::open(root.join("mvcc.wal"))?;
    let mut seed = store.begin().map_err(txn_error)?;
    seed.set("hot", "initial").map_err(txn_error)?;
    seed.commit().map_err(txn_error)?;

    let barrier = Arc::new(Barrier::new(2));
    let writer_store = store.clone();
    let writer_barrier = barrier.clone();
    let writer = thread::spawn(move || -> io::Result<()> {
        writer_barrier.wait();
        for index in 0..WRITER_COUNT {
            let mut transaction = writer_store.begin().map_err(txn_error)?;
            transaction
                .set("hot", format!("value-{index}"))
                .map_err(txn_error)?;
            transaction.commit().map_err(txn_error)?;
        }
        Ok(())
    });

    let snapshot = store.begin().map_err(txn_error)?;
    barrier.wait();
    let started = Instant::now();
    for _ in 0..READ_COUNT {
        if snapshot.get("hot").map_err(txn_error)?.as_deref() != Some("initial") {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "benchmark: snapshot changed under writer",
            ));
        }
    }
    let elapsed = started.elapsed();
    drop(snapshot);
    writer
        .join()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "benchmark writer panicked"))??;
    Ok(rate(READ_COUNT, elapsed))
}

fn replicated_latency(root: &std::path::Path) -> io::Result<Duration> {
    let addresses = [reserve_address()?, reserve_address()?, reserve_address()?];
    let all_nodes: BTreeMap<u64, SocketAddr> = addresses
        .iter()
        .enumerate()
        .map(|(index, address)| ((index + 1) as u64, *address))
        .collect();
    let running = Arc::new(AtomicBool::new(true));
    let mut handles = Vec::new();

    for (index, address) in addresses.into_iter().enumerate() {
        let id = (index + 1) as u64;
        let peers = all_nodes
            .iter()
            .filter(|(peer, _)| **peer != id)
            .map(|(peer, address)| (*peer, *address))
            .collect();
        let server = Server::bind(id, address, peers, root.join(format!("node-{id}")))?;
        let node_running = running.clone();
        handles.push(thread::spawn(move || server.run(node_running)));
    }
    wait_until_ready(&all_nodes)?;

    let client = ClusterClient::new(all_nodes.clone());
    let started = Instant::now();
    for index in 0..REPLICATED_WRITE_COUNT {
        client.put(&format!("replicated-{index}"), "value")?;
    }
    let elapsed = started.elapsed();

    running.store(false, Ordering::SeqCst);
    for address in all_nodes.values() {
        let _ = TcpStream::connect_timeout(address, Duration::from_millis(50));
    }
    for handle in handles {
        handle
            .join()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "benchmark server panicked"))??;
    }
    Ok(elapsed)
}

fn recovery_time(root: &std::path::Path) -> io::Result<Duration> {
    let path = root.join("recovery.wal");
    {
        let mut store = Store::open(&path)?;
        store.set_batch(
            (0..RECOVERY_RECORDS).map(|index| (format!("key-{index:05}"), "payload".to_owned())),
        )?;
    }
    let started = Instant::now();
    let recovered = Store::open(path)?;
    let elapsed = started.elapsed();
    if recovered.len() != RECOVERY_RECORDS {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "benchmark: recovery count mismatch",
        ));
    }
    Ok(elapsed)
}

fn reserve_address() -> io::Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.local_addr()
}

fn wait_until_ready(nodes: &BTreeMap<u64, SocketAddr>) -> io::Result<()> {
    for _ in 0..100 {
        if nodes
            .values()
            .all(|address| TcpStream::connect_timeout(address, Duration::from_millis(20)).is_ok())
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "benchmark: cluster did not start",
    ))
}

fn rate(operations: usize, elapsed: Duration) -> f64 {
    operations as f64 / elapsed.as_secs_f64()
}

fn millis_per(elapsed: Duration, operations: usize) -> f64 {
    elapsed.as_secs_f64() * 1_000.0 / operations as f64
}

fn median_duration(mut values: Vec<Duration>) -> Duration {
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_f64(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn txn_error(error: crate::TxnError) -> io::Error {
    io::Error::new(io::ErrorKind::Other, error)
}

struct Measurement {
    ungrouped: Duration,
    grouped: Duration,
    mvcc_reads: f64,
    replicated: Duration,
    recovery: Duration,
}

struct BenchDir(PathBuf);

impl BenchDir {
    fn new() -> io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "clock before epoch"))?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "nutdb-bench-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for BenchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
