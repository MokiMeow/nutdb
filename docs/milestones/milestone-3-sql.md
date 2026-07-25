# Milestone 3 — SQL ✅

**Goal:** execute a useful SQL subset over durable snapshot-isolated storage.

## What shipped

- [x] A position-aware lexer for keywords, identifiers, signed integers,
      escaped text literals, punctuation, and comparison operators.
- [x] A recursive-descent parser for `CREATE TABLE`, `INSERT`, `SELECT` with
      `WHERE`/`ORDER BY`/`LIMIT`, `UPDATE`, `DELETE`,
      `BEGIN`/`COMMIT`/`ROLLBACK`, and `EXPLAIN`.
- [x] Persisted table schemas with typed columns and exactly one primary key.
- [x] Integer, text, boolean, and `NULL` values with three-valued logic.
- [x] A rule-based physical planner that selects primary-key index lookup for
      equality and otherwise performs a scan.
- [x] Pull-based scan/filter operators; sort materializes by necessity.
- [x] Transactional DDL and DML over `MvccStore`.
- [x] A `cargo run -- sql "..."` CLI path.

## Definition of Done

- [x] `CREATE TABLE`, `INSERT`, persistent `SELECT`,
      `WHERE`/`ORDER BY`/`LIMIT`, `UPDATE`, and `DELETE` work end to end.
- [x] `BEGIN; ... ROLLBACK;` leaves no visible row; commit persists.
- [x] Comparisons against `NULL` produce unknown; filtering retains only true.
- [x] `EXPLAIN` reports `IndexLookup` for primary-key equality and `Scan` for a
      non-key predicate.
- [x] Duplicate primary keys, wrong types, columns, and schemas are rejected.
- [x] Syntax errors include the exact byte position.
- [x] Linux release build is warning-free and all earlier suites remain green.

See [the SQL design](../08-sql.md).

**Next:** [Milestone 4 — Raft](milestone-4-raft.md).
