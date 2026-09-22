# Summary

Integrate store-owned acceptance evidence so specs can be satisfied or blocked by native documentation, validation, and log records rather than by wrapper-owned artifact payloads.

# Why

The existing workflow direction already assigns documentation evidence to `doc-api`, future validation executions to `test-api`, and future logs to `log-api`. The missing work is to wire those stores together with spec fulfillment instead of keeping traceability as markdown archaeology.

# Scope

- define native evidence identities and links for the stores that own the data
- keep `audit-api` as a derived aggregation surface
- make the evidence model sufficient for a migration pilot that proves the contract end to end

# Assumptions To Prove

- the first slice can extend `doc-api` and bootstrap minimal `test-api` / `log-api` entities without solving every workflow tool surface in one pass
- spec fulfillment can depend on native evidence links without reintroducing wrapper-owned truth
- audit rollups can be computed from store-owned metadata instead of standalone artifacts

# Acceptance Criteria

- The child tickets under this tracker define one coherent evidence model across the owning stores and the derived audit surface.
- Evidence ownership remains in the data-owning stores rather than moving into `audit-api` or a wrapper artifact path.
- The resulting evidence model is sufficient for the pilot migration ticket to validate one end-to-end slice.

# Validation

- `ticket.exe health <tracker-id> --workspace-root . --json`
- Child tickets provide the focused validation for store-owned evidence and derived audit reporting.