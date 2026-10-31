//! Maps the shared `DestinationShape` onto the audit's link handling.

use crate::markdown::{DestinationShape, classify_destination};

/// How the audit treats one destination before any filesystem access.
pub(super) enum Disposition {
    /// `mailto:` / `data:`; counted as skipped, never resolved.
    Skip,
    /// `http(s)`; skipped unless the opt-in external check is enabled.
    Web,
    /// Pure `#fragment`; checked against the source's own anchors.
    FragmentOnly,
    /// Unsupported scheme/shape or absolute path (D5): the
    /// `unsupported_dependency` class.
    Unsupported,
    /// Relative path (before `#`) to resolve and check.
    Local(String),
}

pub(super) fn disposition(destination: &str) -> Disposition {
    match classify_destination(destination) {
        DestinationShape::OtherExternal => Disposition::Skip,
        DestinationShape::Web => Disposition::Web,
        DestinationShape::FragmentOnly => Disposition::FragmentOnly,
        DestinationShape::Unsupported | DestinationShape::Absolute(_) => Disposition::Unsupported,
        DestinationShape::Local(path) => Disposition::Local(path),
    }
}
