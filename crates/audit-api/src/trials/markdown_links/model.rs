//! Link classification and trial result types.

use serde::{Deserialize, Serialize};

use crate::models::{AuditFinding, MarkdownLinkMetric};

/// Stable, machine-readable classification for a Markdown link.
///
/// `category()` is the finding `category`/id prefix; `is_blocking()` decides
/// whether the class must fail `audit links` and the pre-commit hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkClass {
    /// Local target exists and is part of the portable guidance corpus.
    GuidanceTarget,
    /// Local target does not exist.
    MissingTarget,
    /// Local target exists but is outside the portable guidance corpus
    /// (ticket/spec store, source code, generated artifact, unrelated file),
    /// and the link source is a guidance file.
    NonGuidanceTarget,
    /// Local target lives in an upward or sibling repository (a repository
    /// that is not the source repository or one of its descendant
    /// submodules).
    CrossRepository,
    /// http(s), mailto:, or data: destination; not resolved for existence.
    External,
    /// Pure `#fragment` destination with no path component.
    FragmentOnly,
    /// Destination uses a scheme/shape this audit does not support (e.g. a
    /// non-http(s) scheme, an absolute filesystem path, or a malformed
    /// destination).
    UnsupportedDependency,
    /// Local destination resolves outside the repository root entirely.
    UnsafePath,
    /// The Markdown source file could not be read as UTF-8 Markdown.
    UnreadableArtifact,
    /// Target is inside a descendant submodule but absent at the commit the
    /// parent records for that submodule.
    SubmoduleRevisionMissingTarget,
    /// The recorded submodule commit (or the submodule itself) is not
    /// available locally, so the target cannot be checked.
    SubmoduleRevisionUnavailable,
    /// Target is inside a nested repository that has no gitlink entry at the
    /// active revision (TD14).
    SubmoduleGitlinkMissing,
    /// `#fragment` matches no heading slug or HTML `id`/`name` in the target
    /// Markdown document (TD7).
    MissingAnchor,
    /// Opt-in external check: HTTP status >= 400 (except 429) or transport
    /// error (TD8).
    ExternalUnreachable,
    /// Opt-in external check: HTTP 429; reported but not blocking.
    ExternalRateLimited,
}

impl LinkClass {
    pub fn category(self) -> &'static str {
        match self {
            LinkClass::GuidanceTarget => "markdown_link_guidance_target",
            LinkClass::MissingTarget => "markdown_link_missing_target",
            LinkClass::NonGuidanceTarget => "markdown_link_non_guidance_target",
            LinkClass::CrossRepository => "markdown_link_cross_repository",
            LinkClass::External => "markdown_link_external",
            LinkClass::FragmentOnly => "markdown_link_fragment_only",
            LinkClass::UnsupportedDependency => "markdown_link_unsupported_dependency",
            LinkClass::UnsafePath => "markdown_link_unsafe_path",
            LinkClass::UnreadableArtifact => "markdown_link_unreadable_artifact",
            LinkClass::SubmoduleRevisionMissingTarget => "markdown_link_submodule_missing_target",
            LinkClass::SubmoduleRevisionUnavailable => {
                "markdown_link_submodule_revision_unavailable"
            }
            LinkClass::SubmoduleGitlinkMissing => "markdown_link_submodule_gitlink_missing",
            LinkClass::MissingAnchor => "markdown_link_missing_anchor",
            LinkClass::ExternalUnreachable => "markdown_link_external_unreachable",
            LinkClass::ExternalRateLimited => "markdown_link_external_rate_limited",
        }
    }

    /// Blocking classes fail `audit links` and the pre-commit hook.
    pub fn is_blocking(self) -> bool {
        matches!(
            self,
            LinkClass::MissingTarget
                | LinkClass::NonGuidanceTarget
                | LinkClass::CrossRepository
                | LinkClass::UnsupportedDependency
                | LinkClass::UnsafePath
                | LinkClass::UnreadableArtifact
                | LinkClass::SubmoduleRevisionMissingTarget
                | LinkClass::SubmoduleRevisionUnavailable
                | LinkClass::SubmoduleGitlinkMissing
                | LinkClass::MissingAnchor
                | LinkClass::ExternalUnreachable
        )
    }
}

pub struct MarkdownLinkResult {
    pub metric: MarkdownLinkMetric,
    pub findings: Vec<AuditFinding>,
}
