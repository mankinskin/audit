# Goal
Implement `session_audit` in session-api and tag persisted session records with a schema version.

## Acceptance criteria
- session-api exposes a stable `session_audit` entrypoint that returns audit data for one session.
- session selection supports explicit id and latest-session resolution.
- session persistence includes a schema version marker and reads remain backward-compatible or fail with explicit diagnostics.
- focused session-api tests cover schema version behavior and audit calculations.
