//! Options for [`super::evaluate_with_options`].

/// Inputs for [`super::evaluate_with_options`].
#[derive(Debug, Clone, Default)]
pub struct EvaluateOptions {
    /// Lift the default `transcripts` / `.workflow-tools` source exclusions.
    pub include_historical: bool,
    /// Extra repo-relative paths to exclude from the source walk.
    pub exclude_paths: Vec<String>,
    /// Opt-in (TD8): issue HTTP requests for `http(s)` destinations.
    /// Default `false`; no network access happens unless enabled.
    pub check_external: bool,
}
