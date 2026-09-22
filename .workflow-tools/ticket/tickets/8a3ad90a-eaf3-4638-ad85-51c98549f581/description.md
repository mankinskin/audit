# Goal
Add a safer, first-class workflow to remediate dangling ticket edges reported by `ticket health`.

# Problem
Dangling references currently require manual parsing and repeated `unlink` commands, which is error-prone.

# Scope
- Single-ticket cleanup helper.
- Bulk cleanup strategy derived from health findings.
- Dry-run preview and reason/audit-friendly output.
- Guidance for three causes: malformed IDs, deleted targets, and possible index/tool visibility faults.

# Acceptance Criteria
1. Operators can clean dangling references for one ticket using a first-class command without manual message parsing.
2. Operators can clean all dangling references using a deterministic strategy with dry-run preview.
3. Cleanup output clearly shows edge triples (`from`, `to`, `kind`) before mutation.
4. Docs/help text explain safe sequence: `scan --force` reconcile, verify, then mutate.
