//! Per-file scan: read and parse every source once, in parallel (TD9).

use std::fs;
use std::path::Path;
use std::sync::Arc;

use rayon::prelude::*;

use super::sources::SourceFile;
use crate::markdown::{Anchors, LinkRef, anchors, extract};

/// Result of the parallel read+parse phase for one source file.
pub(super) struct ParsedSource {
    /// `None` when the file could not be read as UTF-8.
    pub(super) links: Option<Vec<LinkRef>>,
    /// Own anchors, kept only when a fragment-only link needs them.
    pub(super) own_anchors: Option<Arc<Anchors>>,
}

/// Reads and parses every source; the result is index-aligned with `sources`.
pub(super) fn parse_sources(repo_root: &Path, sources: &[SourceFile]) -> Vec<ParsedSource> {
    sources
        .par_iter()
        .map(|source| {
            let Ok(contents) = fs::read_to_string(repo_root.join(&source.rel)) else {
                return ParsedSource {
                    links: None,
                    own_anchors: None,
                };
            };
            let links = extract(&contents);
            let needs_own = links.iter().any(|link| link.destination.starts_with('#'));
            ParsedSource {
                links: Some(links),
                own_anchors: needs_own.then(|| Arc::new(anchors(&contents))),
            }
        })
        .collect()
}
