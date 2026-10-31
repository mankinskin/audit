//! Destination shape classification.

/// Shape of a raw link destination, before any filesystem resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestinationShape {
    /// Empty path (`#fragment` or empty destination).
    FragmentOnly,
    /// `mailto:` or `data:`; never resolved.
    OtherExternal,
    /// `http://` or `https://`.
    Web,
    /// `//host`, other `scheme://`, or contains `<` / `>`.
    Unsupported,
    /// Leading `/`, leading `\`, or drive letter (`X:`); carries the path part.
    Absolute(String),
    /// Everything else; carries the path part before `#`.
    Local(String),
}

/// Classifies `destination` by its path part (before `#`), checking rules in
/// the documented order.
pub fn classify_destination(destination: &str) -> DestinationShape {
    let path = destination.split('#').next().unwrap_or(destination);
    if path.is_empty() {
        return DestinationShape::FragmentOnly;
    }
    if path.starts_with("mailto:") || path.starts_with("data:") {
        return DestinationShape::OtherExternal;
    }
    if path.starts_with("http://") || path.starts_with("https://") {
        return DestinationShape::Web;
    }
    if path.starts_with("//") || path.contains("://") || path.contains('<') || path.contains('>') {
        return DestinationShape::Unsupported;
    }
    let bytes = path.as_bytes();
    let drive_letter = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if path.starts_with('/') || path.starts_with('\\') || drive_letter {
        return DestinationShape::Absolute(path.to_string());
    }
    DestinationShape::Local(path.to_string())
}

#[cfg(test)]
mod tests {
    use super::{DestinationShape as S, classify_destination as classify};

    #[test]
    fn every_shape_row() {
        assert_eq!(classify(""), S::FragmentOnly);
        assert_eq!(classify("#frag"), S::FragmentOnly);
        assert_eq!(classify("mailto:a@b.c"), S::OtherExternal);
        assert_eq!(classify("data:text/plain,x"), S::OtherExternal);
        assert_eq!(classify("http://a.b/c#d"), S::Web);
        assert_eq!(classify("https://a.b"), S::Web);
        assert_eq!(classify("//host/x"), S::Unsupported);
        assert_eq!(classify("ftp://x"), S::Unsupported);
        assert_eq!(classify("a<b"), S::Unsupported);
        assert_eq!(classify("a>b.md"), S::Unsupported);
        assert_eq!(classify("/abs/x.md#f"), S::Absolute("/abs/x.md".into()));
        assert_eq!(classify("\\a"), S::Absolute("\\a".into()));
        assert_eq!(classify("C:/x"), S::Absolute("C:/x".into()));
        assert_eq!(classify("x.md#f"), S::Local("x.md".into()));
        assert_eq!(classify("../a/b.md"), S::Local("../a/b.md".into()));
    }

    #[test]
    fn checks_run_in_table_order() {
        assert_eq!(classify("//C:/x"), S::Unsupported);
        assert_eq!(classify("/a<b"), S::Unsupported);
        assert_eq!(classify("a:b.md"), S::Absolute("a:b.md".into()));
    }
}
