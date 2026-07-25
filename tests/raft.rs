//! Raft safety tests over deterministic in-memory delivery.

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use nutdb::raft::rpc::{AppendEntries, Entry, RequestVote};
use nutdb::raft::state::{RaftNode, Role};
use nutdb::raft::transport::{read_frame, write_frame, InMemoryTransport, Message};
use nutdb::raft::RaftCluster;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("nutdb-raft-{name}-{}", std::process::id()));
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

#[test]
fn three_nodes_elect_and_replace_a_stopped_leader() {
    let dir = TempDir::new("election");
    let mut cluster = RaftCluster::open(&dir.0, &[1, 2, 3], 7).unwrap();
    let first = cluster.elect_by_timeout().unwrap().unwrap();
    assert_eq!(
        [1, 2, 3]
            .iter()
            .filter(|id| cluster.node(**id).role() == Role::Leader)
            .count(),
        1
    );
    cluster.stop(first);
    let replacement = [1, 2, 3]
        .into_iter()
        .find(|id| *id != first && cluster.elect(*id).unwrap())
        .expect("replacement leader");
    assert_ne!(replacement, first);
    assert!(cluster.node(replacement).term() > cluster.node(first).term());
}

#[test]
fn at_most_one_leader_per_term_across_seeded_runs() {
    for seed in 0..32 {
        let dir = TempDir::new(&format!("safety-{seed}"));
        let mut cluster = RaftCluster::open(&dir.0, &[1, 2, 3], seed).unwrap();
        let leader = cluster.elect_by_timeout().unwrap().unwrap();
        let term = cluster.node(leader).term();
        assert_eq!(
            [1, 2, 3]
                .iter()
                .filter(|id| {
                    cluster.node(**id).term() == term
                        && cluster.node(**id).role() == Role::Leader
                })
                .count(),
            1
        );
    }
}

#[test]
fn isolated_old_leader_cannot_commit_and_new_leader_keeps_committed_entry() {
    let dir = TempDir::new("leader-completeness");
    let mut cluster = RaftCluster::open(&dir.0, &[1, 2, 3], 1).unwrap();
    assert!(cluster.elect(1).unwrap());
    cluster.replicate(1, b"committed".to_vec()).unwrap();
    cluster.isolate(1);
    assert!(cluster.replicate(1, b"minority".to_vec()).is_err());

    assert!(cluster.elect(2).unwrap());
    cluster.replicate(2, b"new-term".to_vec()).unwrap();
    assert!(cluster
        .node(2)
        .log()
        .iter()
        .any(|entry| entry.command == b"committed"));
    assert!(!cluster
        .applied(2)
        .iter()
        .any(|command| command == b"minority"));
}

#[test]
fn stale_candidate_cannot_replace_a_leader_with_a_committed_entry() {
    let dir = TempDir::new("stale-candidate");
    let mut cluster = RaftCluster::open(&dir.0, &[1, 2, 3], 11).unwrap();
    assert!(cluster.elect(1).unwrap());
    cluster.partition(1, 3);
    cluster.replicate(1, b"majority-only".to_vec()).unwrap();
    cluster.stop(1);

    assert!(!cluster.elect(3).unwrap(), "stale node must not win node 2's vote");
    assert!(cluster.elect(2).unwrap(), "node with committed entry should win");
    assert!(cluster
        .node(2)
        .log()
        .iter()
        .any(|entry| entry.command == b"majority-only"));
}

#[test]
fn conflicting_suffix_is_replaced_when_partition_heals() {
    let dir = TempDir::new("log-match");
    let mut cluster = RaftCluster::open(&dir.0, &[1, 2, 3], 2).unwrap();
    assert!(cluster.elect(1).unwrap());
    cluster.isolate(1);
    assert!(cluster.replicate(1, b"orphan".to_vec()).is_err());
    assert!(cluster.elect(2).unwrap());
    cluster.replicate(2, b"majority".to_vec()).unwrap();

    cluster.heal_all();
    cluster.synchronize(2).unwrap();
    assert_eq!(cluster.node(1).log(), cluster.node(2).log());
    assert_eq!(cluster.node(2).log(), cluster.node(3).log());
}

#[test]
fn vote_and_log_are_on_disk_before_success_response() {
    let dir = TempDir::new("persistence");
    let path = dir.0.join("node.wal");
    {
        let mut node = RaftNode::open(1, &path, 1).unwrap();
        let response = node
            .request_vote(&RequestVote {
                term: 4,
                candidate_id: 2,
                last_log_index: 0,
                last_log_term: 0,
            })
            .unwrap();
        assert!(response.granted);
    }
    {
        let mut reopened = RaftNode::open(1, &path, 1).unwrap();
        assert_eq!(reopened.term(), 4);
        assert_eq!(reopened.voted_for(), Some(2));
        let response = reopened
            .append_entries(&AppendEntries {
                term: 4,
                leader_id: 2,
                prev_log_index: 0,
                prev_log_term: 0,
                entries: vec![Entry {
                    term: 4,
                    command: b"durable".to_vec(),
                }],
                leader_commit: 0,
            })
            .unwrap();
        assert!(response.success);
    }
    let reopened = RaftNode::open(1, &path, 1).unwrap();
    assert_eq!(reopened.log()[0].command, b"durable");
}

#[test]
fn leader_commits_only_an_entry_from_its_current_term() {
    let dir = TempDir::new("current-term");
    let path = dir.0.join("leader.wal");
    let mut node = RaftNode::open(1, path, 1).unwrap();
    node.start_election().unwrap();
    node.become_leader();
    node.append_as_leader(b"old-term".to_vec()).unwrap();
    node.start_election().unwrap();
    node.become_leader();

    assert_eq!(node.advance_commit(&[1, 0]), 0);
    node.append_as_leader(b"current-term".to_vec()).unwrap();
    assert_eq!(node.advance_commit(&[2, 0]), 2);
}

#[test]
fn apply_loop_is_in_order_and_exactly_once() {
    let dir = TempDir::new("apply");
    let mut cluster = RaftCluster::open(&dir.0, &[1, 2, 3], 3).unwrap();
    assert!(cluster.elect(1).unwrap());
    cluster.replicate(1, b"a".to_vec()).unwrap();
    cluster.replicate(1, b"b".to_vec()).unwrap();
    cluster.synchronize(1).unwrap();
    cluster.synchronize(1).unwrap();
    for id in [1, 2, 3] {
        assert_eq!(cluster.applied(id), [b"a".to_vec(), b"b".to_vec()]);
    }
}

#[test]
fn in_memory_transport_can_partition_drop_and_reorder() {
    let mut transport = InMemoryTransport::default();
    transport.partition(1, 2);
    assert!(!transport.send(Message {
        from: 1,
        to: 2,
        payload: b"blocked".to_vec(),
    }));
    transport.heal(1, 2);
    for payload in [b"first".to_vec(), b"second".to_vec()] {
        assert!(transport.send(Message {
            from: 1,
            to: 2,
            payload,
        }));
    }
    assert_eq!(transport.deliver_last().unwrap().payload, b"second");
    assert_eq!(transport.drop_next().unwrap().payload, b"first");
}

#[test]
fn tcp_frames_are_length_prefixed_and_checked() {
    let mut bytes = Vec::new();
    write_frame(&mut bytes, b"raft").unwrap();
    assert_eq!(&bytes[..4], &4u32.to_be_bytes());
    assert_eq!(read_frame(&mut Cursor::new(bytes)).unwrap(), b"raft");
}
