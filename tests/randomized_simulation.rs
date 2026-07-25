//! Seeded fault simulation: the seed is printed in every assertion message so
//! a failing run is exactly replayable.

use std::fs;
use std::path::PathBuf;

use nutdb::raft::state::Role;
use nutdb::raft::RaftCluster;

const SEED: u64 = 0x4e55_5444_4231_0001;
const STEPS: usize = 200;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("nutdb-random-{}-{SEED:x}", std::process::id()));
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
fn seeded_random_operations_and_faults_preserve_raft_safety() {
    let dir = TempDir::new();
    let mut random = Random(SEED);
    let mut cluster = RaftCluster::open(&dir.0, &[1, 2, 3], SEED).unwrap();
    let mut stopped = [false; 3];
    let mut acknowledged = Vec::new();

    for step in 0..STEPS {
        let node = random.pick_node();
        match random.next() % 7 {
            0 => {
                let _ = cluster.elect(node);
            }
            1 => cluster.isolate(node),
            2 => cluster.heal_all(),
            3 => {
                if stopped.iter().filter(|stopped| !**stopped).count() > 1 {
                    cluster.stop(node);
                    stopped[(node - 1) as usize] = true;
                }
            }
            4 => {
                cluster.start(node);
                stopped[(node - 1) as usize] = false;
            }
            _ => {
                if let Some(leader) = cluster.leader() {
                    let command = format!("seed-{SEED:x}-step-{step}").into_bytes();
                    if cluster.replicate(leader, command.clone()).is_ok() {
                        acknowledged.push(command);
                    }
                }
            }
        }
        assert_safety(&cluster, step);
    }

    for node in 1..=3 {
        cluster.start(node);
    }
    cluster.heal_all();
    let leader = (1..=3)
        .find(|candidate| cluster.elect(*candidate).unwrap())
        .expect("healed cluster elects a leader");
    cluster.synchronize(leader).unwrap();
    cluster.synchronize(leader).unwrap();
    assert_safety(&cluster, STEPS);

    for command in acknowledged {
        for node in 1..=3 {
            assert!(
                cluster.applied(node).contains(&command),
                "seed {SEED:#x}: acknowledged command missing on node {node}"
            );
        }
    }
}

fn assert_safety(cluster: &RaftCluster, step: usize) {
    for term in 0..=cluster
        .node(1)
        .term()
        .max(cluster.node(2).term())
        .max(cluster.node(3).term())
    {
        let leaders = (1..=3)
            .filter(|node| {
                cluster.node(*node).term() == term && cluster.node(*node).role() == Role::Leader
            })
            .count();
        assert!(
            leaders <= 1,
            "seed {SEED:#x}, step {step}: two leaders in term {term}"
        );
    }
    for left in 1..=3 {
        for right in left + 1..=3 {
            let a = cluster.applied(left);
            let b = cluster.applied(right);
            let shared = a.len().min(b.len());
            assert_eq!(
                &a[..shared],
                &b[..shared],
                "seed {SEED:#x}, step {step}: applied logs diverged"
            );
        }
    }
}

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick_node(&mut self) -> u64 {
        self.next() % 3 + 1
    }
}
