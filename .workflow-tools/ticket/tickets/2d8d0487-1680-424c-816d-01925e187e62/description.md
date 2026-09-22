# Goal
Plan and deliver a `session_audit` feature in session-api and expose it through audit-cli's unified audit interface for reviewing a specific session id or the latest persisted session.

## Scope
- Add session-level audit summarization in session-api.
- Add session schema version tagging in persisted session artifacts so format changes are explicit and reviewable.
- Expose a unified audit-cli entrypoint for `session_audit` with selectors for latest session and explicit session id.
- Validate behavior and document usage.

## Required workflow steps
1. Define/update owning spec and acceptance criteria.
2. Implement session-api data model and audit computation.
3. Integrate audit-cli command surface and output format.
4. Validate with focused tests and command-level checks.
5. Update docs and traceability links.

## Done when
- `session_audit` is callable through audit-cli unified interface for latest and explicit session id.
- persisted sessions include a schema version field controlled by session-api.
- tests for session-api and audit-cli pass for the new behavior.
- spec and ticket traceability is current.
