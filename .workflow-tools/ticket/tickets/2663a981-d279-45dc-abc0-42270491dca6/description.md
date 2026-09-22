# Goal
Expose session audit through audit-cli unified audit interface.

## Acceptance criteria
- audit-cli accepts a session selector for latest and explicit session id.
- command output remains compatible with existing audit output conventions while adding session-audit payload.
- text and JSON output include clear session identity and schema version visibility.
- focused CLI tests cover both selector paths.
