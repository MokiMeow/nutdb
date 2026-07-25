# Milestone 3 — SQL

**Goal:** speak SQL — lexer, parser, planner, executor — over the transactional
storage engine.

## Concepts

Lexing and recursive-descent parsing with operator precedence, logical vs
physical plans, the iterator (volcano) execution model, and expression
evaluation.

## Tasks

- [ ] **Lexer**: keywords, identifiers, string/number literals, operators,
      with source positions for error messages.
- [ ] **Parser** (recursive descent): `CREATE TABLE`, `INSERT`, `SELECT … FROM …
      WHERE … ORDER BY … LIMIT`, `UPDATE`, `DELETE`, `BEGIN/COMMIT/ROLLBACK`.
      Expression precedence via one function per level.
- [ ] **Catalog**: table definitions (name, columns, types, primary key)
      persisted through the storage engine like any other data.
- [ ] **Planner**: logical plan (Scan → Filter → Project → Sort → Limit), then a
      physical plan. A simple rule-based optimiser: push filters below
      projections, and use an index scan when the predicate hits the key.
- [ ] **Executor**: iterator model — every operator exposes `next() ->
      Option<Row>` and pulls from its child. Composable and memory-bounded.
- [ ] **Types**: integers, text, booleans, `NULL` with three-valued logic
      (`NULL = NULL` is `NULL`, not true — get this right).
- [ ] **Tests**: end-to-end SQL against expected result sets; parse errors
      report the right position; `EXPLAIN` output shows the plan.

## Files

`src/sql/lexer.rs`, `src/sql/parser.rs`, `src/sql/ast.rs`, `src/sql/plan.rs`,
`src/sql/executor.rs`, `src/catalog.rs`, `tests/sql.rs`, `docs/08-sql.md`.

## Definition of Done

- [ ] `CREATE TABLE`, `INSERT`, `SELECT` with `WHERE`/`ORDER BY`/`LIMIT`,
      `UPDATE`, and `DELETE` all work end to end and persist.
- [ ] Transactions work through SQL: `BEGIN; … ROLLBACK;` leaves no trace.
- [ ] `NULL` semantics are correct (three-valued logic) — tested explicitly.
- [ ] `EXPLAIN` prints the physical plan, and a filter on the primary key
      chooses an index scan rather than a full scan (proven by the plan output).
- [ ] Syntax errors report a useful message and position.
- [ ] Build warning-free; all earlier tests green.

## Notes

Keep the executor's iterator model strict — an operator that materialises its
whole input defeats the design and will not survive large tables. The volcano
model is also what makes adding operators (joins, aggregates) additive later.

**Next:** [Milestone 4 — Raft](milestone-4-raft.md).
