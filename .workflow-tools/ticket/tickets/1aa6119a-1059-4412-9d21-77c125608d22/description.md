# Goal

Improve the `.audit/README.md` generated index so findings are listed per-file at severity-ordered entries with clickable file paths, instead of aggregated per-category summary blocks.

## Problem

Current output has only 8 entries (one summary + 7 category blocks). This gives no actionable file-level information — an agent cannot tell which specific files are triggering `file_length` or `static_complexity` findings without re-running the audit CLI. There are no links to source files.

## Scope

**Refined per Q2-Q3 decisions:**

- Add a new section `## Findings by Severity` below the existing category summary.
- Within each severity level (high → medium → low), list one entry per finding:
  `- [path/to/file.rs](path/to/file.rs) — category · message`
  where the link target is the workspace-relative file path.

- **Q3 Link Format**: The entire entry line is a clickable link. The category and message are part of the visible text after the link, not additional metadata. Format: `- [path](path) — category · message`.

- **Q2 Collapsing**: Keep all severity sections open by default (expand all). Omit `<details>`/`<summary>` wrapping for clarity and simplicity in this version.

- Order within each severity group: category first (compiler_check, compiler_warning, coverage, file_length, static_complexity, test_execution, ticket_graph), then by path.
- `ticket_graph` findings link to the relevant `ticket.toml` path rather than a source file.
- Keep the existing per-category summary section (`## Findings by Category`) unchanged.
- Update `render_readme` in `audit-api/src/store_index.rs`.
- Update integration tests.

## Acceptance criteria

- `.audit/README.md` has a `## Findings by Severity` section with per-finding lines.
- Each finding line is a markdown link to the relevant file or ticket.
- Findings are sorted within severity by category, then by path.
- All severity sections are open (expanded) by default.
- `audit store-index --check` returns `drift:false`.

## Non-goals

- No change to sidecar entry structure.
- No running the audit as part of store-index — findings come from whatever the most recent audit report contains.
