//! Heading slugs, HTML anchors and fragment helpers.

use std::collections::{HashMap, HashSet};

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::path::percent_decode;

/// Link anchors of one Markdown document.
#[derive(Debug, Default, Clone)]
pub struct Anchors {
    /// GitHub-style heading slugs, with duplicate suffixes applied.
    pub slugs: HashSet<String>,
    /// Explicit HTML `id=` / `name=` attribute values.
    pub ids: HashSet<String>,
}

impl Anchors {
    /// Whether `fragment` names an anchor (slugs are matched case-tolerantly).
    pub fn matches(&self, fragment: &str) -> bool {
        self.ids.contains(fragment)
            || self.slugs.contains(fragment)
            || self.slugs.contains(&fragment.to_lowercase())
    }
}

/// GitHub-style heading slug: lowercase, keep alphanumerics, `-`, `_` and
/// spaces, then spaces become `-`.
fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// Collects `id="…"` / `name="…"` attribute values from a raw HTML chunk.
fn collect_html_ids(html: &str, ids: &mut HashSet<String>) {
    let bytes = html.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let rest = &html[index..];
        let name_len = if rest.starts_with("id") {
            2
        } else if rest.starts_with("name") {
            4
        } else {
            index += 1;
            continue;
        };
        let preceded_by_space = index > 0 && bytes[index - 1].is_ascii_whitespace();
        let mut cursor = index + name_len;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if !preceded_by_space || cursor >= bytes.len() || bytes[cursor] != b'=' {
            index += name_len;
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let value = match bytes.get(cursor) {
            Some(&quote @ (b'"' | b'\'')) => {
                let start = cursor + 1;
                let len = html[start..]
                    .find(quote as char)
                    .unwrap_or(html.len() - start);
                cursor = start + len;
                &html[start..start + len]
            }
            Some(_) => {
                let start = cursor;
                while cursor < bytes.len()
                    && !bytes[cursor].is_ascii_whitespace()
                    && bytes[cursor] != b'>'
                {
                    cursor += 1;
                }
                &html[start..cursor]
            }
            None => "",
        };
        if !value.is_empty() {
            ids.insert(value.to_string());
        }
        index = cursor.max(index + name_len);
    }
}

/// Collects the heading slugs (with `-1`, `-2` duplicate suffixes) and HTML
/// `id`/`name` values of `text`.
pub fn anchors(text: &str) -> Anchors {
    let mut anchors = Anchors::default();
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut heading: Option<String> = None;
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Heading { .. }) => heading = Some(String::new()),
            Event::End(TagEnd::Heading(_)) => {
                if let Some(title) = heading.take() {
                    let base = slugify(&title);
                    let mut candidate = base.clone();
                    if anchors.slugs.contains(&candidate) {
                        let next = counts.entry(base.clone()).or_insert(1);
                        loop {
                            candidate = format!("{base}-{next}");
                            *next += 1;
                            if !anchors.slugs.contains(&candidate) {
                                break;
                            }
                        }
                    }
                    anchors.slugs.insert(candidate);
                }
            }
            Event::Text(part) | Event::Code(part) => {
                if let Some(buffer) = heading.as_mut() {
                    buffer.push_str(&part);
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                collect_html_ids(&html, &mut anchors.ids);
            }
            _ => {}
        }
    }
    anchors
}

/// `#L10`, `#L10-L20`, `#L10C2-L12C4`: GitHub line-range anchors, never
/// checked against headings.
fn is_line_anchor(fragment: &str) -> bool {
    let Some(rest) = fragment.strip_prefix('L') else {
        return false;
    };
    let mut seen_digit = false;
    for c in rest.chars() {
        match c {
            '0'..='9' => seen_digit = true,
            'C' | '-' | 'L' if seen_digit => {}
            _ => return false,
        }
    }
    seen_digit
}

/// Percent-decoded fragment of `destination`, or `None` when it is empty,
/// absent, or a GitHub line anchor.
pub fn checkable_fragment(destination: &str) -> Option<String> {
    let (_, fragment) = destination.split_once('#')?;
    let decoded = percent_decode(fragment);
    if decoded.is_empty() || is_line_anchor(&decoded) {
        None
    } else {
        Some(decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::{anchors, checkable_fragment};

    #[test]
    fn heading_slug_with_parentheses_matches() {
        let found = anchors("## Ambiguity Resolution Gate (before prompt-ingestion Stage 2)\n");
        assert!(found.matches("ambiguity-resolution-gate-before-prompt-ingestion-stage-2"));
    }

    #[test]
    fn heading_slug_drops_dots_in_file_names() {
        let found = anchors("## audio-transcript.instructions.md Stage 1\n");
        assert!(found.matches("audio-transcriptinstructionsmd-stage-1"));
    }

    #[test]
    fn duplicate_headings_get_numeric_suffixes() {
        let found = anchors("# x\n\n# x\n");
        assert!(found.matches("x") && found.matches("x-1"));
        assert!(!found.matches("x-2"));
    }

    #[test]
    fn matching_is_case_tolerant_for_slugs() {
        let found = anchors("# Title\n");
        assert!(found.matches("Title") && found.matches("title"));
        assert!(!found.matches("other"));
    }

    #[test]
    fn html_id_and_name_attributes_are_anchors() {
        let found = anchors(
            "<a id=\"custom\"></a>\n\ntext <a name='legacy'>x</a> and <span id=bare>y</span>\n",
        );
        assert!(found.matches("custom") && found.matches("legacy") && found.matches("bare"));
        assert!(!found.matches("nope"));
    }

    #[test]
    fn checkable_fragment_decodes_and_skips_line_anchors() {
        assert_eq!(checkable_fragment("a.md#sec%20x").as_deref(), Some("sec x"));
        assert_eq!(checkable_fragment("a.md#caf%C3%A9").as_deref(), Some("café"));
        assert_eq!(checkable_fragment("a.md"), None);
        assert_eq!(checkable_fragment("a.md#"), None);
        for line in ["#L10", "#L10-L20", "#L10C2-L12C4"] {
            assert_eq!(checkable_fragment(&format!("a.rs{line}")), None, "{line}");
        }
        assert_eq!(checkable_fragment("a.md#Lx").as_deref(), Some("Lx"));
    }
}
