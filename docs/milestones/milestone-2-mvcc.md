# Milestone 2: MVCC and transactions ✅

**Goal:** concurrent transactions read stable snapshots without blocking
writers, while same-key writers use first-committer-wins conflict handling.

## What shipped

- [x] Transaction IDs persisted in a synchronized `Begin` record before begin
      succeeds. Recovery advances past every observed ID, including abandoned
      transactions.
- [x] A separate monotonically increasing commit timestamp. Visibility follows
      commit order, not transaction allocation order.
- [x] Version chains carrying `created_at` and `deleted_at` timestamps.
- [x] Snapshot reads plus read-your-own-writes.
- [x] First-committer-wins write conflicts. A key changed after a transaction's
      snapshot causes that transaction to abort.
- [x] Logged write records followed by a durable commit/abort record. Recovery
      buffers writes and applies only batches with an intact commit.
- [x] Active-snapshot watermark and garbage collection of versions no live
      transaction can observe.
- [x] Thread-safe `MvccStore` / `Transaction` APIs using only `std`.

## WAL format

The MVCC store uses the checksummed WAL framing from milestone 0:

```
Begin:  [0x10][txn:u64][snapshot:u64]
Set:    [0x11][txn:u64][klen:u32][key][vlen:u32][value]
Delete: [0x12][txn:u64][klen:u32][key]
Commit: [0x13][txn:u64][commit_ts:u64]
Abort:  [0x14][txn:u64]
```

Writes may reach the log before commit. That is safe because recovery keeps
them pending until an intact commit record appears.

## Definition of Done

- [x] A long-running reader sees its original value and no newly inserted
      phantom while another transaction commits.
- [x] Two overlapping writers to one key produce exactly one commit and one
      conflict.
- [x] A crash after write records but before commit leaves no partial effects.
- [x] Transaction IDs are not reused after that crash.
- [x] A test proves snapshots follow commit order when an older transaction
      commits late.
- [x] GC preserves a version held by a live reader and reclaims it after that
      reader ends.
- [x] Deletes and aborts have version-correct behavior.
- [x] Linux release build is warning-free and every earlier test remains green.

## Isolation guarantee

This is snapshot isolation, not serializability. It prevents dirty reads,
non-repeatable reads, phantoms within a transaction snapshot, and same-key lost
updates. It permits write skew across disjoint keys.

**Next:** [Milestone 3: SQL](milestone-3-sql.md).
