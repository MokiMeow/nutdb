# 07 — Raft

*Milestones 4–5.* Replicating the log so a cluster survives losing a node —
without ever losing an acknowledged write.

## The problem

One node means one disk and one power supply. Replication means several nodes
must agree on **the same log in the same order**, even while nodes crash and the
network drops, delays, duplicates, and reorders messages. Consensus is what
makes that agreement safe.

## Roles and terms

Every node is a **follower**, **candidate**, or **leader**. Time is divided into
**terms**, each with at most one leader.

```
follower ──(election timeout)──▶ candidate ──(majority votes)──▶ leader
   ▲                                 │                              │
   └────(sees a higher term)─────────┴──────────────────────────────┘
```

Randomised election timeouts (150–300 ms) prevent split votes from repeating.

## The safety rules that actually matter

An approximate Raft passes casual testing and loses data under a specific
partition. These are the rules that prevent it:

1. **Persist before responding.** `currentTerm`, `votedFor`, and the log must be
   on durable storage *before* replying to any RPC. This is where milestone 0's
   WAL earns its place.
2. **Election restriction.** Grant a vote only if the candidate's log is at
   least as up to date as yours (compare last log term, then last index). This
   is what guarantees a new leader has every committed entry.
3. **Log matching.** `AppendEntries` carries `prevLogIndex`/`prevLogTerm`; a
   follower rejects if they do not match, and the leader walks back until they
   do, then truncates the follower's conflicting suffix.
4. **Commit only current-term entries by counting.** A leader may only advance
   the commit index for an entry from **its own term**; earlier-term entries
   become committed indirectly. Skipping this rule is the subtle bug that lets a
   committed entry be overwritten.

## Client interaction

- Writes go to the leader; followers redirect.
- An entry is acknowledged only after a majority has replicated it.
- **A timeout is not a failure.** The write may have committed. Clients retry
  idempotently, and the fault-injection checker (M5) records such operations as
  indeterminate — treating them as failed produces phantom violations.

## Testing it properly

Real networks are not needed to find real bugs. Milestone 4 uses an **in-memory
transport that can drop, delay, and reorder messages**, so tests are
deterministic and fast, and asserts the safety properties directly:

- at most one leader per term (across many randomised runs),
- logs identical at every shared index after a partition heals,
- an acknowledged entry present in every subsequent leader's log.

Milestone 5 then does it for real: three processes, `kill -9` the leader
mid-workload, and check linearizability of the recorded history.

## References

- Ongaro & Ousterhout, *In Search of an Understandable Consensus Algorithm* —
  implement Figure 2 literally and cite it in the code.
- <https://raft.github.io/> — visualisations.
- Jepsen — the model for milestone 5's fault injection.
