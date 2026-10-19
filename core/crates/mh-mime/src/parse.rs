//! Parsing raw RFC 5322 messages into plain data.

use mail_parser::{HeaderValue, MessageParser, MimeHeaders};
use mh_model::{Address, AttachmentInfo, Body, Timestamp, normalize_message_id};

use crate::sanitize::{SanitizeOptions, sanitize_html};
use crate::text::html_to_text;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("the data is not an email message")]
    NotAMessage,
}

/// Everything the core needs from one raw message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMessage {
    pub rfc_message_id: Option<String>,
    pub in_reply_to: Vec<String>,
    pub references: Vec<String>,
    pub subject: String,
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub date: Option<Timestamp>,
    /// Plain text body: the text/plain part, or text derived from the HTML part.
    /// HTML is sanitised with remote images blocked.
    pub body: Body,
    pub attachments: Vec<AttachmentInfo>,
    /// Mailing-list or bulk mail (has List-Id, List-Unsubscribe or Precedence: bulk).
    pub is_bulk: bool,
    pub list_id: Option<String>,
    /// Raw Authentication-Results header, for SPF/DKIM/DMARC display.
    pub authentication_results: Option<String>,
    pub remote_images_blocked: usize,
}

fn addresses(value: Option<&mail_parser::Address<'_>>) -> Vec<Address> {
    value
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    let email = a.address.as_deref()?.trim();
                    (!email.is_empty()).then(|| Address::new(a.name.as_deref(), email))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn message_ids(value: &HeaderValue<'_>) -> Vec<String> {
    let ids: Vec<String> = match value {
        HeaderValue::Text(id) => vec![normalize_message_id(id)],
        HeaderValue::TextList(list) => list.iter().map(|id| normalize_message_id(id)).collect(),
        _ => Vec::new(),
    };
    ids.into_iter().filter(|id| !id.is_empty()).collect()
}

fn has_header(message: &mail_parser::Message<'_>, name: &str) -> bool {
    message.header_raw(name).is_some()
}

/// Parse a raw message. Never panics on malformed input.
pub fn parse_message(raw: &[u8]) -> Result<ParsedMessage, ParseError> {
    let message = MessageParser::default()
        .parse(raw)
        .ok_or(ParseError::NotAMessage)?;
    if message.headers().is_empty() {
        return Err(ParseError::NotAMessage);
    }

    let html_raw: Option<String> = message
        .html_bodies()
        .filter(|part| part.is_text_html())
        .filter_map(|part| part.text_contents())
        .next()
        .map(str::to_string);
    let text_raw: Option<String> = message
        .text_bodies()
        .filter(|part| !part.is_text_html())
        .filter_map(|part| part.text_contents())
        .next()
        .map(str::to_string);

    let sanitized = html_raw
        .as_deref()
        .map(|html| sanitize_html(html, SanitizeOptions::default()));
    let text = text_raw.or_else(|| sanitized.as_ref().map(|s| html_to_text(&s.html)));
    let remote_images_blocked = sanitized.as_ref().map_or(0, |s| s.remote_images_blocked);

    let attachments = message
        .attachments()
        .map(|part| {
            let mime_type = part
                .content_type()
                .map(|ct| match ct.subtype() {
                    Some(sub) => format!("{}/{}", ct.ctype(), sub),
                    None => ct.ctype().to_string(),
                })
                .unwrap_or_else(|| "application/octet-stream".to_string())
                .to_ascii_lowercase();
            AttachmentInfo {
                filename: part.attachment_name().map(str::to_string),
                mime_type,
                size: part.len(),
                content_id: part.content_id().map(normalize_message_id),
                inline: part.content_disposition().is_some_and(|d| d.is_inline()),
            }
        })
        .collect();

    let precedence_bulk = message.header_raw("Precedence").is_some_and(|p| {
        matches!(
            p.trim().to_ascii_lowercase().as_str(),
            "bulk" | "list" | "junk"
        )
    });
    let list_id = message.header_raw("List-Id").map(|v| v.trim().to_string());

    Ok(ParsedMessage {
        rfc_message_id: message
            .message_id()
            .map(normalize_message_id)
            .filter(|id| !id.is_empty()),
        in_reply_to: message_ids(message.in_reply_to()),
        references: message_ids(message.references()),
        subject: message.subject().unwrap_or_default().trim().to_string(),
        from: addresses(message.from()).into_iter().next(),
        to: addresses(message.to()),
        cc: addresses(message.cc()),
        reply_to: addresses(message.reply_to()),
        date: message.date().map(|d| d.to_timestamp()),
        body: Body {
            text,
            html: sanitized.map(|s| s.html),
        },
        attachments,
        is_bulk: list_id.is_some() || has_header(&message, "List-Unsubscribe") || precedence_bulk,
        list_id,
        authentication_results: message
            .header_raw("Authentication-Results")
            .map(|v| v.trim().to_string()),
        remote_images_blocked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: &[u8] = b"Message-ID: <m2@example.org>\r\n\
In-Reply-To: <m1@example.org>\r\n\
References: <m0@example.org> <m1@example.org>\r\n\
From: Ada Lovelace <ada@example.org>\r\n\
To: Grace <grace@example.org>, bob@example.org\r\n\
Subject: Re: Lunch\r\n\
Date: Mon, 19 Oct 2026 12:00:00 +0000\r\n\
\r\n\
Sounds good!\r\n";

    #[test]
    fn parses_headers_and_plain_body() {
        let m = parse_message(PLAIN).expect("parses");
        assert_eq!(m.rfc_message_id.as_deref(), Some("m2@example.org"));
        assert_eq!(m.in_reply_to, vec!["m1@example.org"]);
        assert_eq!(m.references, vec!["m0@example.org", "m1@example.org"]);
        assert_eq!(m.subject, "Re: Lunch");
        assert_eq!(
            m.from,
            Some(Address::new(Some("Ada Lovelace"), "ada@example.org"))
        );
        assert_eq!(m.to.len(), 2);
        assert_eq!(m.to[1].email, "bob@example.org");
        assert_eq!(m.date, Some(1_792_411_200));
        assert_eq!(m.body.text.as_deref().map(str::trim), Some("Sounds good!"));
        assert!(m.body.html.is_none());
        assert!(!m.is_bulk);
    }

    #[test]
    fn html_only_message_gets_text_and_sanitised_html() {
        let raw = b"From: news@shop.example\r\nSubject: Sale\r\nList-Unsubscribe: <mailto:u@shop.example>\r\n\
Content-Type: text/html; charset=utf-8\r\n\r\n\
<p>Big <b>sale</b></p><img src=\"https://shop.example/pixel.gif\"><script>x()</script>";
        let m = parse_message(raw).expect("parses");
        assert_eq!(m.body.text.as_deref(), Some("Big sale"));
        let html = m.body.html.expect("has html");
        assert!(!html.contains("script"));
        assert!(!html.contains("pixel.gif"));
        assert_eq!(m.remote_images_blocked, 1);
        assert!(m.is_bulk);
    }

    #[test]
    fn lists_attachments() {
        let raw = b"From: a@x.org\r\nSubject: Files\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nSee attached.\r\n\
--b\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"plan.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--b--\r\n";
        let m = parse_message(raw).expect("parses");
        assert_eq!(m.attachments.len(), 1);
        assert_eq!(m.attachments[0].filename.as_deref(), Some("plan.pdf"));
        assert_eq!(m.attachments[0].mime_type, "application/pdf");
        assert!(!m.attachments[0].inline);
    }

    #[test]
    fn rejects_garbage_without_panicking() {
        assert!(parse_message(b"").is_err());
        let _ = parse_message(&[0xff, 0xfe, 0x00, 0x13, 0x37]);
    }
}
