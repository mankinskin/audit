## Goal
Add a command-line surface for the repository QA tool so it can be used outside MCP.

## Scope
- Add a CLI binary in the existing repo-qa crate
- Reuse the same audit engine as the MCP endpoint
- Support configurable thresholds and repo root selection
- Emit JSON and human-readable output
- Validate the CLI with focused tests
