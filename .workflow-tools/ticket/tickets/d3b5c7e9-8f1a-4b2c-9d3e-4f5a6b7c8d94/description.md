# [bootstrap][T4] Implement Validator Handoff with Separation-of-Duties

## Context

After a worker completes implementation (T3), the ticket moves to `validating` state. A second agent — the **validator** — is dispatched by the coordinator with a different identity to independently verify the acceptance criteria. The critical invariant is **separation of duties**: the same identity that implemented the ticket cannot also validate it.

This ticket covers the validator handoff protocol and the integration test that verifies the SoD enforcement.

## Separation-of-Duties Contract

- Validator identity (`sub` claim) must differ from the worker identity that implemented the ticket
- Coordinator checks identity before dispatching validator assignment packet
- Executor enforces SoD at claim time: same-identity validator claim is rejected with a structured error
- Reassignment is mandatory if SoD would be violated — previous assignment is closed as `superseded` (never overwritten)

## Validation Flow

```
T3 handoff complete — ticket in review/validating
  ↓
coordinator dispatches validator assignment packet
  (different sub from worker assignment)
  ↓
validator authenticates + verifies context (same T1/T2 protocol)
  ↓
validator claims ticket under validating state
  ↓
validator runs required checks per validation_plan
  ↓
validator attaches evidence refs
  ↓
validator emits: passed | failed with structured evidence
  ↓
if passed → ticket advances toward release-candidate
if failed → ticket returns to review with rejection details + linked bug ticket
```

## Validator Rejection Detail Format

```
ValidationResult {
    status:   passed | failed
    evidence: Vec<EvidenceRef>
    notes:    Option<String>
    rejection: Option<RejectionDetail>
}

RejectionDetail {
    reason:   String
    bug_id:   Option<TicketId>   // linked bug if product defect found
    severity: Option<BugSeverity>
}
```

## Scope

### What is being built / tested
- Validator assignment dispatch with a different identity than the worker
- Executor SoD enforcement at claim time
- Structured rejection of same-identity validator claim
- Validator claim, evidence attachment, and pass/fail result emission
- Reassignment on SoD violation (new assignment_id, prior closed as `superseded`)

### Not in scope
- Full validation plan execution (acceptance criteria checking logic)
- Release gating (T6)
- Production governance policy (separate design ticket)

## Acceptance Criteria

- [ ] Coordinator dispatches validator assignment with identity distinct from worker
- [ ] Validator successfully claims the ticket in `validating` state with its own assignment_id
- [ ] Validator attaches evidence and emits `passed` result
- [ ] Same-identity validator claim attempt is rejected with a structured SoD error
- [ ] Reassignment creates a new `assignment_id`; previous is closed as `superseded`
- [ ] All validator events carry the validator's `assignment_id` (distinct from worker's)
- [ ] Audit chain links both worker and validator assignment_ids to the ticket

## Dependencies

- Depends on: T3 (`c2a4b6d8`) — handoff state is the starting point
- Blocks: T6 (merge linkage requires both worker + validator assignment chain)