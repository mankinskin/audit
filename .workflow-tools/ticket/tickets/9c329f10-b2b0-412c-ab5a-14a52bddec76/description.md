# Goal
Triage `dead_code` compiler warnings in the `viewer-api` submodule (split off from parent `9347c9f8` mechanical pass). Largest concentration.

# Result
Resolved the full scoped warning set for `viewer-ctl` + `viewer-api-dioxus`.

## Strategy used
This slice is dominated by code that is live only in browser / wasm flows but is being audited under the default host target. Instead of deleting active frontend systems, I applied host-only allowances at module boundaries:
- `#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]` on frontend modules whose warnings are an artifact of host-only compilation:
  - `components/theme_settings/model.rs`
  - `components/theme_settings/preview.rs`
  - `effects/wgpu_overlay/element_types.rs`
  - `effects/wgpu_overlay/settings.rs`
  - `graph3d/mod.rs`
  - `graph3d/data.rs`
  - `graph3d/theme.rs`
  - `store/session.rs`
  - `store/theme.rs`
- Narrow helper allowances in `viewer-ctl/src/config.rs` for serde-default / lookup helpers that are intentionally available but not directly called in this build shape.

## Why this is the correct fix
A usage scan showed the warned items are not abandoned code; they are either:
- browser-only state, storage, and WebGPU/theme code compiled on non-wasm host targets,
- helpers used only from wasm paths or tests,
- persistent config helpers referenced through serde attributes.
Deleting them would remove active frontend capability rather than clean obsolete code.

# Validation
Commands run:
- `cargo check -p viewer-ctl -p viewer-api-dioxus --message-format=short | grep ...`
- `cargo build -p viewer-ctl -p viewer-api-dioxus`

Results:
- Scoped warning recount for the ticket file set is **0**.
- `viewer-ctl` and `viewer-api-dioxus` both build cleanly.
- No browser smoke was required here because the change is warning gating only; no runtime logic or rendering behavior changed.

# Acceptance
- Scoped dead_code findings resolved. ✓
- No broader build regression introduced. ✓
- Intentional frontend scaffolding preserved with target-appropriate allowances. ✓