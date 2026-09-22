# Summary

Teach `audit-api` to report derived spec fulfillment status by reading store-owned expectation and evidence metadata.

# Why

`audit-api` should help operators see satisfied, blocked, and missed expectations across the repository, but it should do that by reading the real stores instead of becoming a second source of truth.

# Scope

- derive spec fulfillment rollups from native `spec-api`, `doc-api`, `test-api`, and `log-api` metadata
- report missing evidence, blocked acceptance clauses, and stale fulfillment states as actionable findings
- preserve `audit-api` as a read-model and aggregation layer

# Assumptions To Prove

- derived rollups can be computed without storing authoritative fulfillment state in audit-owned artifacts
- the first audit view can be useful with partial adoption as long as blocked or missing evidence is explicit
- current audit trial patterns are sufficient for adding spec-fulfillment reporting

# Acceptance Criteria

- `audit-api` reports derived satisfied, blocked, and missed expectation status from store-owned metadata.
- Findings identify the missing or blocking evidence with authoritative store identities.
- No new audit-owned artifact path becomes the source of truth for fulfillment.
- The resulting report is sufficient for the migration pilot to use as a review surface.

# Validation

- Focused `audit-api` tests for the new derived rollup behavior.
- A narrow `audit` CLI check demonstrating the new reporting surface on representative fixtures.