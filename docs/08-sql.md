# 08 — SQL

## Pipeline

```
source → positioned tokens → AST → rule planner → pull executor → MVCC
```

The lexer uppercases identifiers and keywords while preserving text literals.
The recursive-descent parser gives `AND` tighter binding than `OR`. Every
frontend error carries a byte offset.

Catalog entries and rows are ordinary MVCC keys. Schemas record column names,
types, and one primary key. Rows use explicitly tagged, hex-encoded values; the
primary value also forms the row key.

The planner recognizes primary-key equality with a literal and emits
`IndexLookup`; other predicates use `Scan`. Filter precedes projection.
Scan/filter operators expose `next() -> Result<Option<Row>>`; sorting
materializes by necessity.

Without `BEGIN`, each statement is one transaction. An explicit transaction
persists across statements until `COMMIT` or `ROLLBACK`, including DDL.

Comparisons involving `NULL` return unknown. SQL `AND`/`OR` truth tables are
implemented, and `WHERE` retains only true. `IS NULL`, joins, aggregates, and
secondary indexes remain future work.
