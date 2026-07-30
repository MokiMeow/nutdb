# Milestone 6: Polish (portfolio pass)

**Goal:** benchmarks, presentation, and `v1.0.0`.

## Tasks

### Proof
- [x] Full test suite green: crash recovery, B-tree, MVCC, SQL, Raft safety,
      fault injection, linearizability.
- [x] CI: build warning-free, run the whole suite, and run the fault-injection
      tests on the in-memory transport (deterministic and fast enough for CI).
- [x] A seeded, reproducible randomised test (a mini deterministic simulation):
      random operations + random faults under a fixed seed, so a failure can be
      replayed exactly.

### Benchmarks (measured, with conditions stated)
- [x] Single-node write throughput with and without group commit.
- [x] Read throughput under MVCC with a concurrent writer.
- [x] 3-node replicated write latency vs single-node: **replication costs
      something; publish the number rather than hiding it.**
- [x] Recovery time after a crash with a large log.

### Presentation
- [x] **The headline recording**: an asciinema of the 3-node cluster with the
      leader killed mid-workload, failover happening, and every acknowledged
      write still readable. Embed at the top of the README.
- [x] An architecture diagram: client → Raft leader → WAL → storage engine →
      followers.
- [x] A short "what nutdb guarantees, and what it doesn't" section: isolation
      level, durability assumptions (including that consumer drives can lie
      about fsync), and known limitations. **Honesty here reads as expertise.**

### Hygiene
- [x] All status tables accurate; every milestone DoD ticked.
- [x] `CHANGELOG.md` moved from Unreleased to `1.0.0`.
- [x] Tag `v1.0.0`.

## Definition of Done

- [x] CI green on `main` with the full suite.
- [x] README opens with the failover recording and the benchmark table.
- [x] The guarantees/limitations section is written and accurate.
- [x] `v1.0.0` tagged.

## Stretch goals (after v1.0.0)

- Serializable snapshot isolation (prevent write skew).
- Raft log compaction and snapshot transfer for new nodes.
- Secondary indexes and joins.
- A PostgreSQL wire-protocol front end so `psql` connects.
- Follower reads with leader leases.
