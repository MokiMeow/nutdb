# 07 — Raft

The implementation follows the state and receiver rules in Figure 2 of the Raft
paper.

## Persistent state

Every node persists its term, vote, and log through the checksummed WAL. A vote
grant or successful append response is not returned until the new state has
been synchronized. A restart replays the last complete state record and repairs
any torn tail.

## Elections

Election timeouts are randomized in the 150–300 ms interval from a reproducible
seed. A candidate increments and persists its term, votes for itself, then asks
peers. A follower grants at most one vote per term and only when the candidate's
last `(term, index)` is at least as new as its own.

## Replication

AppendEntries checks `prev_log_index` and `prev_log_term`. A mismatch rejects
the request. A conflicting entry deletes that entry and its suffix before the
leader's suffix is appended. Commit propagation uses
`min(leader_commit, last_log_index)`.

Leaders advance commit only when a majority match an index whose entry belongs
to the leader's current term. This subtle rule is what preserves leader
completeness across term changes.

## Apply

`commit_index` and `last_applied` are distinct. The apply loop walks each newly
committed entry in index order and advances `last_applied`; repeated heartbeats
therefore cannot apply the same command twice.

## Testing

The deterministic in-memory network can isolate nodes and drop/reorder queued
messages. Safety tests cover election safety, stale candidates, minority
leaders, suffix repair, current-term commit, persistence boundaries, and
exactly-once apply. TCP transport uses a big-endian 32-bit length prefix with a
16 MiB limit.

Reference: Ongaro and Ousterhout, *In Search of an Understandable Consensus
Algorithm*, especially Figure 2 and section 5.4.
