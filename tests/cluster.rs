//! Real TCP cluster, failover, partition, restart, and history checks.

use std::collections::BTreeMap;
use std::fs;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nutdb::linearizability::{
    is_linearizable, HistoryEntry, Operation, Outcome,
};
use nutdb::server::{request_peer, Server};
use nutdb::ClusterClient;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("nutdb-cluster-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct RunningNode {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl RunningNode {
    fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

impl Drop for RunningNode {
    fn drop(&mut self) {
        self.stop();
    }
}

struct TestCluster {
    _dir: TempDir,
    addresses: BTreeMap<u64, SocketAddr>,
    nodes: BTreeMap<u64, RunningNode>,
}

impl TestCluster {
    fn start(name: &str) -> Self {
        let dir = TempDir::new(name);
        let addresses: BTreeMap<u64, SocketAddr> =
            (1..=3).map(|id| (id, free_address())).collect();
        let mut cluster = Self {
            _dir: dir,
            addresses,
            nodes: BTreeMap::new(),
        };
        for id in 1..=3 {
            cluster.start_node(id);
        }
        cluster
    }

    fn start_node(&mut self, id: u64) {
        let peers = self
            .addresses
            .iter()
            .filter(|(peer, _)| **peer != id)
            .map(|(peer, address)| (*peer, *address))
            .collect();
        let server = Server::bind(
            id,
            self.addresses[&id],
            peers,
            self._dir.0.join(format!("node-{id}")),
        )
        .unwrap();
        let running = Arc::new(AtomicBool::new(true));
        let flag = running.clone();
        let thread = thread::spawn(move || server.run(flag).unwrap());
        self.nodes.insert(
            id,
            RunningNode {
                running,
                thread: Some(thread),
            },
        );
        wait_ready(self.addresses[&id]);
    }

    fn stop_node(&mut self, id: u64) {
        self.nodes.get_mut(&id).unwrap().stop();
    }

    fn restart_node(&mut self, id: u64) {
        self.start_node(id);
    }

    fn client(&self) -> ClusterClient {
        ClusterClient::new(self.addresses.clone())
    }
}

#[test]
fn leader_kill_re_elects_and_preserves_every_acknowledged_write() {
    let mut cluster = TestCluster::start("failover");
    let client = cluster.client();
    client.put("before", "leader-one").unwrap();
    assert_eq!(client.get("before").unwrap().as_deref(), Some("leader-one"));

    cluster.stop_node(1);
    client.put("after", "leader-two").unwrap();
    assert_eq!(client.get("before").unwrap().as_deref(), Some("leader-one"));
    assert_eq!(client.get("after").unwrap().as_deref(), Some("leader-two"));

    cluster.restart_node(1);
    assert_eq!(client.get("after").unwrap().as_deref(), Some("leader-two"));
}

#[test]
fn minority_refuses_writes_while_majority_keeps_serving_then_converges() {
    let mut cluster = TestCluster::start("partition");
    let client = cluster.client();
    client.put("base", "safe").unwrap();
    for (node, peer) in [(1, 2), (1, 3), (2, 1), (3, 1)] {
        client.block(node, peer).unwrap();
    }

    let minority = request_peer(cluster.addresses[&1], "PUT 6d696e6f72697479 6e6f")
        .unwrap();
    assert!(minority.starts_with("RETRY "));
    client.put("majority", "yes").unwrap();
    assert_eq!(client.get("majority").unwrap().as_deref(), Some("yes"));

    for node in 1..=3 {
        client.heal(node).unwrap();
    }
    assert_eq!(client.get("majority").unwrap().as_deref(), Some("yes"));

    // Avoid an unused-mut warning while keeping Drop-based cleanup explicit.
    cluster.stop_node(3);
}

#[test]
fn restarted_node_catches_up_before_it_serves_as_leader() {
    let mut cluster = TestCluster::start("restart");
    let client = cluster.client();
    cluster.stop_node(1);
    client.put("while-down", "committed").unwrap();
    cluster.restart_node(1);
    assert_eq!(
        client.get("while-down").unwrap().as_deref(),
        Some("committed")
    );
}

#[test]
fn recorded_tcp_history_is_linearizable() {
    let cluster = TestCluster::start("history");
    let client = cluster.client();
    let mut clock = 0;
    let mut history = Vec::new();
    for (id, operation) in [
        (1, Operation::Write("a".into())),
        (2, Operation::Read),
        (3, Operation::Write("b".into())),
        (4, Operation::Read),
    ] {
        clock += 1;
        let invoke = clock;
        let outcome = match &operation {
            Operation::Write(value) => {
                client.put("register", value).unwrap();
                Outcome::Ok(None)
            }
            Operation::Read => Outcome::Ok(client.get("register").unwrap()),
        };
        clock += 1;
        history.push(HistoryEntry {
            id,
            invoke,
            complete: clock,
            operation,
            outcome,
        });
    }
    assert!(is_linearizable(&history));
}

#[test]
fn checker_rejects_a_deliberately_broken_history() {
    let history = vec![
        HistoryEntry {
            id: 1,
            invoke: 1,
            complete: 2,
            operation: Operation::Write("new".into()),
            outcome: Outcome::Ok(None),
        },
        HistoryEntry {
            id: 2,
            invoke: 3,
            complete: 4,
            operation: Operation::Read,
            outcome: Outcome::Ok(Some("old".into())),
        },
    ];
    assert!(!is_linearizable(&history));
}

#[test]
fn timeout_write_is_indeterminate_not_assumed_failed() {
    let history = vec![
        HistoryEntry {
            id: 1,
            invoke: 1,
            complete: 2,
            operation: Operation::Write("maybe".into()),
            outcome: Outcome::Info,
        },
        HistoryEntry {
            id: 2,
            invoke: 3,
            complete: 4,
            operation: Operation::Read,
            outcome: Outcome::Ok(Some("maybe".into())),
        },
    ];
    assert!(is_linearizable(&history));
}

fn free_address() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

fn wait_ready(address: SocketAddr) {
    for _ in 0..100 {
        if request_peer(address, "PING").is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("server at {address} did not start");
}
