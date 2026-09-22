# Problem

The repository generates custom agents for research, testing, interview, and audit, but it does not provide a dedicated implement agent that is optimized for surgical execution once the scope is clear.

## Scope

- add a generated `.agents/agents/implement.agent.md` surface
- add the matching target in `rule-targets/45-agents-agents.yaml`
- create the canonical `.rule` entry that owns the implement agent content
- validate the new surface with focused `rule.exe explain-target`, `rule.exe generate-target`, and `rule.exe generate-target --check` commands

## Non-goals

- changing existing generated agents beyond shared consistency
- broad workflow or tooling refactors
