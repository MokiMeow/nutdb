# Milestone 0: Durability ✅ (done)

**Goal:** a key-value store whose committed writes survive a crash at any point,
including mid-append.

## Concepts

Write-ahead logging, fsync ordering, checksums, torn writes, and crash-recovery
testing.

## What shipped

- [x] `src/crc.rs`: CRC-32 (IEEE), verified against standard test vectors.
- [x] `src/wal.rs`: append-only log of `[len][crc32][payload]` records;
      `append`, `sync`, and `replay` that stops at the first torn or corrupt
      record and reports the valid prefix length.
- [x] `src/command.rs`: hand-written `Set`/`Delete` encoding with explicit
      bounds checking and UTF-8 validation.
- [x] `src/store.rs`: the store: **append → fsync → mutate memory**, plus a
      `Recovery` report (records replayed, truncated, valid bytes).
- [x] `src/main.rs`: a demo that writes, "crashes", reopens, and verifies.
- [x] `tests/crash_recovery.rs`: 10 tests that manufacture failures.

## Verified result

```
build: 0 warnings          cargo test: 10 crash-recovery tests + unit + doc tests, all pass

durability verified: every committed write survived, the delete stuck
  replayed 5 records (truncated: false)
  keys recovered: ["user:1", "user:2", "user:3"]
```

## Definition of Done

- [x] `cargo build --release`: zero warnings.
- [x] Committed writes survive a simulated crash (drop + reopen).
- [x] A **torn write** (header promising bytes that were never written) is
      discarded, earlier data survives, and `valid_bytes` equals the exact
      length of the good prefix.
- [x] A **single flipped bit** in a payload is caught by the CRC and that record
      is not applied.
- [x] A **partial header** is treated as a torn write, not a parse error.
- [x] A missing or empty log opens as an empty database.
- [x] Replay preserves write order (last write wins; deletes persist).
- [x] Empty and large (256 KiB) payloads round-trip.

## What was learned (worth keeping)

The checksum is not decoration. Length-prefixed records alone cannot distinguish
"the file ended cleanly" from "a crash left half a record whose length field is
garbage that happens to parse." The CRC is what makes recovery *decidable*,
which is why `corrupted_payload_fails_its_checksum` is arguably the most
important test in the repo.

## References

- Gray & Reuter, *Transaction Processing*: logging and recovery
- ARIES (Mohan et al.)
- [docs/05: Durability](../05-durability.md)

**Next:** [Milestone 1: Storage engine](milestone-1-storage.md).
