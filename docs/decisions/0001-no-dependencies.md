# ADR 0001: No dependencies

**Status:** accepted · **Date:** 2026

## Context

The obvious crates exist for every layer: `serde` for encoding, `crc32fast` for
checksums, `sled`/`rocksdb` for storage, `sqlparser` for SQL, `tokio` +
`openraft` for async consensus.

## Decision

Write everything with the Rust **standard library only**. Zero dependencies in
`Cargo.toml`.

## Rationale

- The dependencies would replace exactly the parts worth building. `openraft`
  *is* the milestone-4 learning; `sqlparser` *is* milestone 3.
- On-disk formats and the write path are the database's correctness contract.
  Hand-writing them forces the format to be understood and documented rather
  than derived from a macro.
- Zero dependencies means `cargo test` works offline, forever, with no version
  churn: valuable for a project meant to be read and re-run years later.
- It keeps the safety argument auditable: every byte written and every `fsync`
  is visible in this repo.

## Consequences

- More code, and the responsibility for correctness sits here, which is why the
  test suite manufactures failures rather than assuming they cannot happen.
- Async I/O is off the table for now; milestone 4's networking uses threads and
  blocking TCP, which is simpler to reason about for consensus anyway.
- Performance will trail tuned engines. That is acceptable and will be reported
  honestly with measured numbers.
