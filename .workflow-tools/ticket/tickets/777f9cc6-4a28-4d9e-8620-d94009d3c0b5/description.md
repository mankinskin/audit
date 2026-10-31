Create the shared `markdown-links` crate (spec `0c5727b1`, dossier `transcripts/31-10-2026_shared-markdown-links/`, waypoint W1).

Scope: `workflow-tools/audit/crates/markdown-links/` with `extract`, `LinkRef`, `anchors`/`Anchors`, `checkable_fragment`, `classify_destination`/`DestinationShape`, `resolve_local`, `encode_destination`; unit tests for ML-01..ML-05; `audit-api` re-export `pub use markdown_links as markdown;`; workflow-tools workspace member and `[patch."https://github.com/mankinskin/audit"]` entry. No consumer migration.
