Updated handoff policy sources and regenerated prompts to require upfront shorthand/placeholder legends, explicit ticket legends, and unresolved-reference guards. Validated generated handoff and handoff-tickets outputs include the new requirements.

Review-close evidence for the follow-up:

- Rule-overlap audit checkpoint is usable in structured output when read from `metrics.rule_overlap`, not a top-level `rule_overlap` field.
- Fresh audit evidence: `status=collected`, `rules_considered=298`, `compared_pairs=44253`, `high_overlap_pairs=53`, `max_similarity=1.0`.
- Structured output also emitted concrete `rule_overlap` findings, so the earlier `null` result was an extraction mistake rather than a missing metric.
- Path-scoped review checkpoint is clean: `.agents/instructions/audit.instructions.md`, `.agents/prompts/handoff.prompt.md`, `.agents/prompts/handoff-tickets.prompt.md`, and their canonical rule sources showed no remaining unstaged changes during this review pass.
- Fresh `rule sync-targets --config rule-targets.yaml` regeneration completed with `status=ok`, and the reviewed policy surfaces remained drift-free afterward.
- No metric-absence follow-up ticket was needed. Remaining work, if taken up later, is content deduplication driven by the reported high-overlap pairs rather than audit-output ergonomics.