# Summary

Bootstrap the first `log-api` entities for validation-log capture and retrieval linked from `test-api` executions.

# Why

Validation logs are part of review and debugging evidence, but this track only needs a minimal first slice. The log model should stay bounded and build directly on the new `test-api` execution identities.

# Scope

- add the first `log-api` crate and workspace wiring needed for native validation-log identities
- define minimal entities for validation-log capture and retrieval linked from `test-api` executions
- define the minimal link metadata needed to connect logs back to specs, tickets, docs, and test executions
- keep the slice focused on store-owned model and tests rather than full CLI/MCP/HTTP rollout

# Assumptions To Prove

- linking logs from `test-api` executions is sufficient for the first slice; logs do not need to become a general-purpose repository log system here
- the initial retrieval model can stay narrow and still support the pilot and audit rollups
- the new crate can be introduced without forcing every transport surface to land in the same ticket

# Acceptance Criteria

- A minimal `log-api` crate exists in the workspace.
- Native entities exist for validation-log capture and retrieval linked from `test-api` executions.
- The first slice exposes stable identifiers and link metadata for specs, tickets, docs, and test executions.
- The resulting model is sufficient for the pilot migration and downstream audit rollups to consume.

# Validation

- Focused model and linkage tests for the first `log-api` entities.
- A narrow workspace-level validation that the new crate integrates cleanly into the existing build.