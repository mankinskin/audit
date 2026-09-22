# Goal

Create a reusable rule-generated prompt in `.agents/prompts/reviews.prompt.md` that reviews the highest-ranked `in-review` tickets using the ticket system.

## Requested behavior

- Use ticket-system tools to find candidate tickets in `in-review`.
- Process tickets in rank order.
- Accept a limit per invocation; default to `5`.
- Allow an effectively unbounded mode via values such as `infinite`, `endlessly`, or `until no more candidates are found`.
- Return immediately when there are no `in-review` tickets.
- For each ticket:
  - audit the ticket for validity and clarity
  - read context and related tickets to understand the goal
  - verify the acceptance criteria are met
  - decide whether to move the ticket back to implementation or close it
- Use spec and audit tooling when researching each ticket, and read referenced code.
- Repeat until the ticket limit is reached.

## Implementation notes

- Add the prompt as a rule target in `rule-targets.yaml`.
- Create or reuse canonical rule entries for the prompt content.
- Generate the prompt file through the rule system and verify it with the rule target check flow.

## Acceptance criteria

1. `rule-targets.yaml` contains a reusable target for `.agents/prompts/reviews.prompt.md`.
2. The generated prompt file exists and is produced by the rule system.
3. The prompt explicitly uses ticket-system ranking to review `in-review` tickets with default limit `5`.
4. The prompt describes the endless/unbounded mode and the stop condition when no candidates remain.
5. The prompt instructs the agent to audit each ticket, gather context, verify acceptance criteria, and either return it to implementation or close it.
6. The prompt instructs the agent to use spec and audit tools plus referenced code while reviewing.
7. Rule generation and target check both succeed.
