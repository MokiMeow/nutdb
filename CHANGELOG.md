# Changelog

All notable changes to nutdb are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims
to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Milestone 1: checksummed 4 KiB slotted pages, an LRU pager, a handwritten
  multi-level B-tree, ordered range scans, durable page snapshots,
  checkpoint/WAL truncation, and batched group commit.
- Storage failure tests covering 100,000-key reopen, dirty eviction, page
  corruption, interrupted checkpoints, partial checkpoint records, bounded WAL
  growth, and recovery followed by new durable appends.
- Milestone 0: durability — a write-ahead log of `[len][crc32][payload]`
  records, CRC-32 (IEEE) checksums verified against standard vectors, a
  hand-written `Set`/`Delete` on-disk encoding with bounds and UTF-8 checking,
  and a key-value store that appends and `fsync`s **before** mutating memory.
- Crash recovery that replays the log, stops at the first torn or corrupt
  record, and reports `Recovery { records_replayed, truncated, valid_bytes }`.
- 10 crash-recovery tests that manufacture failures rather than assume they
  cannot happen: torn writes, single-bit payload corruption, truncated headers,
  missing and empty logs, repeated reopens, and large payloads.
- A demo (`cargo run -- demo`) that writes, simulates a crash, reopens from the
  log alone, and verifies every committed write survived.
- A small CLI (`set` / `get` / `list`).
- Documentation set under `docs/` (durability, MVCC, Raft, architecture,
  glossary), 3 ADRs, 7 milestone specs, and the `AGENTS.md` operating manual.

### Fixed

- Physically truncate a torn or corrupt WAL tail during recovery. Previously a
  later append could be acknowledged after the bad bytes but remain
  unreachable because every replay stopped at the same damaged record.

## [0.1.0] — milestone 0
- First working version: a crash-safe, checksummed durable key-value store.
