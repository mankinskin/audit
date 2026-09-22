# Goal
Triage `dead_code` compiler warnings in the `log-viewer` frontend (`tools/viewer/log-viewer`, root repo — split off from parent `9347c9f8` mechanical pass).

# Scope (host `cargo check --workspace` counts)
- ~8 `dead_code` warnings (`log-viewer-dioxus`):
  - `types.rs` (5)
  - `api.rs` (2)
  - `app.rs` (1)

# Approach
- Likely unused DTO fields / helper fns. Prefer `#[allow(dead_code)]` with rationale on intentional API-mirror types; delete clearly obsolete items.

# Validation
- `cargo check -p log-viewer-dioxus` clean of dead_code for touched files.

# Notes
- Lives in the root repo (not a submodule), unlike the other three dead_code children.
- Mechanical warnings already resolved by parent `9347c9f8` (8 fixed in log-viewer-dioxus bin).