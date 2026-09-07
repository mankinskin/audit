# audit

Repository quality audit CLI and MCP server: computes metrics (file size,
cyclomatic complexity, coverage) and groups actionable findings by crate,
category, severity, metric, or path. Primary use case: run it against a
repository before or during a review pass to surface quality issues without
reading every file by hand.

The audit also checks the repository root for non-empty `README.md`,
`INSTALL.md`, and `CONTRIBUTING.md` files. The first version reports only
missing or whitespace-empty files; it does not parse document sections or run
the documented commands.

Command specifics, subcommands, and examples are documented in
[crates/audit-cli/README.md](crates/audit-cli/README.md).

## Quickstart

```bash
cargo run -p audit-cli --bin audit -- --help
audit run .
```
