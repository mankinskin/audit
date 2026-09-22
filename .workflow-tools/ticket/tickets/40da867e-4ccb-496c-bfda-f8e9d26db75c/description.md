## Objective
Refine spec `58a1d32c` with symlink confinement and race semantics.

## Context
Owner decision: “permit symlinks that resolve inside the root; reject escapes.”
Audit: `tmp/test-coverage-audit/03-requirements.md`.

## Acceptance criteria
- Define resolution-inside-root success and escape rejection.
- State per-platform TOCTOU/race behavior.
- Identify observable error behavior for rejected paths.

## Out of scope
- Implementing filesystem checks.