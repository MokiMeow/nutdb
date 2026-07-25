# Milestone 1 — Storage engine ✅

**Goal:** move durable state out of an ever-growing replay log and into
checksummed fixed-size pages indexed by a handwritten B-tree.

## What shipped

- [x] A 4 KiB pager with a bounded LRU cache. Dirty pages are written before
      eviction and `flush` synchronizes the page file.
- [x] Checksummed slotted pages with a documented header, forward-growing slot
      array, and backward-growing cell area.
- [x] A handwritten multi-level B-tree with search, insertion and node
      splitting, deletion, and ordered half-open range scans.
- [x] Full page snapshots written to a temporary file, synchronized, and
      atomically published. A previous snapshot is retained during replacement
      where the platform requires it.
- [x] Checkpoint order: publish durable pages, append and sync a checkpoint
      record, then truncate and sync the redundant WAL.
- [x] Recovery loads the latest complete snapshot and replays only the
      post-checkpoint WAL.
- [x] Group commit through `Store::apply_batch` / `set_batch`: append the whole
      batch, perform one `fsync`, then mutate the B-tree.
- [x] A recovered torn WAL tail is physically truncated before new appends, so
      later committed records cannot become stranded behind corrupt bytes.

## On torn pages

The store never overwrites the live checkpoint in place. It writes a complete
checksummed replacement, syncs it, and only then publishes it. A crash during
the write leaves the previous snapshot and full WAL authoritative. This avoids
the half-old/half-new page problem without requiring full-page images in the
WAL.

Deletion currently rebuilds the in-memory tree from the remaining ordered
entries. It is correct but intentionally not optimized; inserting and reopening
large trees still exercise real multi-level splits and persisted internal
nodes.

## Definition of Done

- [x] 100,000 keys insert, read back, checkpoint, and survive reopen.
- [x] Range scans return keys in sorted order.
- [x] A checkpointed reopen replays zero records from the truncated prefix.
- [x] Partial page snapshots and partial checkpoint records recover to the
      previous consistent state.
- [x] Repeated checkpoints keep the WAL bounded.
- [x] Dirty eviction, page corruption, huge torn lengths, and appends after a
      repaired tail have regression tests.
- [x] Release build is warning-free on Linux and all earlier tests remain green.

**Next:** [Milestone 2 — MVCC](milestone-2-mvcc.md).
