## Goal
Make feedback-mcp, session-mcp, peek-mcp, rule-mcp, audit-mcp, and compact-terminal-mcp report their true package identities in MCP initialize responses so VS Code Copilot can distinguish and activate every configured server.

## Confirmed root cause
Each ServerHandler built ServerInfo with `..Default::default()`. rmcp's `Implementation::from_build_env()` is compiled inside the dependency crate, so every consuming server reported the name `rmcp`. Five visible stale processes therefore shared one initialize identity, while feedback-mcp was configured but had no running process and no Copilot launch attempt in the current logs. The identity collision prevented reliable distinction/discovery.

## Implementation
Each of the six ServerHandler implementations now explicitly sets `server_info.name` from `CARGO_PKG_NAME` and `server_info.version` from `CARGO_PKG_VERSION`.

## Validation
- Focused cargo check: passed for all six packages.
- Release build: passed after terminating five stale Windows-locked MCP processes.
- Strict initialize/tools-list handshake: six unique names, correct version 0.1.0, 56 tools total.
- feedback-mcp initialized as `feedback-mcp` and exposed all five feedback tools.
- Focused cargo test: 9 passed across 19 suites.
- Source diagnostics: no errors.
- Focused diff: 30 additions, only explicit server_info metadata.
- `cargo fmt --check` is blocked by unrelated pre-existing feedback-mcp formatting drift; no broad reformat was applied.
- Validation execution: `exec-vscode-copilot-mcp-identities-20260715`.
- Spec: `.spec/specs/1a62c7f7-4f94-420b-9532-3a28b0c1ecd5/spec.toml`.

## Activation
Run `Developer: Reload Window` in VS Code. The stale server processes were stopped and corrected release binaries are ready; Copilot must restart them and rebuild its MCP list.