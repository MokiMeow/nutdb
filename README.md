<h1 align="center">nutdb</h1>

<p align="center">
  <em>A distributed SQL database written from scratch in Rust — built from the
  durability layer up: write-ahead logging, MVCC, a SQL engine, and Raft
  replication. No database dependencies.</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/lang-Rust-orange" alt="Rust">
  <img src="https://img.shields.io/badge/deps-none-brightgreen" alt="no dependencies">
  <img src="https://img.shields.io/badge/focus-durability%20%2B%20consensus-blue" alt="durability">
  <img src="https://img.shields.io/badge/license-MIT-lightgrey" alt="MIT">
</p>

---

## What this is

Most "build a database" projects start with SQL parsing. nutdb starts where a
database earns the right to be called one: **not losing your data.** Milestone 0
is a checksummed write-ahead log and a store that survives a crash at any point,
including *mid-write*. Everything after — transactions, SQL, replication — is
built on that foundation.

```
set(k, v) ──▶ append to WAL ──▶ fsync ──▶ update memory ──▶ return
                                  ▲
                  a crash before this loses the write;
                  a crash after it does not; a crash *during*
                  it is detected by the checksum and discarded
```

## Milestone 0 result — durability, proven

```
$ cargo run --release -- demo
nutdb milestone 0 — write-ahead log durability

opening a fresh store at data/demo.wal
  set user:1 = ada
  set user:2 = grace
  set user:3 = alan
  set temp = scratch
  delete temp
  3 keys live, log is 126 bytes

process 'crashes' — reopening from the log only
  replayed 5 records (truncated: false)
  keys recovered: ["user:1", "user:2", "user:3"]

durability verified: every committed write survived, the delete stuck
```

**10 crash-recovery tests**, including the ones that actually matter:

| test | what it proves |
|---|---|
| `writes_survive_a_simulated_crash` | committed data returns after a process death |
| `torn_write_is_discarded_and_earlier_writes_survive` | a half-written record is dropped; the valid prefix is exact |
| `corrupted_payload_fails_its_checksum` | a single flipped bit is caught, not replayed as data |
| `truncated_header_is_handled` | a partial header is a torn write, not a parse crash |
| `last_write_wins_and_deletes_persist` | replay order is the write order |

## Why it is interesting (the depth on show)

- **Write-ahead logging done properly** — records are `[len][crc32][payload]`,
  the CRC is what separates "the file ended" from "the file lied", and replay
  stops at the first torn record.
  ([docs/05-durability.md](docs/05-durability.md))
- **Crash safety is tested, not claimed** — the suite *creates* torn writes and
  bit-flips instead of assuming they cannot happen.
- **MVCC and snapshot isolation** — transactions that read a consistent snapshot
  without blocking writers. ([docs/06-mvcc.md](docs/06-mvcc.md))
- **Raft consensus from scratch** — leader election, log replication, and the
  safety properties that make a 3-node cluster survive a leader dying.
  ([docs/07-raft.md](docs/07-raft.md))

## Quick start

```bash
cargo run --release -- demo     # the durability demonstration above
cargo test                      # 10 crash-recovery tests + unit tests
cargo run -- set user:1 ada     # persist a key
cargo run -- get user:1         # read it back
cargo run -- list               # everything stored
```

Needs only a Rust toolchain — there are no dependencies at all.

## Status

Milestone 0 (durable storage) is **done and tested**. The road to a replicated
SQL database is in [docs/04-roadmap.md](docs/04-roadmap.md).

| # | Milestone | State |
|---|-----------|-------|
| 0 | WAL + crash recovery | ✅ done |
| 1 | Pages, B-tree, and a real on-disk store | ⬜ |
| 2 | MVCC transactions + snapshot isolation | ⬜ |
| 3 | SQL: parser → planner → executor | ⬜ |
| 4 | Raft: leader election + log replication | ⬜ |
| 5 | 3-node cluster + Jepsen-style fault injection | ⬜ |
| 6 | Benchmarks, CI, `v1.0.0` | ⬜ |

The endgame: **a 3-node cluster that keeps serving correct, linearizable reads
while you kill the leader** — verified by a fault-injection harness, not by
assertion.

## Repository layout

```
nutdb/
├── src/          # crc, wal, command, store, cli
├── tests/        # crash-recovery and torn-write tests
├── docs/         # durability, MVCC, Raft, SQL, roadmap, milestones, ADRs
└── Cargo.toml    # zero dependencies
```

## License

MIT — see [LICENSE](LICENSE).
