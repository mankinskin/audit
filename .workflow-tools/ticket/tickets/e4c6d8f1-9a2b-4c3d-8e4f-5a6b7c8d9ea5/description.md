# [bootstrap][T5] Handle Early-Stop Recovery and Reassignment

## Context

Agent sessions can terminate unexpectedly at any point during an assignment: stdio disconnect, heartbeat/liveness timeout, repeated auth failures, or explicit worker abort. The executor must handle all of these gracefully — invalidating the token, releasing the lease, emitting a structured incident event, and leaving the ticket in a state the coordinator can act on.

This ticket covers the early-stop detection, incident event protocol, and the integration test that exercises this failure path.

## Early-Stop Detection Triggers

| Trigger | Detection mechanism |
|---|---|
| stdio disconnect | EOF on stdin / broken pipe on stdout |
| Liveness timeout | No heartbeat within configurable TTL |
| Repeated auth failures | N consecutive `auth.*` deny events |
| Worker explicit abort | Worker sends abort signal with reason |

## Required Handling Steps (in order)

1. Mark session closed with `reason: <cause>`
2. Stop token refresh; invalidate active executor token
3. Release or expire lease according to policy (expired if liveness timeout; explicit release otherwise)
4. Emit structured incident event: `{ assignment_id, ticket_id, worker_id, reason, timestamp }`
5. Transition ticket to `blocked` or back to `review` with blocker metadata attached
6. Coordinator decides: requeue (same ticket, new assignment) vs reassignment (new worker)

## Incident Event Error Classes

```
session.disconnect
session.timeout
session.auth_failure
execution.branch_mismatch   (if caught post-claim)
execution.cwd_mismatch
execution.validation_failed
execution.unexpected_exit
```

## Reassignment Protocol

When requeuing after early-stop:
- New `assignment_id` is minted — previous assignment is closed as `aborted`
- Prior assignment record is preserved in audit with reason
- Token from aborted assignment is immediately invalid (cannot be refreshed)

## Scope

### What is being built / tested
- Simulated mid-assignment worker termination (test: kill the stdio connection)
- Token invalidation verification (subsequent use of the token is rejected)
- Lease release/expiry behavior under each termination trigger
- Incident event structure and completeness
- Ticket state after early-stop (blocked or review with blocker metadata)
- Re-dispatch of the ticket by coordinator under a new assignment_id

### Not in scope
- Validation handoff recovery (T4)
- Merge linkage (T6)

## Acceptance Criteria

- [ ] Simulated stdio disconnect triggers session close with `session.disconnect` reason
- [ ] Liveness timeout (configurable, short TTL for tests) triggers `session.timeout`
- [ ] Token is invalidated immediately on session close; subsequent use is rejected
- [ ] Lease is released/expired correctly for each termination type
- [ ] Structured incident event is emitted with: assignment_id, ticket_id, worker_id, reason, timestamp
- [ ] Ticket transitions to `blocked` (or `review`) with blocker metadata attached
- [ ] Coordinator can re-dispatch the ticket under a new `assignment_id`
- [ ] Aborted assignment record is preserved in audit; not overwritten

## Dependencies

- Depends on: T1 (`a8d6c1d2`), T2 (`b1f3e2a4`), T3 (`c2a4b6d8`) — needs the full session flow to interrupt
- Does not block T6 (T6 requires T4, not T5)