# Milestone 4 — Raft consensus

**Goal:** replicate the log across nodes with leader election and log
replication. This is the hardest milestone and the one almost nobody attempts.

## Concepts

Terms, leader election, log matching, commit index, and the safety properties
that make consensus *correct* rather than merely working.

## Tasks

- [ ] **State**: `currentTerm`, `votedFor`, and the log **persisted before
      responding to any RPC** — this is a Raft safety requirement, not an
      optimisation. Reuse milestone 0's WAL.
- [ ] **Roles**: follower / candidate / leader, with randomised election
      timeouts (150–300 ms) to avoid split votes.
- [ ] **RequestVote RPC**: grant a vote only if the candidate's log is at least
      as up to date as ours (compare last term, then last index).
- [ ] **AppendEntries RPC**: heartbeats plus replication; the consistency check
      (`prevLogIndex`/`prevLogTerm`) and truncation of conflicting suffixes.
- [ ] **Commit index**: advance only when a majority has replicated an entry
      **from the current term** — the subtle rule that prevents committed
      entries from being overwritten.
- [ ] **Apply loop**: committed entries are applied to the state machine (the
      store) in index order, exactly once.
- [ ] **Transport**: TCP with a simple length-prefixed message format (no
      dependencies). Tests use an in-memory transport that can drop, delay, and
      reorder messages.
- [ ] **Tests** on the in-memory transport: a leader is elected from 3 nodes;
      exactly one leader per term; a partitioned old leader cannot commit; logs
      converge after a partition heals; an entry committed by a majority is
      never lost across leader changes.

## Files

`src/raft/mod.rs`, `src/raft/state.rs`, `src/raft/rpc.rs`,
`src/raft/transport.rs`, `tests/raft.rs`, `docs/07-raft.md`.

## Definition of Done

- [ ] Three nodes elect exactly one leader, and re-elect within a bounded time
      when the leader stops.
- [ ] **Election safety**: a test asserts at most one leader per term across
      many randomised runs.
- [ ] **Log matching**: after a partition heals, all logs are identical at every
      index they share.
- [ ] **Leader completeness**: an entry acknowledged as committed is present in
      every subsequent leader's log — tested across forced leader changes.
- [ ] Persistent state is written **before** any RPC response (tested by
      crashing between the two).
- [ ] Build warning-free; all earlier tests green.

## Notes

Implement the rules *literally* from the paper's Figure 2 and cite it in the
code. Raft's failure mode is that an approximate implementation passes casual
testing and silently loses committed data under a specific partition. The
in-memory transport with controllable message loss is what turns "seems to work"
into evidence.

## References

- Ongaro & Ousterhout, *In Search of an Understandable Consensus Algorithm*
  (the Raft paper) — especially Figure 2 and §5.4 safety.
- <https://raft.github.io/> — visualisations.

**Next:** [Milestone 5 — Cluster](milestone-5-cluster.md).
