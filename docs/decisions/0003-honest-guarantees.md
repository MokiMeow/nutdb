# ADR 0003 — State guarantees precisely, including the limits

**Status:** accepted · **Date:** 2026

## Context

Database projects routinely overclaim: "ACID" without defining the isolation
level, "durable" without acknowledging that consumer drives lie about `fsync`,
"consistent" as a marketing word. Readers cannot tell a real guarantee from a
hopeful one.

## Decision

Every guarantee is stated with its exact meaning **and its limits**, in the docs
and the README, and each is backed by a test that tries to violate it.

Concretely:

- Milestone 2 provides **snapshot isolation**, which permits **write skew**.
  The docs say so and give the example, rather than saying "ACID".
- Durability assumes the storage device honours `fsync`. Documented explicitly.
- "No data loss" claims are scoped to *acknowledged* writes, and a timeout is
  recorded as indeterminate rather than as a failure.
- Raft safety properties are asserted by tests over a lossy transport, not by
  narrative.

## Rationale

- A precise limitation is more convincing than a vague strength. An engineer
  reading "snapshot isolation, which permits write skew — here is the example"
  learns the author understands isolation levels; "fully ACID" tells them
  nothing, or worse.
- It keeps the project honest internally: writing down the limit forces the
  question of whether a test actually covers the claim.
- When a guarantee is later strengthened (SSI in the stretch goals), the change
  is legible because the earlier claim was precise.

## Consequences

- The README carries a "what nutdb guarantees and what it doesn't" section
  (milestone 6), which is treated as a feature, not an apology.
- Benchmarks state conditions; safety claims cite the test that proves them.
- Where the implementation is weaker than the ideal, the docs say so rather than
  omitting it.
