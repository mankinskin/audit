# W2 review — audit link-trial migration and split

Commits: audit 278a5d9 (migration onto markdown-links, split into trials/markdown_links/*), ac1e4ee (fix: restore old resolution semantics in targets.rs via decode/join/lexical-normalize/containment adapter; additive `decode_percent` export; 5 regression tests in tests/resolution.rs). workflow-tools gitlinks e8563e4, 64cd4ee.

Gates: markdown-links 29 tests pass; audit-api 64 tests pass; audit-cli builds; bench builds; inventory diff check empty (baseline 59, after 65 incl. new tests); all files <=500 lines.

Review round 1: two regressions in targets.rs (raw undecoded resolved_target evidence; resolve_local changed unsafe-path semantics) -> fixed in ac1e4ee.
Review round 2 (278a5d9^..ac1e4ee): no significant issues. Behaviour equals old trial except D5 and UTF-8 decode fix.
