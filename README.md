# audit

Repository quality audit CLI and MCP server: computes metrics (file size,
cyclomatic complexity, coverage) and groups actionable findings by crate,
category, severity, metric, or path. Primary use case: run it against a
repository before or during a review pass to surface quality issues without
reading every file by hand.

Complexity finding IDs include the file path, function name, and starting
line and column. Distinct same-named methods or conditional definitions,
including those on the same line, remain separate persisted findings.
Unchanged source produces the same IDs across audit runs; moving a function
changes its location-based ID.

The audit also checks the repository root for non-empty `README.md`,
`INSTALL.md`, and `CONTRIBUTING.md` files. The first version reports only
missing or whitespace-empty files; it does not parse document sections or run
the documented commands.

Command specifics, subcommands, and examples are documented in
[crates/audit-cli/README.md](crates/audit-cli/README.md).

`audit links [REPO]` validates Markdown links across the repository and its
submodules (guidance-set rules, cross-repository rule, anchors, opt-in external
checks via `--check-external`, historical areas via `--include-historical`).
The full rules, exit codes and finding classes are in the
[Markdown link validation section](crates/audit-cli/README.md#markdown-link-validation-audit-links).

## Quickstart

```bash
cargo run -p audit-cli --bin audit -- --help
audit run .
```
