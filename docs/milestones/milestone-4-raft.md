# Milestone 4 — Raft consensus ✅

**Goal:** implement the Raft Figure 2 safety rules with persisted state and a
transport that can manufacture network faults.

## What shipped

- [x] Persisted `current_term`, `voted_for`, and complete log snapshots in the
      checksummed WAL.
- [x] Follower, candidate, and leader roles with deterministic randomized
      150–300 ms election timeouts.
- [x] RequestVote with one vote per term and last-term/last-index freshness.
- [x] AppendEntries consistency checks, conflicting-suffix truncation,
      heartbeats, and commit propagation.
- [x] Majority commit restricted to entries from the leader's current term.
- [x] Ordered exactly-once apply loop.
- [x] A controllable in-memory transport that partitions, drops, delays, and
      reorders messages, plus checked length-prefixed TCP frames.
- [x] A deterministic cluster harness for election, replication, stopping,
      partitions, healing, and convergence.

## Persistence boundary

Term changes, granted votes, and log mutations append and synchronize a complete
persistent-state record before a successful RPC response is returned. Tests
drop and reopen a node immediately after the response and verify the vote/log
is already present.

## Definition of Done

- [x] Three nodes elect exactly one leader and replace a stopped leader.
- [x] Thirty-two seeded runs assert at most one leader per term.
- [x] A stale candidate cannot win a vote from a node containing a
      majority-committed entry.
- [x] A minority old leader cannot commit.
- [x] Conflicting logs converge after partition healing.
- [x] Current-term-only commit advancement is tested directly.
- [x] Persist-before-response is verified for votes and appended entries.
- [x] Apply order is exact and repeated heartbeats do not reapply commands.
- [x] Linux release build is warning-free and all earlier suites remain green.

**Next:** [Milestone 5 — Cluster](milestone-5-cluster.md).
