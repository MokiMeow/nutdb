# 06 — MVCC

*Milestone 2.* Multi-version concurrency control: readers see a consistent
snapshot without blocking writers, and writers never block readers.

## The idea

Never overwrite a row. Every write creates a **new version** tagged with the
transaction that created it:

```
key "balance"
   ├─ version A: 100   created_txn=5   deleted_txn=9
   └─ version B: 150   created_txn=9   deleted_txn=None   ← current
```

A transaction with snapshot `t` sees the version where
`created_txn <= t` and (`deleted_txn` is none or `deleted_txn > t`).

Because old versions still exist, a long-running read never blocks and never
sees a half-applied write.

## Snapshot isolation — and its limit

Each transaction takes a snapshot at start and reads consistently from it.
Write-write conflicts are resolved **first-committer-wins**: the second
transaction to commit a conflicting key aborts.

**This is not serializability.** Snapshot isolation permits *write skew*: two
transactions read overlapping data, write disjoint keys, and both commit —
producing a state no serial order could. The classic example is two on-call
doctors each checking "is someone else on duty?", seeing yes, and both going
off duty.

nutdb documents this honestly rather than claiming serializability. Upgrading to
SSI (serializable snapshot isolation) by tracking read-write dependencies is a
documented stretch goal.

## Garbage collection

Old versions accumulate. Track a **watermark** — the oldest snapshot any live
transaction is using — and reclaim versions no live transaction can see:

```
reclaimable  ⟺  deleted_txn is set AND deleted_txn < watermark
```

The failure mode to test: a long-running transaction holds the watermark back,
and GC must **not** reclaim a version it can still see. That test is the
milestone's real Definition of Done.

## Recovery interaction

Transaction boundaries go in the log. Recovery must:

1. replay committed transactions, and
2. **discard** work from transactions with no commit record.

Test it by crashing between the writes and the commit record — the partial
transaction must leave no trace.

## References

- Berenson et al., *A Critique of ANSI SQL Isolation Levels* (defines snapshot
  isolation and write skew)
- Fekete et al., *Making Snapshot Isolation Serializable* (the SSI path)
- [docs/05 — Durability](05-durability.md)
