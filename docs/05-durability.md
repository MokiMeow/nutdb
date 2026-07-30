# 05: Durability

*Milestone 0.* The property everything else depends on: **an acknowledged write
survives a crash, and no crash leaves the database in a state it cannot
recover from.**

## Write-ahead logging

The rule is an ordering rule:

```
1. append the mutation to the log
2. fsync            ← the write is now durable
3. apply it to memory
4. tell the caller it succeeded
```

Crash after step 2 → replay finds the record, the write is there.
Crash before step 2 → the caller was never told it succeeded, so losing it is
correct.
Crash *during* step 1 → a torn record, detected and discarded (below).

Getting this order backwards: updating memory first, logging later: is the
classic bug that makes a database silently lose acknowledged writes.

## Record format

```
┌──────────┬──────────┬─────────────────┐
│ len: u32 │ crc: u32 │ payload: len B  │   all integers little-endian
└──────────┴──────────┴─────────────────┘
```

The CRC covers the payload. It is the difference between *"the file ended"* and
*"the file lied"*: without it, a half-written record whose length field happens
to look plausible would be replayed as real data.

## Torn writes

A crash mid-append leaves a partial record. Replay handles three cases, all
tested:

| on disk | what replay does |
|---|---|
| fewer than 8 bytes of header | stop; mark truncated |
| header promises more payload than exists | stop; mark truncated |
| payload present but CRC mismatches | stop; mark truncated |

In every case **the valid prefix is kept** and everything from the bad record on
is discarded. `Recovery::valid_bytes` reports exactly where the good data ends,
and `Store::open` truncates the damaged tail there before accepting another
write. Otherwise a later record could be acknowledged behind bytes replay must
always stop at.

Stopping at the *first* bad record (rather than skipping it and continuing) is
deliberate: after a torn write, later bytes cannot be trusted to be record
boundaries at all.

## Why it is tested by breaking it

Crash-safety code that has only ever been run through clean shutdowns is
untested crash-safety code. The suite therefore *manufactures* the failures:

- appends a header promising 999 bytes that were never written,
- flips a single bit inside a committed record's payload,
- appends three stray bytes (a partial header),
- reopens the store 25 times in a row.

Each asserts that earlier committed data is intact **and** that the damaged
record did not become visible.

## fsync, honestly

`Wal::sync` calls `sync_all` (i.e. `fsync`). Two caveats worth knowing:

- **Consumer drives can lie** about having flushed their write cache. Real
  databases document this; so does this one.
- **`fsync` is slow** (~ms). Milestone 1 adds group commit: batching several
  transactions into one flush, which is how real systems get throughput
  without weakening the guarantee.

## Checkpoints

Milestone 1 writes a complete checksummed page snapshot to a temporary file,
syncs it, and publishes it before appending a checkpoint record and truncating
the WAL. Thus a crash during snapshot creation leaves the previous snapshot and
full WAL intact; a crash after publication can replay the still-present log
idempotently; and a completed checkpoint starts replay from an empty WAL.

## Where this goes next

- **M1 (complete)**: the log is now the bounded crash-recovery journal in front
  of a paged B-tree.
- **M2**: log records carry transaction ids so recovery can roll back
  uncommitted work.
- **M4**: the same log becomes the **Raft** replication log, which is why
  getting its format and ordering right first matters so much.

## References

- Gray & Reuter, *Transaction Processing*, ch. on logging and recovery
- ARIES (Mohan et al.): the canonical WAL recovery algorithm
- [docs/06: MVCC](06-mvcc.md), [docs/07: Raft](07-raft.md)
