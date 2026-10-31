//! Link extraction with line numbers and destination spans.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag};

use crate::line_index::LineIndex;
use crate::span::locate;

/// One link, image or autolink found outside code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRef {
    /// Parsed destination (`dest_url`): escapes and entities processed, not
    /// percent-decoded.
    pub destination: String,
    /// 1-based line of the link start.
    pub line: usize,
    /// Byte range of `destination` verbatim in the source, or `None` when it
    /// is not found verbatim (angle-bracket form, escapes, entities).
    pub span: Option<Range<usize>>,
}

/// Extracts inline, reference (full/collapsed/shortcut), autolink and image
/// links in source order. Nothing inside code blocks or code spans is a link.
pub fn extract(text: &str) -> Vec<LinkRef> {
    let line_index = LineIndex::new(text);
    let mut links = Vec::new();
    let mut events = Parser::new_ext(text, Options::empty()).into_offset_iter();
    while let Some((event, range)) = events.next() {
        let (Event::Start(Tag::Link {
            link_type,
            dest_url,
            id,
            ..
        })
        | Event::Start(Tag::Image {
            link_type,
            dest_url,
            id,
            ..
        })) = event
        else {
            continue;
        };
        let definition = events
            .reference_definitions()
            .get(&id)
            .map(|definition| definition.span.clone());
        links.push(LinkRef {
            line: line_index.line_number(range.start),
            span: locate(text, link_type, &range, &dest_url, definition.as_ref()),
            destination: dest_url.into_string(),
        });
    }
    links
}

#[cfg(test)]
mod tests;
