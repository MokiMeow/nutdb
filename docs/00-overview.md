# 00 — Overview

## What nutdb is

A distributed SQL database written from scratch in Rust, with no dependencies.
It is built in the order a database must actually be built: **durability first**,
then a storage engine, then transactions, then SQL, then replication.

## The one-sentence idea

> Never lose an acknowledged write — locally, then across a cluster.

## Design goals

1. **Durability is correctness.** The invariant `log → fsync → apply → ack` is
   never bent, including for benchmarks.
2. **Prove safety by attacking it.** Crash-safety code that has only seen clean
   shutdowns is untested. The suite manufactures torn writes and bit flips;
   later milestones kill leaders and partition networks.
3. **No dependencies.** WAL, checksums, B-tree, SQL parser, and Raft are all
   written here ([ADR 0001](decisions/0001-no-dependencies.md)).
4. **Honest guarantees.** The docs state the isolation level precisely and name
   the assumptions (including that consumer drives can lie about `fsync`).
   Overclaiming is how database projects mislead.

## What it is *not*

- Not a Postgres replacement — no wire protocol, joins, or secondary indexes in
  v1 (all stretch goals).
- Not serializable in v1: milestone 2 provides **snapshot isolation**, which
  does not prevent write skew. Said plainly, on purpose.
- Not tuned for throughput; correctness first, measured performance second.

## The stack

```
        SQL: lexer → parser → planner → executor        M3
                        │
        MVCC: versioned rows, snapshot isolation        M2
                        │
        storage engine: pages, B-tree, checkpoints   ✅ M1
                        │
        durability: WAL + CRC + crash recovery       ✅ M0
                        │
        Raft: leader election, log replication          M4
                        │
        cluster: 3 nodes + fault injection              M5
```

Milestones 0 and 1 are complete and tested: acknowledged writes survive torn
WAL records, the recovered tail is repaired before new appends, and 100,000
keys survive B-tree splits, checkpoint, and reopen. See
[durability](05-durability.md) and the
[storage milestone](milestones/milestone-1-storage.md).

Read the [architecture doc](02-architecture.md) next, or
[getting started](01-getting-started.md) to run it.
