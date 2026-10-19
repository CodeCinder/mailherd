//! Building outgoing messages: multipart/alternative with correct threading
//! headers and a standard quote of the previous message.

use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address as BuilderAddress;
use mail_builder::headers::message_id::MessageId;
use mh_model::{Address, Timestamp};

/// The message being replied to, quoted below the new text for recipients
/// who read mail in an ordinary client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotedOriginal {
    pub from: Address,
    /// Already formatted for the sender's locale, for example "Mon, 19 Oct 2026 at 12:00".
    pub date_label: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingMessage {
    pub message_id: String,
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub date: Timestamp,
    /// What the user typed, as plain text.
    pub text: String,
    pub quote: Option<QuotedOriginal>,
}

/// Prefix "Re: " unless the subject already starts with a reply prefix.
pub fn reply_subject(subject: &str) -> String {
    let trimmed = subject.trim();
    let lower = trimmed.to_lowercase();
    if lower.starts_with("re:") || lower.starts_with("aw:") || lower.starts_with("sv:") {
        trimmed.to_string()
    } else if trimmed.is_empty() {
        "Re:".to_string()
    } else {
        format!("Re: {trimmed}")
    }
}

/// Escape text for HTML and turn line breaks into paragraphs and `<br>`.
pub fn text_to_html(text: &str) -> String {
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    escaped
        .split("\n\n")
        .map(|para| format!("<p>{}</p>", para.trim_end().replace('\n', "<br>")))
        .collect::<Vec<_>>()
        .join("\n")
}

fn attribution(quote: &QuotedOriginal) -> String {
    let who = match &quote.from.name {
        Some(name) => format!("{name} <{}>", quote.from.email),
        None => quote.from.email.clone(),
    };
    format!("On {}, {} wrote:", quote.date_label, who)
}

fn plain_body(message: &OutgoingMessage) -> String {
    let mut body = message.text.trim_end().to_string();
    if let Some(quote) = &message.quote {
        body.push_str("\n\n");
        body.push_str(&attribution(quote));
        body.push('\n');
        for line in quote.text.trim_end().lines() {
            if line.is_empty() {
                body.push_str(">\n");
            } else {
                body.push_str("> ");
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    body.push('\n');
    body
}

fn html_body(message: &OutgoingMessage) -> String {
    let mut html = String::from("<!DOCTYPE html><html><body>\n");
    html.push_str(&text_to_html(message.text.trim_end()));
    if let Some(quote) = &message.quote {
        html.push_str("\n<div class=\"mh_quote\"><p>");
        html.push_str(
            &text_to_html(&attribution(quote))
                .replace("<p>", "")
                .replace("</p>", ""),
        );
        html.push_str("</p><blockquote type=\"cite\" style=\"margin:0 0 0 .8ex;border-left:1px solid #ccc;padding-left:1ex\">");
        html.push_str(&text_to_html(quote.text.trim_end()));
        html.push_str("</blockquote></div>");
    }
    html.push_str("\n</body></html>\n");
    html
}

fn builder_address(address: &Address) -> BuilderAddress<'static> {
    BuilderAddress::new_address(address.name.clone(), address.email.clone())
}

fn builder_list(addresses: &[Address]) -> BuilderAddress<'static> {
    BuilderAddress::new_list(addresses.iter().map(builder_address).collect())
}

/// Serialise an outgoing message to RFC 5322 bytes ready for SMTP or an API upload.
pub fn build_message(message: &OutgoingMessage) -> std::io::Result<Vec<u8>> {
    let mut builder = MessageBuilder::new()
        .message_id(message.message_id.clone())
        .from(builder_address(&message.from))
        .subject(message.subject.clone())
        .date(message.date)
        .text_body(plain_body(message))
        .html_body(html_body(message));
    if !message.to.is_empty() {
        builder = builder.to(builder_list(&message.to));
    }
    if !message.cc.is_empty() {
        builder = builder.cc(builder_list(&message.cc));
    }
    if !message.bcc.is_empty() {
        builder = builder.bcc(builder_list(&message.bcc));
    }
    if let Some(parent) = &message.in_reply_to {
        builder = builder.in_reply_to(parent.clone());
    }
    if !message.references.is_empty() {
        builder = builder.references(MessageId::new_list(message.references.iter().cloned()));
    }
    builder.write_to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_message;

    fn reply() -> OutgoingMessage {
        OutgoingMessage {
            message_id: "new@mailherd.local".into(),
            from: Address::new(Some("Me"), "me@example.org"),
            to: vec![Address::new(Some("Ada"), "ada@example.org")],
            cc: vec![Address::new(None, "bob@example.org")],
            bcc: vec![],
            subject: reply_subject("Lunch"),
            in_reply_to: Some("m1@example.org".into()),
            references: vec!["m0@example.org".into(), "m1@example.org".into()],
            date: 1_792_411_200,
            text: "Sounds good <3\n\nSee you there.".into(),
            quote: Some(QuotedOriginal {
                from: Address::new(Some("Ada"), "ada@example.org"),
                date_label: "Mon, 19 Oct 2026 at 11:00".into(),
                text: "Lunch at 1?".into(),
            }),
        }
    }

    #[test]
    fn reply_subjects() {
        assert_eq!(reply_subject("Lunch"), "Re: Lunch");
        assert_eq!(reply_subject("RE: Lunch"), "RE: Lunch");
        assert_eq!(reply_subject("  "), "Re:");
    }

    #[test]
    fn built_reply_round_trips_through_the_parser() {
        let raw = build_message(&reply()).expect("builds");
        let parsed = parse_message(&raw).expect("parses");
        assert_eq!(parsed.rfc_message_id.as_deref(), Some("new@mailherd.local"));
        assert_eq!(parsed.in_reply_to, vec!["m1@example.org"]);
        assert_eq!(parsed.references, vec!["m0@example.org", "m1@example.org"]);
        assert_eq!(parsed.subject, "Re: Lunch");
        assert_eq!(parsed.cc[0].email, "bob@example.org");
        let text = parsed.body.text.expect("text part");
        assert!(text.starts_with("Sounds good <3"));
        assert!(text.contains("On Mon, 19 Oct 2026 at 11:00, Ada <ada@example.org> wrote:"));
        assert!(text.contains("> Lunch at 1?"));
        let html = parsed.body.html.expect("html part");
        assert!(html.contains("Sounds good &lt;3"));
        assert!(html.contains("<blockquote"));
    }

    #[test]
    fn text_to_html_escapes_and_breaks_lines() {
        assert_eq!(text_to_html("a<b\nc\n\nd"), "<p>a&lt;b<br>c</p>\n<p>d</p>");
    }
}
