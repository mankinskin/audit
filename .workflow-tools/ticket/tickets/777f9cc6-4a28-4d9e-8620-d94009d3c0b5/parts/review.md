# W1 review — markdown-links crate

Reviewed commit range (audit): d6f4c6e..b46a81b (`feat(markdown-links): add shared CommonMark link crate`, `fix(markdown-links): locate inline destination after link text`). workflow-tools: a824116 (member + patch), e7f3183 (gitlink bump).

## Code review
- Round 1 (code-review agent): confirmed bug — the inline destination opener could be picked from titles or code spans. Fixed in b46a81b: the first `](` after the end of the link-text children; no fallback. Tests added (title, `x.md "x.md"`, escaped code-span → None, nested image).
- Round 2: the fix was verified. Remaining note: HTML `id`/`name` attributes are matched case-sensitively. This is ported verbatim from audit (ML-03 requires audit's semantics unchanged), so it is not a W1 defect; deferred as feedback c35ea259.

## Gates
- G-ML-TEST `timeout 900 cargo test -p markdown-links`: 29 passed, 0 failed.
- G-AUDIT-BUILD `timeout 900 cargo build -p audit-api`: exit 0 (1 pre-existing dead_code warning).
- G-LINES-ML: no file over 500 lines (max anchors.rs 211).
- G-SUBMODULES: PASS.
