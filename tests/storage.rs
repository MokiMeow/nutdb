//! Milestone 1 storage tests: force splits, reopen page snapshots, manufacture
//! interrupted checkpoints, and prove the WAL remains bounded.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use nutdb::Store;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "nutdb-storage-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn wal(&self) -> PathBuf {
        self.0.join("nutdb.wal")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn one_hundred_thousand_keys_split_and_survive_reopen() {
    let dir = TempDir::new("100k");
    let path = dir.wal();
    {
        let mut store = Store::open(&path).unwrap();
        store
            .set_batch((0..100_000).map(|i| (format!("key-{i:06}"), format!("value-{i}"))))
            .unwrap();
        store.checkpoint().unwrap();
        assert_eq!(store.wal_size().unwrap(), 0);
    }

    let store = Store::open(&path).unwrap();
    assert_eq!(store.len(), 100_000);
    for i in [0, 1, 42, 9_999, 50_000, 99_999] {
        assert_eq!(
            store.get(&format!("key-{i:06}")),
            Some(format!("value-{i}").as_str())
        );
    }
    assert_eq!(store.recovery().records_replayed, 0);
}

#[test]
fn range_scan_is_sorted_and_half_open() {
    let dir = TempDir::new("range");
    let mut store = Store::open(dir.wal()).unwrap();
    store
        .set_batch((0..100).rev().map(|i| (format!("{i:03}"), i.to_string())))
        .unwrap();
    let rows = store.range("020", "025");
    let keys: Vec<&str> = rows.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys, ["020", "021", "022", "023", "024"]);
}

#[test]
fn interrupted_checkpoint_keeps_previous_snapshot_and_wal() {
    let dir = TempDir::new("checkpoint-crash");
    let path = dir.wal();
    {
        let mut store = Store::open(&path).unwrap();
        store.set("before", "safe").unwrap();
        store.checkpoint().unwrap();
        store.set("after", "also-safe").unwrap();
    }

    // A process died while writing the replacement page file. The committed
    // snapshot and WAL are untouched; the orphan .tmp must be ignored.
    fs::write(format!("{}.pages.tmp", path.display()), b"partial page").unwrap();

    let store = Store::open(&path).unwrap();
    assert_eq!(store.get("before"), Some("safe"));
    assert_eq!(store.get("after"), Some("also-safe"));
    assert_eq!(store.recovery().records_replayed, 1);
}

#[test]
fn partial_checkpoint_record_recovers_from_the_synced_snapshot() {
    let dir = TempDir::new("partial-checkpoint-record");
    let path = dir.wal();
    {
        let mut store = Store::open(&path).unwrap();
        store.set("stable", "snapshot").unwrap();
        store.checkpoint().unwrap();
    }
    {
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&1u32.to_le_bytes()).unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap();
        file.sync_all().unwrap();
    }

    let store = Store::open(&path).unwrap();
    assert_eq!(store.get("stable"), Some("snapshot"));
    assert!(store.recovery().truncated);
    assert_eq!(store.recovery().records_replayed, 0);
}

#[test]
fn repeated_checkpoints_bound_the_wal() {
    let dir = TempDir::new("bounded-wal");
    let path = dir.wal();
    let mut store = Store::open(&path).unwrap();
    for round in 0..20 {
        store
            .set_batch((0..50).map(|i| {
                (
                    format!("key-{i:03}"),
                    format!("round-{round}-value-{i}"),
                )
            }))
            .unwrap();
        store.checkpoint().unwrap();
        assert_eq!(store.wal_size().unwrap(), 0);
    }
    drop(store);

    let store = Store::open(&path).unwrap();
    assert_eq!(store.len(), 50);
    assert_eq!(store.get("key-049"), Some("round-19-value-49"));
}

#[test]
fn appends_after_torn_tail_remain_replayable() {
    let dir = TempDir::new("repair-tail");
    let path = dir.wal();
    {
        let mut store = Store::open(&path).unwrap();
        store.set("before", "safe").unwrap();
    }
    {
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&999u32.to_le_bytes()).unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap();
        file.write_all(b"torn").unwrap();
        file.sync_all().unwrap();
    }
    {
        let mut recovered = Store::open(&path).unwrap();
        assert!(recovered.recovery().truncated);
        recovered.set("after", "durable").unwrap();
    }

    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.get("before"), Some("safe"));
    assert_eq!(reopened.get("after"), Some("durable"));
    assert!(!reopened.recovery().truncated);
}
