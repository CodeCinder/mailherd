//! MIME parsing, HTML sanitising and tracker removal.
//!
//! Every message that reaches a UI passes through this crate first: raw
//! RFC 5322 bytes come in, and plain data with sanitised HTML comes out.

mod build;
mod parse;
mod sanitize;
mod text;
mod tracking;

pub use build::{OutgoingMessage, QuotedOriginal, build_message, reply_subject, text_to_html};
pub use parse::{ParseError, ParsedMessage, parse_message};
pub use sanitize::{SanitizeOptions, SanitizedHtml, sanitize_html};
pub use text::html_to_text;
pub use tracking::{clean_url, is_tracker_host};
