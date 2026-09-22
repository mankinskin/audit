Review of ticket d1b3a6c9 found `cargo test -p audit-api` currently reports 13 passed / 1 failed, not the "14 passed, 0 failed" claimed in d1b3a6c9's description:

```
thread 'trials::rule_overlap::tests::reports_high_overlap_between_near_duplicate_rules' panicked at
memory-api\crates\audit-api\src\trials\rule_overlap.rs:236:36:
called `Result::unwrap()` on an `Err` value: DuplicateSlug("shared/prompts/handoff-a")
```

Reproduced deterministically (both multi-threaded and `--test-threads=1`). Confirmed via `git show a605fa6` that d1b3a6c9's diff does not touch `rule_overlap.rs` or any rule-api code, so this is not caused by that ticket's changes — but it does mean the validation evidence recorded on d1b3a6c9 is currently inaccurate and unreproducible.

Root cause guess: `store.create(&first, None)` fails as `DuplicateSlug` on the *first* insert into a fresh `tempdir()`-backed `RuleStore::init`, which suggests `RuleStore` may be reading/writing a shared/global rule root or registry that leaks across test runs on this machine, rather than being fully scoped to the test's tempdir.

Acceptance criteria:
- `cargo test -p audit-api` passes 14/14 (or updated count) with no DuplicateSlug failure, reproducible both single- and multi-threaded.
- Root cause identified and documented (test isolation bug in RuleStore, or environment-specific state).
- d1b3a6c9's validation evidence corrected to match the actual reproducible test result once fixed.