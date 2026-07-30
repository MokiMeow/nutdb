# 09: Cluster testing

NutDB tests distributed behavior at three levels:

1. `src/raft/` uses a deterministic in-memory transport to exhaustively exercise
   election, log matching, current-term commit, persistence, partitions, and
   ordered exactly-once application.
2. `tests/cluster.rs` starts real TCP servers with separate durable data
   directories and tests leader death, minority refusal, healing, restart
   catch-up, and recorded histories.
3. `scripts/cluster-demo.sh` runs three operating-system processes, pauses and
   resumes one follower, kills the leader, and checks that all acknowledged
   writes remain readable after failover.

Run the complete suite:

```bash
cargo test
bash scripts/cluster-demo.sh
```

The demo prints:

```text
[node1] leader
[node2] follower
[node3] follower
ok
pausing node3
ok
killing node1
[node2] leader
ok
verified: every acknowledged write is readable after leader failover
```

## Linearizability histories

Each operation is recorded as an invocation and one of:

- `ok`: the result is known and constrains the sequential history;
- `fail`: the operation did not take effect;
- `info`: the client timed out, so a write may or may not have committed.

The checker searches for a legal sequential register history while respecting
real-time order. Tests feed it both a real TCP history and a deliberately
impossible stale-read history. An `info` write branches into committed and
not-committed possibilities, avoiding the classic false violation caused by
treating a timeout as a definite failure.

## Fault model and limits

The integration tests cover crash-stop failure, process pause, restart, and
explicit peer blocking. They do not simulate Byzantine nodes, disk firmware
lying about `fsync`, or arbitrary packet reordering. TCP replication uses
majority fencing and durable staging; the standalone Raft state machine is not
yet used as the live TCP server protocol.
