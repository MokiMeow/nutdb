# 10 — Glossary

- **ACID** — atomicity, consistency, isolation, durability.
- **B-tree** — the balanced on-disk index structure (milestone 1).
- **Checkpoint** — flushing dirty pages and recording that the log prefix before
  it is no longer needed for recovery.
- **Commit index** — in Raft, the highest log index known to be committed.
- **CRC-32** — the checksum that distinguishes "the file ended" from "the file
  lied" — how torn writes are detected.
- **fsync** — force buffered writes to durable storage. Slow, mandatory before
  acknowledging a write, and unreliable on consumer drives that lie.
- **Group commit** — batching several transactions into one `fsync`.
- **Linearizability** — every operation appears to take effect atomically at a
  point between its invocation and response, respecting real time.
- **Log matching** — Raft's property that logs agreeing at an index agree on all
  preceding entries.
- **MVCC** — multi-version concurrency control: writes create new versions so
  readers never block.
- **Page** — the fixed-size unit (4 KiB) of on-disk storage.
- **Raft** — the consensus algorithm used for replication.
- **Replay** — rebuilding state by re-applying the log after a crash.
- **Serializability** — the strongest isolation: results equal *some* serial
  order. Stronger than snapshot isolation.
- **Slotted page** — page layout with a slot array from the front and cell data
  from the back.
- **Snapshot isolation** — each transaction reads a consistent snapshot; permits
  **write skew**, so it is *not* serializable.
- **Term** — Raft's logical clock; at most one leader per term.
- **Tombstone** — a marker recording that a key was deleted.
- **Torn write** — a record left partially written by a crash mid-append.
- **Transaction id (txn)** — monotonically increasing id identifying a
  transaction and stamping the versions it creates.
- **WAL (write-ahead log)** — the append-only log written and `fsync`ed before
  any change is applied to in-memory or on-disk state.
- **Watermark** — the oldest snapshot any live transaction is using; the bound
  on what garbage collection may reclaim.
- **Write skew** — the anomaly snapshot isolation permits: two transactions read
  overlapping data, write disjoint keys, and both commit into a state no serial
  order allows.
