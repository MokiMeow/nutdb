# 02 — Architecture

Layered so each level depends only on the level beneath it, and so the bottom
level — durability — can never be bypassed by anything above.

```
┌────────────────────────────────────────────────┐
│ client / CLI                                    │
├────────────────────────────────────────────────┤
│ SQL: lexer → parser → planner → executor    M3  │
├────────────────────────────────────────────────┤
│ MVCC: transactions, snapshots, GC           M2  │
├────────────────────────────────────────────────┤
│ storage engine: pager, B-tree, checkpoints  M1  │
├────────────────────────────────────────────────┤
│ durability: WAL (len + crc32 + payload)  ✅ M0  │
└────────────────────────────────────────────────┘
                      ▲
                      │ the same log is replicated by
┌────────────────────────────────────────────────┐
│ Raft: election, replication, commit index   M4  │
└────────────────────────────────────────────────┘
```

**The log is the spine.** It starts as the crash-recovery journal (M0), becomes
the journal in front of a paged B-tree (M1), carries transaction boundaries
(M2), and finally becomes the **replication** log Raft ships between nodes (M4).
Getting its format and ordering right first is why milestone 0 came first.

## Current components

| File | Role |
|------|------|
| `src/crc.rs` | CRC-32 (IEEE), the torn-write detector |
| `src/wal.rs` | append-only checksummed log; `append` / `sync` / `replay` |
| `src/command.rs` | `Set`/`Delete` on-disk encoding, bounds-checked |
| `src/store.rs` | the store: log → fsync → apply; `Recovery` reporting |
| `src/main.rs` | demo + small CLI |
| `tests/crash_recovery.rs` | tests that manufacture crashes and corruption |

## Invariants

1. **Write ordering** — `append → fsync → mutate memory → acknowledge`. Never
   reordered, never skipped for speed.
2. **Replay stops at the first bad record** — after a torn write, later bytes
   cannot be assumed to be record boundaries.
3. **Recovery is reported, not silent** — `Recovery { records_replayed,
   truncated, valid_bytes }` so callers (and tests) can assert on what happened.
4. **Formats are explicit** — every on-disk layout is a documented byte diagram
   in the module that owns it.

## Error handling

Library code returns `io::Result`; format problems are
`ErrorKind::InvalidData` with a message naming what was expected. No panics
outside tests and `main`, because a database that aborts on malformed input is a
database that loses availability to a single bad byte.

## Concurrency (today and later)

Milestone 0 is single-threaded and synchronous — the simplest thing that can be
proven correct. Concurrency arrives with MVCC (M2), where readers take snapshots
and never block writers, and again with Raft (M4), where each node runs an
election timer, a replication loop, and an apply loop.

See the [roadmap](04-roadmap.md) and the [milestones](milestones/).
