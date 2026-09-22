# Problem

The default repo audit currently validates only a narrow slice of ticket dependency topology. The shipped audit spec covers orphan tickets, and this ticket started as a follow-up to surface raw dependency counts. That is not enough to explain the execution-order failures that matter for best-next ranking.

The ranking gap we need to close is not just "how many dependents does this ticket have?" but "is this prerequisite lagging behind work that already advanced past it?" Today that signal is missing from the default repo audit. Health surfaces also only emit a generic "ticket has unresolved dependencies" message, which does not distinguish a normal blocked ticket from a dependency-convergence problem where a more advanced dependent is waiting on an earlier-state prerequisite.

The clean layering is for `ticket-api` to own the dependency-convergence derivation and for `audit-api` to consume it. Audit should stay responsible for repo-level aggregation, finding severity, and remediation guidance, but it should not define a second graph-classification model.

## Requested improvement

Extend the default repo audit so it reports dependency-convergence findings, not just raw topology counts, and make those findings consume the same `ticket-api` dependency-convergence model defined by the graph-aware best-next redesign.

## Scope

- Preserve the existing orphan-ticket audit trial and dependency-count visibility.
- Add convergence-risk findings for cases where a non-terminal ticket depends on a prerequisite in a strictly earlier workflow state.
- Import the shared dependency-convergence derivation from `ticket-api` instead of recomputing graph traversal, state-gap logic, or reverse-impact metrics locally in `audit-api`.
- Reuse the same derived signals defined by the best-next redesign, including affected reverse-dependent reach and dependency-state gap terminology.
- Emit machine-readable evidence for the dependent ticket, blocking prerequisite, both states, and the derived convergence metrics needed for triage.
- Keep the default repo audit resilient when no local ticket store exists.

## Out of scope

- Changing the ranking order directly; ranking behavior is specified by the graph-aware best-next redesign ticket and spec.
- Replacing ticket health output; health surfaces may align with this model, but that is separate implementation work.