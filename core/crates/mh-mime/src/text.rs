//! Conversion of HTML bodies to plain text for chat bubbles, previews and search.

/// Wide enough that paragraphs are never hard-wrapped; the UI wraps text itself.
const RENDER_WIDTH: usize = 10_000;

/// Convert HTML to plain text. Blockquotes become `> ` prefixed lines, so the
/// quote stripper in `mh-chat` treats HTML and plain-text replies the same way.
pub fn html_to_text(html: &str) -> String {
    let rendered = html2text::config::plain_no_decorate()
        .link_footnotes(false)
        .no_table_borders()
        .string_from_read(html.as_bytes(), RENDER_WIDTH)
        .unwrap_or_default();
    tidy(&rendered)
}

/// Trim trailing spaces on each line and collapse runs of blank lines.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0;
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() || line == ">" {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_paragraphs_and_line_breaks() {
        let text = html_to_text("<p>Hello Ada,</p><p>See you<br>tomorrow.</p>");
        assert_eq!(text, "Hello Ada,\n\nSee you\ntomorrow.");
    }

    #[test]
    fn blockquotes_become_quoted_lines() {
        let text = html_to_text("<p>Sounds good</p><blockquote><p>Lunch at 1?</p></blockquote>");
        assert!(text.starts_with("Sounds good"));
        assert!(text.contains("> Lunch at 1?"), "got: {text}");
    }

    #[test]
    fn long_paragraphs_are_not_wrapped() {
        let long = "word ".repeat(100);
        let text = html_to_text(&format!("<p>{long}</p>"));
        assert_eq!(text.lines().count(), 1);
    }
}
