//! Shared CommonMark link parsing.
//!
//! This crate owns Markdown link *syntax*: finding real links (never inside
//! code), their byte spans, heading/HTML anchors, destination shapes and path
//! resolution. Consumers own the rules built on top of it.

mod anchors;
mod destination;
mod extract;
mod line_index;
mod path;
mod span;

pub use anchors::{Anchors, anchors, checkable_fragment};
pub use destination::{DestinationShape, classify_destination};
pub use extract::{LinkRef, extract};
pub use path::{decode_percent, encode_destination, resolve_local};
