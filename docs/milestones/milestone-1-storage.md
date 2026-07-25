# Milestone 1 — Storage engine

**Goal:** stop keeping the whole database in RAM. Pages on disk, a B-tree index,
and checkpointing so the log can be truncated instead of growing forever.

## Concepts

Page layout, B-tree nodes and splits, the buffer pool, checkpointing, and log
truncation.

## Tasks

- [ ] **Pager**: fixed 4 KiB pages, read/write by page id, with a simple buffer
      pool (LRU eviction). Dirty pages flush before the page is evicted.
- [ ] **Page layout**: a header (type, item count, free-space pointer) plus a
      slot array growing from the front and cell data from the back — the
      standard slotted-page design. Document the byte layout as a diagram.
- [ ] **B-tree**: search, insert with node splitting, delete, and range scan.
      Leaf nodes hold key→value; internal nodes hold separator keys.
- [ ] **Checkpointing**: flush dirty pages, write a checkpoint record, then
      truncate the WAL prefix that is now redundant. This is what stops the log
      growing without bound.
- [ ] **Recovery with checkpoints**: replay only from the last checkpoint
      forward.
- [ ] **Group commit**: batch several transactions into one `fsync` — throughput
      without weakening the durability rule.
- [ ] **Tests**: B-tree with thousands of keys (including splits at every
      level); a crash after a checkpoint; a crash *during* a checkpoint; range
      scans returning keys in order.

## Files

`src/pager.rs`, `src/page.rs`, `src/btree.rs`, `src/checkpoint.rs`,
`tests/btree.rs`, `tests/checkpoint_recovery.rs`, `docs/05-durability.md`.

## Definition of Done

- [ ] 100k keys insert, read back correctly, and survive a reopen.
- [ ] Range scan returns keys in sorted order.
- [ ] A crash immediately after a checkpoint recovers correctly and **does not**
      replay the truncated prefix.
- [ ] A crash *during* a checkpoint recovers to the previous consistent state —
      test it by writing a partial checkpoint record.
- [ ] The WAL does not grow unboundedly across many checkpoints (assert size).
- [ ] Build warning-free; all tests green, including milestone 0's.

## Notes

The nastiest bug here is a page written half-old/half-new (torn page). Real
databases handle it with full-page writes after a checkpoint — implement that,
or document explicitly why the design does not need it.

**Next:** [Milestone 2 — MVCC](milestone-2-mvcc.md).
