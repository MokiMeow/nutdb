# Milestone 2 — MVCC and transactions

**Goal:** concurrent transactions that read a consistent snapshot without
blocking writers — the feature that separates a key-value store from a database.

## Concepts

Multi-version concurrency control, snapshot isolation, transaction ids and
watermarks, garbage collection of dead versions, and write-write conflicts.

## Tasks

- [ ] **Transaction ids**: a monotonically increasing counter, persisted so ids
      never repeat after a restart.
- [ ] **Versioned rows**: each value carries `(created_txn, deleted_txn)`. A
      write creates a new version rather than overwriting.
- [ ] **Snapshot reads**: a transaction sees versions created before its
      snapshot and not deleted before it — no locks, no blocking.
- [ ] **Write-write conflicts**: two concurrent transactions writing the same
      key — the second to commit aborts (first-committer-wins).
- [ ] **Commit/abort**: log a commit record; recovery must roll back
      transactions with no commit record.
- [ ] **Watermark + GC**: track the oldest active snapshot and reclaim versions
      no live transaction can see.
- [ ] **Tests**: a reader sees a consistent snapshot while a writer commits
      concurrently; a write-write conflict aborts exactly one side; recovery
      discards uncommitted work; GC does not reclaim a version a live
      transaction still needs.

## Files

`src/txn.rs`, `src/version.rs`, `src/gc.rs`, `src/store.rs` (versioned),
`tests/mvcc.rs`, `docs/06-mvcc.md`.

## Definition of Done

- [ ] A long-running reader observes a stable snapshot while writes commit
      around it (no phantom reads within the snapshot).
- [ ] Concurrent writers to the same key: exactly one commits, the other gets a
      conflict error.
- [ ] A crash mid-transaction leaves **no** partial effects after recovery —
      tested by killing between the writes and the commit record.
- [ ] GC reclaims versions and a test proves it never reclaims a visible one.
- [ ] Build warning-free; all earlier tests still green.

## Notes

Snapshot isolation does **not** prevent write skew. Say so explicitly in the
docs rather than implying serializability; upgrading to SSI (serializable
snapshot isolation) is a documented stretch goal. Overclaiming an isolation
level is the most common way database projects mislead.

**Next:** [Milestone 3 — SQL](milestone-3-sql.md).
