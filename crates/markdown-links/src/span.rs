//! Locating a link destination verbatim in the source text.
//!
//! All offsets are byte offsets into the parsed text.

use std::ops::Range;

use pulldown_cmark::LinkType;

/// Skips the separator before a destination, mirroring pulldown-cmark: spaces
/// and tabs, then at most one line ending (`\n` or `\r\n`); after a line
/// ending, also the continuation line's container prefix (spaces, tabs and
/// `>` block-quote markers).
pub(crate) fn skip_separator(text: &str, mut pos: usize) -> usize {
    let bytes = text.as_bytes();
    let skip_blanks = |mut at: usize, extra_gt: bool| {
        while at < bytes.len()
            && (matches!(bytes[at], b' ' | b'\t') || (extra_gt && bytes[at] == b'>'))
        {
            at += 1;
        }
        at
    };
    pos = skip_blanks(pos, false);
    let line_end = if bytes[pos..].starts_with(b"\r\n") {
        Some(2)
    } else if bytes.get(pos) == Some(&b'\n') {
        Some(1)
    } else {
        None
    };
    match line_end {
        Some(len) => skip_blanks(pos + len, true),
        None => pos,
    }
}

fn is_verbatim(text: &str, at: usize, dest: &str) -> bool {
    text.as_bytes()
        .get(at..at + dest.len())
        .is_some_and(|found| found == dest.as_bytes())
}

/// Span of an inline link or image destination within the event `range`.
fn inline_span(text: &str, range: &Range<usize>, dest: &str) -> Option<Range<usize>> {
    let source = &text[range.clone()];
    let candidates: Vec<usize> = source.match_indices("](").map(|(at, _)| at).collect();
    for at in candidates.into_iter().rev() {
        let start = skip_separator(&text[..range.end], range.start + at + 2);
        if text.as_bytes().get(start) == Some(&b'<') {
            return None;
        }
        let end = start + dest.len();
        if end <= range.end
            && is_verbatim(text, start, dest)
            && text[end..range.end]
                .chars()
                .next()
                .is_none_or(|c| c == ')' || c.is_whitespace())
        {
            return Some(start..end);
        }
    }
    None
}

/// Span of an autolink destination: the range without its `<` and `>`.
fn autolink_span(text: &str, range: &Range<usize>, dest: &str) -> Option<Range<usize>> {
    let source = &text[range.clone()];
    if source.len() >= 2 && source.starts_with('<') && source.ends_with('>') {
        let inner = range.start + 1..range.end - 1;
        if &text[inner.clone()] == dest {
            return Some(inner);
        }
    }
    None
}

/// Span of a reference definition's destination token inside `def_span`.
pub(crate) fn definition_span(
    text: &str,
    def_span: &Range<usize>,
    dest: &str,
) -> Option<Range<usize>> {
    let bytes = text.as_bytes();
    let end = def_span.end.min(text.len());
    let mut at = def_span.start + text[def_span.start..end].find('[')?;
    // Scan to the first `]` that is not escaped by a preceding backslash.
    at += 1;
    while at < end && bytes[at] != b']' {
        at += if bytes[at] == b'\\' { 2 } else { 1 };
    }
    if at >= end || bytes[at] != b']' || bytes.get(at + 1) != Some(&b':') {
        return None;
    }
    let start = skip_separator(&text[..end], at + 2);
    if start >= end || bytes[start] == b'<' {
        return None;
    }
    let token_len = text[start..end].find(char::is_whitespace).unwrap_or(end - start);
    (&text[start..start + token_len] == dest).then(|| start..start + token_len)
}

/// Locates `dest` for a link event of `link_type` whose source is `range`.
/// `definition` is the matching reference definition's span, if any.
pub(crate) fn locate(
    text: &str,
    link_type: LinkType,
    range: &Range<usize>,
    dest: &str,
    definition: Option<&Range<usize>>,
) -> Option<Range<usize>> {
    match link_type {
        LinkType::Inline => inline_span(text, range, dest),
        LinkType::Autolink | LinkType::Email => autolink_span(text, range, dest),
        LinkType::Reference | LinkType::Collapsed | LinkType::Shortcut => {
            definition_span(text, definition?, dest)
        }
        _ => None,
    }
}
