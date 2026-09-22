## Objective
Refine spec `39983ddf` around a versioned graded-budget cost policy.

## Context
Owner decision: “compute from a versioned policy table (graded budget). Resolves the contradiction with `.agents/instructions/orchestration/model-prices.instructions.md` in favour of grading, not outright refusal.”
Audit: `tmp/test-coverage-audit/03-requirements.md`.

## Acceptance criteria
- Define versioned table inputs and graded budget outcome.
- Reconcile the instruction wording with grading semantics.
- Define policy-version testability.

## Out of scope
- Cost-gate implementation.