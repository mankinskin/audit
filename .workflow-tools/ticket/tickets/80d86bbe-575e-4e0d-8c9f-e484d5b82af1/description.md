# Production Distribution Batches

Execute only user-approved, deterministically classified production batches that distribute ticket, specification, and session records from the central meta-workspace stores to their smallest owning repositories.

Each batch must use the owning domain's journaled preflight, apply, resume, and rollback operations. Record source and target read-back, relation disposition, and terminal journal state. Retain and report every unclassified record. Do not copy entity directories, migrate schemas, or introduce database/framework work.

Dependencies: the distributed-entity-stores roadmap's readiness audit, session batch parity, non-production pilot, and explicit user production-release decision.