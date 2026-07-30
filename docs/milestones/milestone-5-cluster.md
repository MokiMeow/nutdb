# Milestone 5: Cluster + fault injection

**Goal:** run three real nodes, break them on purpose, and prove no acknowledged
write is ever lost. This is the milestone that turns the project from "a
database" into "a database you can trust."

## Concepts

Linearizability, fault injection, and Jepsen-style verification.

## Tasks

- [x] **Real cluster**: three processes over TCP, configured with a peer list,
      each with its own data directory. `cargo run -- serve --id 1 --peers …`
- [x] **Client**: routes writes to the leader, follows leader redirects, and
      retries with a bounded backoff.
- [x] **Fault-injection harness**: a test driver that, while a workload runs,
      (a) kills the leader, (b) partitions a node from the others, (c) pauses a
      process (SIGSTOP/SIGCONT), (d) restarts a node from disk.
- [x] **History recording**: log every client operation as
      `invoke` / `ok` / `fail` / `info(timeout)` with timestamps: a timeout is
      **not** a failure; the write may still have committed, and treating it as
      failed is the classic checker bug.
- [x] **Linearizability checker**: verify the recorded history is consistent with
      *some* sequential execution respecting real-time ordering. A small
      Knossos-style search is enough for short histories.
- [x] **The demo**: a scripted run that kills the leader mid-workload and shows
      the cluster continuing, with every acknowledged write still readable.

## Files

`src/server.rs`, `src/client.rs`, `src/linearizability.rs`, `tests/cluster.rs`,
`scripts/cluster-demo.sh`, `docs/09-testing.md`.

## Definition of Done

- [x] A 3-node cluster serves reads and writes over TCP.
- [x] Killing the leader mid-workload: a new leader is elected and **every
      previously acknowledged write is still readable**.
- [x] A minority partition cannot commit; the majority side keeps serving.
- [x] A restarted node catches up from the leader's log and converges.
- [x] The linearizability checker passes on recorded histories, and is itself
      validated by **feeding it a deliberately broken history that it must
      reject** (a checker that always passes proves nothing).
- [x] The scripted demo runs end to end and is recorded for the README.

## Notes

Two traps worth naming:

1. **Timeouts are not failures.** An operation that timed out may have
   committed. Record it as indeterminate; a checker that assumes failure will
   report phantom violations.
2. **Test the checker.** Before trusting a green result, hand it a history with
   a known violation and confirm it fails.

The TCP integration layer deliberately remains small: it elects the
lowest-numbered node in the currently reachable majority, stages each mutation
on peers, and acknowledges only after a majority durably stores the commit.
The complete Raft Figure 2 state machine is tested independently in milestone
4. The TCP layer does not yet carry those Raft RPCs; wiring the two together is
listed as post-1.0 work rather than being hidden behind a stronger claim.

## References

- Kyle Kingsbury's Jepsen analyses: the model for this milestone.
- Herlihy & Wing, *Linearizability: A Correctness Condition for Concurrent
  Objects*.

**Next:** [Milestone 6: Polish](milestone-6-polish.md).
