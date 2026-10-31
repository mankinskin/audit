//! Markdown link coherence trial.
//!
//! Link syntax (extraction, anchors, destination shapes, path resolution)
//! comes from the shared `markdown-links` crate; this module owns the audit
//! rules and findings built on top of it.

mod anchors;
mod classify;
mod evaluate;
mod external;
mod findings;
mod model;
mod options;
mod repos;
mod scan;
mod sources;
mod targets;

#[cfg(test)]
mod tests;

pub use evaluate::{evaluate, evaluate_with_options};
pub use model::{LinkClass, MarkdownLinkResult};
pub use options::EvaluateOptions;
pub use sources::AUDIT_IGNORE_FILENAME;
