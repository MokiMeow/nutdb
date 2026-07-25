# 04 — Roadmap

From "a crash cannot lose your data" (today) to "a 3-node cluster survives its
leader being killed."

## The plan

| # | Milestone | You'll build | You'll learn |
|---|-----------|--------------|--------------|
| 0 | **Durability** ✅ | CRC'd WAL, crash recovery, KV store | write-ahead logging, torn writes, fsync ordering |
| 1 | **Storage engine** ✅ | pages, a B-tree, on-disk store, checkpointing | page layout, node splits, buffer management |
| 2 | **MVCC** ✅ | versioned rows, snapshot isolation, GC | transactions, watermarks, concurrency control |
| 3 | **SQL** | lexer → parser → planner → executor | query planning, expression evaluation |
| 4 | **Raft** | leader election, log replication | consensus, terms, log matching, commit index |
| 5 | **Cluster** | 3 nodes + fault injection | partitions, linearizability, real distributed failure |
| 6 | **Polish** | benchmarks, CI, `v1.0.0` | measurement and presentation |

## Dependency order

```
M0 ─► M1 ─► M2 ─► M3 ─► M6
        └──► M4 ─► M5 ──┘
```

Raft (M4) replicates the *log* from M0/M1, so it does not need SQL — M3 and M4
can proceed in either order once the storage engine exists. Everything needs M0,
because replicating data you can lose locally is pointless.

## Definition of Done (whole project)

A 3-node nutdb cluster:

- serves SQL `SELECT`/`INSERT`/`UPDATE`/`DELETE` with snapshot-isolated
  transactions,
- elects a new leader within seconds of the current one being killed,
- **loses no acknowledged write** under a fault-injection harness that kills
  leaders and partitions the network,
- and proves it with a reproducible test, not a claim.

## The headline artifact

An asciinema recording (and a README section) showing:

```
[node1] leader   ← writes flowing
[node2] follower
[node3] follower

$ kill -9 <node1>

[node2] election started… became leader (term 3)
[node3] follower
$ every acknowledged write is still readable ✓
```

**A demonstrated leader failover with zero data loss is the single most
convincing thing a database project can show.** Almost nobody builds one.

## Stretch goals (after v1.0.0)

- Range scans and secondary indexes.
- Serializable snapshot isolation (write-skew prevention).
- Log compaction / snapshots for Raft.
- A wire protocol so `psql` can connect.
- Read-only follower replicas with lease-based reads.
