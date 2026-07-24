# Milestones

Each milestone leaves a database that **builds clean and passes every test**,
including the tests that try to break it.

| # | Milestone | State |
|---|-----------|-------|
| 0 | [Durability](milestone-0-durability.md) | ✅ done |
| 1 | [Storage engine](milestone-1-storage.md) | ⬜ |
| 2 | [MVCC](milestone-2-mvcc.md) | ⬜ |
| 3 | [SQL](milestone-3-sql.md) | ⬜ |
| 4 | [Raft](milestone-4-raft.md) | ⬜ |
| 5 | [Cluster + fault injection](milestone-5-cluster.md) | ⬜ |
| 6 | [Polish](milestone-6-polish.md) | ⬜ |

## Every milestone spec has

**Goal · Concepts · Tasks · Files · Definition of Done · References.**

## The loop (from AGENTS.md)

1. Pick the lowest-numbered unfinished milestone.
2. Implement its tasks.
3. **Write the test that tries to break it** — not just the happy path.
4. `cargo build --release` warning-free, `cargo test` green.
5. Update the concept doc, tick the DoD, update README/CHANGELOG/roadmap.
6. Commit (`type(scope): …`), keep CI green.

## The rule that matters most

**A safety property without a test that tries to violate it is a claim, not a
property.** Milestone 0 does not assert "we handle torn writes" — it writes a
torn record into the log and proves the committed data still comes back. Hold
every later milestone to the same bar, especially Raft.
