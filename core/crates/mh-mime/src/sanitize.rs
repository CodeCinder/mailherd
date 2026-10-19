//! HTML sanitising for display in a locked-down viewer.
//!
//! Scripts, forms, event handlers and anything that could load remote content
//! are removed. Remote images are blocked unless the user allows them, and
//! known tracking pixels are always removed.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use url::Url;

use crate::tracking::{clean_url, is_tracker_host};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SanitizeOptions {
    /// Load remote images (the user chose "show images" for this message or sender).
    pub allow_remote_images: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedHtml {
    pub html: String,
    /// Remote images that were not loaded because remote images are blocked.
    pub remote_images_blocked: usize,
    /// Images removed because they come from a known tracking service.
    pub trackers_removed: usize,
}

const EXTRA_TAGS: &[&str] = &["center", "font", "u", "s", "strike", "big", "small", "hr"];

const GENERIC_ATTRIBUTES: &[&str] = &[
    "style",
    "align",
    "valign",
    "bgcolor",
    "width",
    "height",
    "border",
    "cellpadding",
    "cellspacing",
    "color",
    "face",
    "size",
    "dir",
    "title",
    "colspan",
    "rowspan",
];

/// CSS that can fetch remote content or run code is removed from style attributes.
fn clean_style(style: &str) -> Option<String> {
    let kept: Vec<&str> = style
        .split(';')
        .map(str::trim)
        .filter(|decl| {
            let lower = decl.to_ascii_lowercase();
            !decl.is_empty()
                && !lower.contains("url(")
                && !lower.contains("expression(")
                && !lower.contains("@import")
                && !lower.contains("behavior:")
                && !lower.contains("-moz-binding")
        })
        .collect();
    (!kept.is_empty()).then(|| kept.join("; "))
}

fn is_remote(src: &str) -> bool {
    let lower = src.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("//")
}

/// Sanitise an HTML email body for display.
pub fn sanitize_html(html: &str, options: SanitizeOptions) -> SanitizedHtml {
    let blocked = Arc::new(AtomicUsize::new(0));
    let trackers = Arc::new(AtomicUsize::new(0));
    let (blocked_cb, trackers_cb) = (Arc::clone(&blocked), Arc::clone(&trackers));
    let allow_remote = options.allow_remote_images;

    let mut builder = ammonia::Builder::default();
    builder
        .add_tags(EXTRA_TAGS)
        .add_generic_attributes(GENERIC_ATTRIBUTES)
        .url_schemes(HashSet::from([
            "http", "https", "mailto", "tel", "cid", "data",
        ]))
        .url_relative(ammonia::UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer nofollow"))
        .strip_comments(true)
        .attribute_filter(move |element, attribute, value| {
            match (element, attribute) {
                ("img", "src") => {
                    let src = value.trim();
                    if src.to_ascii_lowercase().starts_with("data:image/") {
                        return Some(Cow::Borrowed(value));
                    }
                    if src.to_ascii_lowercase().starts_with("cid:") {
                        return Some(Cow::Borrowed(value));
                    }
                    if !is_remote(src) {
                        return None;
                    }
                    let absolute = if src.starts_with("//") {
                        format!("https:{src}")
                    } else {
                        src.to_string()
                    };
                    let host = Url::parse(&absolute)
                        .ok()
                        .and_then(|u| u.host_str().map(str::to_string));
                    if host.as_deref().is_some_and(is_tracker_host) {
                        trackers_cb.fetch_add(1, Ordering::Relaxed);
                        return None;
                    }
                    if allow_remote {
                        Some(Cow::Owned(absolute))
                    } else {
                        blocked_cb.fetch_add(1, Ordering::Relaxed);
                        None
                    }
                }
                // Responsive image sources would bypass the src checks above.
                ("img", "srcset") => None,
                (_, "href") => {
                    if value.trim().to_ascii_lowercase().starts_with("data:") {
                        None
                    } else {
                        Some(Cow::Owned(clean_url(value)))
                    }
                }
                (_, "style") => clean_style(value).map(Cow::Owned),
                (_, "background") => None,
                _ => Some(Cow::Borrowed(value)),
            }
        });

    let html = builder.clean(html).to_string();
    SanitizedHtml {
        html,
        remote_images_blocked: blocked.load(Ordering::Relaxed),
        trackers_removed: trackers.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str) -> SanitizedHtml {
        sanitize_html(html, SanitizeOptions::default())
    }

    #[test]
    fn removes_scripts_and_event_handlers() {
        let out = clean(
            r#"<p onclick="steal()">Hi</p><script>alert(1)</script><iframe src="https://x"></iframe>"#,
        );
        assert_eq!(out.html, "<p>Hi</p>");
    }

    #[test]
    fn blocks_remote_images_by_default() {
        let out = clean(r#"<img src="https://cdn.example.org/logo.png" alt="Logo">"#);
        assert!(!out.html.contains("cdn.example.org"));
        assert!(out.html.contains(r#"alt="Logo""#));
        assert_eq!(out.remote_images_blocked, 1);
    }

    #[test]
    fn allows_remote_images_when_asked_but_still_drops_trackers() {
        let html = r#"<img src="https://cdn.example.org/a.png"><img src="https://us5.list-manage.com/track/open.php?u=1">"#;
        let out = sanitize_html(
            html,
            SanitizeOptions {
                allow_remote_images: true,
            },
        );
        assert!(out.html.contains("cdn.example.org/a.png"));
        assert!(!out.html.contains("list-manage"));
        assert_eq!(out.trackers_removed, 1);
        assert_eq!(out.remote_images_blocked, 0);
    }

    #[test]
    fn keeps_inline_and_embedded_images() {
        let out = clean(r#"<img src="cid:logo@x"><img src="data:image/png;base64,iVBOR">"#);
        assert!(out.html.contains("cid:logo@x"));
        assert!(out.html.contains("data:image/png;base64,iVBOR"));
    }

    #[test]
    fn strips_remote_css_but_keeps_layout_styles() {
        let out = clean(
            r#"<div style="color: red; background-image: url(https://t.example/p.gif); padding: 4px">x</div>"#,
        );
        assert!(out.html.contains("color: red; padding: 4px"));
        assert!(!out.html.contains("url("));
    }

    #[test]
    fn cleans_tracking_from_links() {
        let out = clean(r#"<a href="https://example.org/a?utm_source=mail">Read</a>"#);
        assert!(out.html.contains(r#"href="https://example.org/a""#));
        assert!(out.html.contains("noopener"));
    }

    #[test]
    fn drops_data_links_and_srcset() {
        let out = clean(
            r#"<a href="data:text/html,evil">x</a><img srcset="https://t.example/a.png 2x">"#,
        );
        assert!(!out.html.contains("data:text"));
        assert!(!out.html.contains("t.example"));
    }
}
