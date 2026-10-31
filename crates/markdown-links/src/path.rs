//! Percent decoding/encoding and lexical path resolution.

/// Percent-decodes `value` on bytes and builds UTF-8 (`from_utf8_lossy` for
/// invalid sequences), so non-ASCII paths decode correctly.
pub fn decode_percent(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2]))
        {
            out.push((high << 4) | low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

/// Resolves `path` (percent-decoded) against the `/`-separated `base_dir`,
/// lexically collapsing `.` and `..`. Returns the `/`-joined result, or `None`
/// when it escapes above the root.
pub fn resolve_local(base_dir: &str, path: &str) -> Option<String> {
    let decoded = decode_percent(path);
    let mut parts: Vec<&str> = Vec::new();
    for segment in base_dir.split('/').chain(decoded.split('/')) {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// Percent-encodes space, `%`, `(`, `)`, `<` and `>`; everything else is kept.
pub fn encode_destination(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '%' => out.push_str("%25"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{encode_destination, decode_percent, resolve_local};

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(resolve_local("a", "b%20c.md").as_deref(), Some("a/b c.md"));
        assert_eq!(decode_percent("100%"), "100%");
        assert_eq!(decode_percent("%zz%4"), "%zz%4");
        assert_eq!(decode_percent("%41"), "A");
    }

    #[test]
    fn decodes_utf8_literal_and_encoded() {
        assert_eq!(resolve_local("", "café.md").as_deref(), Some("café.md"));
        assert_eq!(resolve_local("", "caf%C3%A9.md").as_deref(), Some("café.md"));
        assert_eq!(resolve_local("d", "caf%c3%a9.md").as_deref(), Some("d/café.md"));
    }

    #[test]
    fn resolves_dots_and_root_escape() {
        assert_eq!(resolve_local("a/b", "./c.md").as_deref(), Some("a/b/c.md"));
        assert_eq!(resolve_local("a/b", "../c.md").as_deref(), Some("a/c.md"));
        assert_eq!(resolve_local("a/b", "../../c.md").as_deref(), Some("c.md"));
        assert_eq!(resolve_local("a/b", "../../../c.md"), None);
        assert_eq!(resolve_local("", "../c.md"), None);
        assert_eq!(resolve_local("", "c.md").as_deref(), Some("c.md"));
    }

    #[test]
    fn encode_round_trips_through_resolve() {
        let path = "dir (x)/a b%c<d>é.md";
        let encoded = encode_destination(path);
        assert_eq!(encoded, "dir%20%28x%29/a%20b%25c%3Cd%3Eé.md");
        let expected = format!("base/{path}");
        assert_eq!(resolve_local("base", &encoded).as_deref(), Some(expected.as_str()));
        assert_eq!(encode_destination("plain/é.md"), "plain/é.md");
    }
}
