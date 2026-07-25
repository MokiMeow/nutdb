# AGENTS.md — how this repo is built

The working agreement for this repository: anyone contributing to nutdb should
read it fully before making changes. If anything here conflicts with a note elsewhere, **this file
wins.**

---

## 1. How the work is organised

- **Planning** — plans milestones, defines Definitions of Done, reviews
  diffs, keeps the docs and safety claims honest.
- **Implementation** — proceed one milestone at a time against `docs/milestones/`,
  keeping the build clean and every test green.

The loop: **pick the lowest-numbered unfinished milestone → implement →
build clean → prove it with tests that try to break it → tick the Definition of
Done → update docs/CHANGELOG → commit → next.**

## 2. Ground rules (non-negotiable)

1. **Durability is a correctness property, not a feature.** Any change to the
   write path must keep this invariant: *a committed write is on durable storage
   before the caller is told it succeeded, and any crash leaves the database
   self-consistent.* Order is always: append to log → `fsync` → mutate memory.
2. **Test the failure, not the happy path.** Crash-safety tests must *create*
   the failure — truncate the log, flip a bit, kill mid-append. A test that only
   does clean shutdowns proves nothing about crash safety.
3. **No dependencies.** `std` only. The WAL, checksums, B-tree, SQL parser, and
   Raft implementation are the project ([ADR 0001](docs/decisions/0001-no-dependencies.md)).
4. **The build must never break.** `cargo build --release` and `cargo test` are
   green at every commit, with zero warnings.
5. **On-disk formats are a contract.** Any change to a record layout must be
   documented in the module's doc comment *and* the relevant `docs/` page, and
   must consider what happens to an existing log written by the old code.
6. **No unsafe code** without an explicit justification comment and a test.
7. **No feature without a doc.** Update the matching `docs/` page in the same
   milestone.

## 3. Build, run, verify

| Command | What it does |
|---------|--------------|
| `cargo build --release` | Build (zero warnings expected). |
| `cargo test` | Unit + crash-recovery tests. |
| `cargo run --release -- demo` | The durability demonstration. |
| `cargo run -- set/get/list` | The small CLI. |

**Definition of "it works":** builds warning-free, `cargo test` passes, and any
new failure mode has a test that reproduces it.

## 4. Coding standards

- Rust 2021, `rustfmt` defaults. Idiomatic: `Result` over panics in library
  code, `?` for propagation, no `unwrap()` outside tests and `main`.
- Module-level `//!` doc comments explain *why the design is what it is* — the
  WAL and store modules are the model to follow.
- Document on-disk layouts as ASCII diagrams in the module doc.
- Errors: `std::io::Error` with `ErrorKind::InvalidData` for format problems,
  carrying a message that says what was expected.
- Comment *why* (which invariant, which failure mode), not *what*.

## 5. Commit and branch style

- `type(scope): outcome`, imperative, lower case.
  Examples: `feat(mvcc): add snapshot reads with a watermark`,
  `fix(wal): treat a partial header as a torn write`.
- Types: `feat`, `fix`, `docs`, `refactor`, `build`, `chore`, `test`, `perf`.
- **No AI/co-author trailers.**
- Branch per milestone (`milestone-2-mvcc`), PR into `main`, CI green.

## 6. The milestone path

Specs with Definitions of Done live in `docs/milestones/`.

| # | Milestone | Adds | Spec |
|---|-----------|------|------|
| 0 | Durability | CRC'd WAL, crash recovery, KV store | [spec](docs/milestones/milestone-0-durability.md) ✅ |
| 1 | Storage engine | pages, B-tree, on-disk store, checkpointing | [spec](docs/milestones/milestone-1-storage.md) ✅ |
| 2 | MVCC | versioned rows, snapshot isolation, GC | [spec](docs/milestones/milestone-2-mvcc.md) ✅ |
| 3 | SQL | lexer, parser, planner, executor | [spec](docs/milestones/milestone-3-sql.md) |
| 4 | Raft | leader election, log replication | [spec](docs/milestones/milestone-4-raft.md) |
| 5 | Cluster | 3 nodes, fault injection, linearizability | [spec](docs/milestones/milestone-5-cluster.md) |
| 6 | Polish | benchmarks, CI, `v1.0.0` | [spec](docs/milestones/milestone-6-polish.md) |

**Definition of Done (whole project):** a 3-node nutdb cluster serves SQL with
snapshot-isolated transactions, elects a new leader when one is killed, loses no
committed data under fault injection, and demonstrates it with a reproducible
harness.

## 7. What NOT to do

- Do not weaken the fsync-before-acknowledge rule for a benchmark number.
- Do not add a dependency (no `serde`, no `tokio`, no `sqlparser`).
- Do not claim a safety property without a test that tries to violate it.
- Do not change an on-disk format without considering existing logs.
- Do not implement Raft "roughly" — the safety rules (term checks, log matching,
  commit index) are exactly where subtle data loss hides.

## 8. Tools reference

- **cargo** — build, test, bench.
- **`cargo test -- --nocapture`** — see test output while debugging.
- **`hexdump -C data/*.wal`** — inspect the on-disk format directly; the surest
  way to check a record layout.
- **The Raft paper** (Ongaro & Ousterhout, *In Search of an Understandable
  Consensus Algorithm*) — the authority for milestone 4.
- **Jepsen's writeups** — the model for milestone 5's fault injection.

Build one milestone, try to break it, document it, commit. Then the next.
