# ADR 0002: Build durability before SQL

**Status:** accepted · **Date:** 2026

## Context

Most "build your own database" projects start with a SQL parser, because that is
the visible part. Durability and recovery get added later, if at all.

## Decision

Milestone 0 is the **write-ahead log and crash recovery**. SQL is milestone 3.

## Rationale

- A system that parses SQL beautifully and loses data on a crash is not a
  database. Durability is the property that earns the name.
- The log is the spine of everything after it: it becomes the recovery journal
  in front of the B-tree (M1), carries transaction boundaries (M2), and finally
  *is* the Raft replication log (M4). Designing its format and ordering
  correctly first avoids rewriting the foundation three times.
- Crash recovery is far easier to get right in isolation than retrofitted under
  a working query engine, where every layer already assumes it can mutate state
  freely.
- It is also the more differentiating skill: many people have written a parser;
  far fewer have written and *tested* a recovery path.

## Consequences

- The project has no SQL for its first several milestones, and the early demo is
  a key-value store. The README states this plainly rather than implying more.
- Every later milestone inherits the `log → fsync → apply → ack` invariant and
  must not weaken it (see [AGENTS.md](../../AGENTS.md) §2).
- Milestone 0's tests set the standard for the rest: *manufacture the failure*
  rather than assert the absence of one.
