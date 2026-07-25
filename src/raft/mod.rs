//! Raft consensus and a deterministic three-node simulation harness.

pub mod rpc;
pub mod state;
pub mod transport;

use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::Path;

use rpc::AppendEntries;
use state::{RaftNode, Role};
use transport::InMemoryTransport;

pub struct RaftCluster {
    nodes: BTreeMap<u64, RaftNode>,
    transport: InMemoryTransport,
    stopped: HashSet<u64>,
    applied: BTreeMap<u64, Vec<Vec<u8>>>,
}

impl RaftCluster {
    pub fn open(path: impl AsRef<Path>, ids: &[u64], seed: u64) -> io::Result<Self> {
        std::fs::create_dir_all(path.as_ref())?;
        let mut nodes = BTreeMap::new();
        let mut applied = BTreeMap::new();
        for id in ids {
            nodes.insert(
                *id,
                RaftNode::open(
                    *id,
                    path.as_ref().join(format!("node-{id}.raft.wal")),
                    seed,
                )?,
            );
            applied.insert(*id, Vec::new());
        }
        Ok(Self {
            nodes,
            transport: InMemoryTransport::default(),
            stopped: HashSet::new(),
            applied,
        })
    }

    pub fn node(&self, id: u64) -> &RaftNode {
        self.nodes.get(&id).expect("cluster node")
    }

    pub fn leader(&self) -> Option<u64> {
        self.nodes
            .iter()
            .find(|(id, node)| !self.stopped.contains(id) && node.role() == Role::Leader)
            .map(|(id, _)| *id)
    }

    pub fn elect(&mut self, candidate: u64) -> io::Result<bool> {
        if self.stopped.contains(&candidate) {
            return Ok(false);
        }
        let request = self
            .nodes
            .get_mut(&candidate)
            .ok_or_else(|| invalid("raft: unknown candidate"))?
            .start_election()?;
        let mut votes = 1;
        let ids: Vec<u64> = self.nodes.keys().copied().collect();
        for peer in ids {
            if peer == candidate
                || self.stopped.contains(&peer)
                || !self.transport.reachable(candidate, peer)
                || !self.transport.reachable(peer, candidate)
            {
                continue;
            }
            let response = self
                .nodes
                .get_mut(&peer)
                .expect("peer")
                .request_vote(&request)?;
            if response.term > request.term {
                self.nodes
                    .get_mut(&candidate)
                    .expect("candidate")
                    .observe_term(response.term)?;
                return Ok(false);
            }
            if response.granted {
                votes += 1;
            }
        }
        if votes * 2 > self.nodes.len() {
            self.nodes
                .get_mut(&candidate)
                .expect("candidate")
                .become_leader();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn elect_by_timeout(&mut self) -> io::Result<Option<u64>> {
        let candidate = self
            .nodes
            .iter()
            .filter(|(id, _)| !self.stopped.contains(id))
            .min_by_key(|(_, node)| node.election_timeout_ms())
            .map(|(id, _)| *id);
        match candidate {
            Some(id) if self.elect(id)? => Ok(Some(id)),
            _ => Ok(None),
        }
    }

    pub fn replicate(&mut self, leader: u64, command: Vec<u8>) -> io::Result<u64> {
        if self.stopped.contains(&leader) {
            return Err(io::Error::new(io::ErrorKind::NotConnected, "raft: node stopped"));
        }
        let index = self
            .nodes
            .get_mut(&leader)
            .ok_or_else(|| invalid("raft: unknown leader"))?
            .append_as_leader(command)?;
        self.replicate_existing(leader)?;
        if self.nodes[&leader].commit_index() < index {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "raft: entry did not reach a majority",
            ));
        }
        Ok(index)
    }

    pub fn synchronize(&mut self, leader: u64) -> io::Result<()> {
        self.replicate_existing(leader).map(|_| ())
    }

    fn replicate_existing(&mut self, leader: u64) -> io::Result<u64> {
        let term = self.nodes[&leader].term();
        let leader_log = self.nodes[&leader].log().to_vec();
        let leader_commit = self.nodes[&leader].commit_index();
        let ids: Vec<u64> = self.nodes.keys().copied().collect();
        let mut matched = Vec::new();

        for follower in ids.iter().copied() {
            if follower == leader {
                continue;
            }
            if self.stopped.contains(&follower)
                || !self.transport.reachable(leader, follower)
            {
                matched.push(0);
                continue;
            }
            let mut success = false;
            for prev in (0..=leader_log.len()).rev() {
                let request = AppendEntries {
                    term,
                    leader_id: leader,
                    prev_log_index: prev as u64,
                    prev_log_term: if prev == 0 {
                        0
                    } else {
                        leader_log[prev - 1].term
                    },
                    entries: leader_log[prev..].to_vec(),
                    leader_commit,
                };
                let response = self
                    .nodes
                    .get_mut(&follower)
                    .expect("follower")
                    .append_entries(&request)?;
                if response.term > term {
                    self.nodes
                        .get_mut(&leader)
                        .expect("leader")
                        .observe_term(response.term)?;
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "raft: leader observed a higher term",
                    ));
                }
                if response.success {
                    matched.push(response.match_index);
                    success = true;
                    break;
                }
            }
            if !success {
                matched.push(0);
            }
        }

        let commit = self
            .nodes
            .get_mut(&leader)
            .expect("leader")
            .advance_commit(&matched);

        // Publish the new commit index and apply in order, exactly once.
        let ids: Vec<u64> = self.nodes.keys().copied().collect();
        for follower in ids.iter().copied() {
            if follower == leader
                || self.stopped.contains(&follower)
                || !self.transport.reachable(leader, follower)
            {
                continue;
            }
            let follower_len = self.nodes[&follower].last_log_index();
            let request = AppendEntries {
                term,
                leader_id: leader,
                prev_log_index: follower_len,
                prev_log_term: if follower_len == 0 {
                    0
                } else {
                    leader_log[(follower_len - 1) as usize].term
                },
                entries: Vec::new(),
                leader_commit: commit,
            };
            self.nodes
                .get_mut(&follower)
                .expect("follower")
                .append_entries(&request)?;
        }
        self.apply_all();
        Ok(commit)
    }

    fn apply_all(&mut self) {
        let ids: Vec<u64> = self.nodes.keys().copied().collect();
        for id in ids {
            let commands = self.nodes.get_mut(&id).expect("node").take_applied();
            self.applied
                .get_mut(&id)
                .expect("applied log")
                .extend(commands.into_iter().map(|entry| entry.command));
        }
    }

    pub fn partition(&mut self, a: u64, b: u64) {
        self.transport.partition(a, b);
    }

    pub fn isolate(&mut self, node: u64) {
        for peer in self.nodes.keys().copied().collect::<Vec<_>>() {
            if peer != node {
                self.transport.partition(node, peer);
            }
        }
    }

    pub fn heal_all(&mut self) {
        self.transport.heal_all();
    }

    pub fn stop(&mut self, node: u64) {
        self.stopped.insert(node);
    }

    pub fn start(&mut self, node: u64) {
        self.stopped.remove(&node);
    }

    pub fn applied(&self, node: u64) -> &[Vec<u8>] {
        self.applied.get(&node).expect("node")
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
