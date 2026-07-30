# 06: MVCC

nutdb implements snapshot isolation with durable transaction boundaries and
first-committer-wins conflicts.

## Identity versus time

Two counters have different jobs:

- A transaction ID identifies one attempt and is persisted before `begin`
  returns, so it cannot repeat after restart.
- A commit timestamp is assigned while committing under the store lock and
  defines visibility.

They must be separate. Transaction 1 can commit after transaction 2; a snapshot
opened between those commits must not see transaction 1 merely because its ID
is numerically smaller.

## Visibility

A version is visible at snapshot `s` when:

```
created_at <= s && (deleted_at is absent || deleted_at > s)
```

Writes create versions rather than overwriting values. Deletes timestamp the
current version. A transaction consults its private write set first, which gives
read-your-own-writes without exposing uncommitted data to anyone else.

## Conflicts

At commit, every written key is checked for a modification after the
transaction's snapshot. If one exists, the transaction writes a durable abort
record and returns `TxnError::Conflict`; otherwise all its earlier write records
become visible through one synchronized commit record.

This is first-committer-wins. It prevents lost updates on the same key.

## Crash recovery

Recovery buffers `Set` and `Delete` records by transaction ID. It applies a
buffer only when a valid `Commit` follows. An incomplete record is removed by
the WAL repair path; a complete write batch with no commit is ignored. Thus a
crash immediately before the commit record has no partial effect, while a crash
immediately after its `fsync` recovers the complete transaction.

## Watermark and GC

The watermark is the oldest snapshot among active transactions. A version whose
deletion timestamp is older than that watermark cannot be seen by any live or
future transaction and can be reclaimed. Tests hold an old reader open, run GC,
verify its version survives, close it, and verify a later GC reclaims it.

## Guarantee and limitation

Snapshot isolation supplies stable transaction snapshots, no dirty reads, no
non-repeatable reads, no phantoms within a snapshot, and same-key write conflict
detection. It is not serializable: write skew remains possible when concurrent
transactions read overlapping data and update disjoint keys. Serializable
snapshot isolation is a future extension.

References: Berenson et al., *A Critique of ANSI SQL Isolation Levels*; Fekete
et al., *Making Snapshot Isolation Serializable*.
