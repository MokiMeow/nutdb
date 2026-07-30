# Contributing to nutdb

This is primarily a learning/portfolio project, but clean contributions are
welcome.

## Before you start

- Read [AGENTS.md](AGENTS.md): the operating manual, which applies to humans
  too.
- Skim [docs/00-overview.md](docs/00-overview.md), the
  [roadmap](docs/04-roadmap.md), and
  [docs/05-durability.md](docs/05-durability.md).
- You need only a Rust toolchain; there are no dependencies.

## The two rules that matter most

1. **Never bend the write ordering:** append to the log → `fsync` → mutate
   state → acknowledge. Not for a benchmark, not for convenience.
2. **Test by breaking it.** A safety property without a test that tries to
   violate it is a claim, not a property. Truncate logs, flip bits, kill
   mid-transaction, drop and reorder messages.

## Workflow

1. Pick the lowest-numbered unfinished milestone in
   [docs/milestones/](docs/milestones/), or an open issue.
2. Branch from `main`: `git checkout -b milestone-2-mvcc`.
3. `cargo build --release` must be warning-free and `cargo test` must pass at
   every commit.
4. Add failure-injecting tests for anything new.
5. Update the relevant doc and tick the Definition of Done.
6. Open a PR into `main`; CI must be green.

## Commit style

`type(scope): outcome` in the imperative, lower case. Types: `feat`, `fix`,
`docs`, `refactor`, `build`, `chore`, `test`, `perf`. No AI/co-author trailers.

Example: `fix(wal): treat a partial header as a torn write`.

## Code style

See §4 of [AGENTS.md](AGENTS.md). Rust 2021, `rustfmt` defaults, `Result` over
panics in library code, no `unwrap()` outside tests and `main`. Document
on-disk layouts as ASCII byte diagrams in the owning module.

## Reporting issues

Include what you did, what you expected, and what happened. For a recovery
problem, include the `Recovery` report (`records_replayed`, `truncated`,
`valid_bytes`) and, if you can, a `hexdump -C` of the log.
