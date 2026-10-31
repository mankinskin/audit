//! Link extraction with line numbers and destination spans.

use std::ops::Range;

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

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
    // Open links/images: (index in `links`, range, max end of nested events).
    let mut open: Vec<(usize, Range<usize>, usize)> = Vec::new();
    // Per link: (link type, definition span), resolved when the link closes.
    let mut pending: Vec<Option<(LinkType, Option<Range<usize>>)>> = Vec::new();
    while let Some((event, range)) = events.next() {
        match event {
            Event::Start(Tag::Link {
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
            }) => {
                for entry in &mut open {
                    entry.2 = entry.2.max(range.end);
                }
                let definition = events
                    .reference_definitions()
                    .get(&id)
                    .map(|definition| definition.span.clone());
                open.push((links.len(), range.clone(), range.start + 1));
                pending.push(Some((link_type, definition)));
                links.push(LinkRef {
                    line: line_index.line_number(range.start),
                    span: None,
                    destination: dest_url.into_string(),
                });
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                if let Some((index, range, text_end)) = open.pop() {
                    if let Some((link_type, definition)) = pending[index].take() {
                        let link = &mut links[index];
                        link.span = locate(
                            text,
                            link_type,
                            &range,
                            text_end,
                            &link.destination,
                            definition.as_ref(),
                        );
                    }
                }
            }
            _ => {
                for entry in &mut open {
                    entry.2 = entry.2.max(range.end);
                }
            }
        }
    }
    links
}

#[cfg(test)]
mod tests;
