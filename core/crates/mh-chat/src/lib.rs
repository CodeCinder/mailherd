//! Splitting a message into what is new and what is history.
//!
//! Chat bubbles show only the new text of each message. Quoted history,
//! forwarded content and signatures are split off so the UI can tuck them
//! behind a "…" toggle. Detection is conservative: when in doubt, text stays
//! visible, because hiding something the sender wrote is worse than showing
//! a quote twice.

use std::sync::LazyLock;

use regex::Regex;

mod signature;

pub use signature::{learn_signature, strip_learned_signature};

/// A message body split into its parts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplyParts {
    /// What the sender wrote in this message.
    pub new_text: String,
    /// Quoted or forwarded history below (or above) the new text.
    pub quoted: Option<String>,
    pub signature: Option<String>,
    /// True when the history is a forwarded message rather than a reply quote.
    pub forwarded: bool,
}

impl ReplyParts {
    /// True when something is hidden behind the "…" toggle.
    pub fn has_hidden(&self) -> bool {
        self.quoted.is_some() || self.signature.is_some()
    }
}

/// "On <date>, <name> wrote:" and its equivalents in other languages.
static ATTRIBUTION: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?i)^on\b.{3,300}\bwrote\s*:$",
        r"(?i)^am\b.{3,300}\bschrieb\b.{0,300}:$",
        r"(?i)^le\b.{3,300}\ba [ée]crit\s*:$",
        r"(?i)^el\b.{3,300}\bescribi[óo]\s*:$",
        r"(?i)^il\b.{3,300}\bha scritto\s*:$",
        r"(?i)^op\b.{3,300}\bschreef\b.{0,300}:$",
        r"(?i)^(em|no dia)\b.{3,300}\bescreveu\s*:$",
        r"(?i)^den\b.{3,300}\bskrev\b.{0,300}:$",
        r"(?i)^.{3,300}\bskrev\s*:$",
        r"(?i)^(w dniu|dnia)\b.{3,300}\b(pisze|napisa[łl]\(a\)|napisa[łl]a?)\s*:$",
        r"(?i)^.{3,300}\b(пишет|написал\(а\)|написала?)\s*:$",
        r"(?i)^.{3,300}\bkirjoitti\s*:$",
        r"(?i)^.{3,300}\bnapsal(\(a\)|a)?\s*:$",
        r"(?i)^.{3,300}\bşunu yazdı\s*:$",
        r"^.{3,300}(のメッセージ|は書きました)\s*[:：]$",
        r"^(在|於).{3,300}(写道|寫道)\s*[:：]$",
        r"^.{3,300}작성\s*[:：]$",
    ]
    .iter()
    .filter_map(|p| Regex::new(p).ok())
    .collect()
});

/// Separators that start a forwarded message.
static FORWARD_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(-{2,}\s*(forwarded message|weitergeleitete nachricht|message transf[ée]r[ée]|mensaje reenviado|messaggio inoltrato|doorgestuurd bericht|mensagem encaminhada|vidarebefordrat meddelande)\s*-{2,}|begin forwarded message\s*:|anfang der weitergeleiteten nachricht\s*:|d[ée]but du message r[ée]exp[ée]di[ée]\s*:)$",
    )
    .expect("valid regex")
});

/// Separators that start an Outlook or Thunderbird style quote.
static ORIGINAL_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^-{2,}\s*(original message|urspr[üu]ngliche nachricht|message d'origine|mensaje original|messaggio originale|oorspronkelijk bericht|mensagem original|originalmeddelande|opprinnelig melding|alkuper[äa]inen viesti|p[ůu]vodn[íi] zpr[áa]va|oryginalna wiadomo[śs][ćc])\s*-{2,}$",
    )
    .expect("valid regex")
});

/// The "From:" line of an Outlook header block.
static HEADER_FROM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\*?(from|von|de|da|van|fr[åa]n|fra|od|l[äa]hett[äa]j[äa]|от|差出人|发件人|寄件者)\s*[:：]\*?\s*\S")
        .expect("valid regex")
});

/// Other lines of an Outlook header block.
static HEADER_OTHER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\*?(sent|date|gesendet|datum|envoy[ée]|enviado|inviato|verzonden|skickat|sendt|wys[łl]ano|отправлено|送信日時|发送时间|日期|to|an|[àa]|para|aan|till|til|do|subject|betreff|objet|asunto|oggetto|onderwerp|[äa]mne|emne|temat|тема|件名|主题|主旨|cc|kopie)\s*[:：]")
        .expect("valid regex")
});

/// Footers added by mail apps, treated as signatures.
static APP_FOOTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(sent from my \w[\w\s]{0,40}|sent from (mail|outlook) for \w+|sent from outlook|sent from yahoo mail.{0,40}|get outlook for \w+|sent with proton mail.{0,40}|sent from proton mail.{0,40}|envoy[ée] de mon \w+|von meinem \w+ gesendet|enviado desde mi \w+|inviato da(l mio)? \w+|verzonden (vanaf|met) mijn \w+|skickat fr[åa]n min \w+|sendt fra min \w+)\.?$",
    )
    .expect("valid regex")
});

fn is_quoted(line: &str) -> bool {
    line.trim_start().starts_with('>')
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn is_underscore_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 8 && t.chars().all(|c| c == '_')
}

fn is_attribution(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && ATTRIBUTION.iter().any(|re| re.is_match(t))
}

/// Does an Outlook-style header block ("From: … Sent: … To: … Subject: …") start here?
fn is_header_block(lines: &[&str], i: usize) -> bool {
    if !HEADER_FROM.is_match(lines[i].trim()) {
        return false;
    }
    let others = lines
        .iter()
        .skip(i + 1)
        .take(5)
        .filter(|l| HEADER_OTHER.is_match(l.trim()))
        .count();
    others >= 2
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CutKind {
    Reply,
    Forward,
}

/// Find the first line where history starts.
fn find_cut(lines: &[&str]) -> Option<(usize, CutKind)> {
    for i in 0..lines.len() {
        let line = lines[i].trim();
        if line.is_empty() {
            continue;
        }
        if FORWARD_MARKER.is_match(line) {
            return Some((i, CutKind::Forward));
        }
        if ORIGINAL_MARKER.is_match(line) {
            return Some((i, CutKind::Reply));
        }
        if is_attribution(line) {
            return Some((i, CutKind::Reply));
        }
        // Gmail and Apple Mail wrap long attributions across two lines.
        if let Some(next) = lines.get(i + 1) {
            let joined = format!("{line} {}", next.trim());
            if !is_quoted(lines[i]) && is_attribution(&joined) {
                return Some((i, CutKind::Reply));
            }
        }
        if is_header_block(lines, i) {
            // Include an underscore rule directly above the header block.
            let mut start = i;
            let mut j = i;
            while j > 0 {
                j -= 1;
                if is_blank(lines[j]) {
                    continue;
                }
                if is_underscore_rule(lines[j]) {
                    start = j;
                }
                break;
            }
            return Some((start, CutKind::Reply));
        }
    }
    trailing_quote_start(lines).map(|i| (i, CutKind::Reply))
}

/// Start of a trailing block of `>` lines, unless the message is an inline
/// (interleaved) reply, in which case nothing is cut.
fn trailing_quote_start(lines: &[&str]) -> Option<usize> {
    let last = lines.iter().rposition(|l| !is_blank(l))?;
    if !is_quoted(lines[last]) {
        return None;
    }
    let mut start = last;
    while start > 0 && (is_quoted(lines[start - 1]) || is_blank(lines[start - 1])) {
        start -= 1;
    }
    while start <= last && is_blank(lines[start]) {
        start += 1;
    }
    let interleaved = lines[..start].iter().any(|l| is_quoted(l));
    if interleaved {
        return None;
    }
    // An attribution in a format we don't know ("2026年10月19日 Ada <ada@x>:")
    // still ends with a colon and names an address, directly above the quote.
    if let Some(above) = lines[..start].iter().rposition(|l| !is_blank(l)) {
        let t = lines[above].trim();
        if (t.ends_with(':') || t.ends_with('：')) && t.contains('@') {
            return Some(above);
        }
    }
    Some(start)
}

fn join(lines: &[&str]) -> String {
    lines.join("\n").trim().to_string()
}

/// Split trailing signatures and app footers off the new text.
fn split_signature(lines: &[&str]) -> (usize, Option<String>) {
    // A standard "-- " delimiter, used by most clients.
    if let Some(pos) = lines
        .iter()
        .rposition(|l| l.trim_end() == "--" || *l == "-- ")
    {
        let sig = &lines[pos + 1..];
        let sig_lines = sig.iter().filter(|l| !is_blank(l)).count();
        if sig_lines > 0 && sig_lines <= 20 {
            return (pos, Some(join(sig)));
        }
    }
    // "Sent from my iPhone" and similar footers at the very end.
    let mut end = lines.len();
    while end > 0 && is_blank(lines[end - 1]) {
        end -= 1;
    }
    if end > 0 && APP_FOOTER.is_match(lines[end - 1].trim()) {
        return (end - 1, Some(lines[end - 1].trim().to_string()));
    }
    (lines.len(), None)
}

/// Text written after the quote ("bottom-posting"), for messages whose top is empty.
fn bottom_posted(quoted: &[&str]) -> Option<(usize, usize)> {
    let last_quote = quoted.iter().rposition(|l| is_quoted(l))?;
    let after = &quoted[last_quote + 1..];
    if after.iter().all(|l| is_blank(l)) {
        return None;
    }
    Some((last_quote + 1, quoted.len()))
}

/// Normalise line endings and remove a leading byte-order mark.
fn normalize(text: &str) -> String {
    text.trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\u{a0}', " ")
}

/// Split a plain-text message body into new text, quoted history and signature.
pub fn split_reply(text: &str) -> ReplyParts {
    let normalized = normalize(text);
    let lines: Vec<&str> = normalized.lines().collect();

    let (cut, kind) = match find_cut(&lines) {
        Some((cut, kind)) => (cut, Some(kind)),
        None => (lines.len(), None),
    };
    let (top, history) = lines.split_at(cut);
    let (sig_at, signature) = split_signature(top);
    let new_text = join(&top[..sig_at]);
    let quoted = (!history.is_empty())
        .then(|| join(history))
        .filter(|q| !q.is_empty());
    let forwarded = kind == Some(CutKind::Forward);

    if !new_text.is_empty() || forwarded || quoted.is_none() {
        return ReplyParts {
            new_text,
            quoted,
            signature,
            forwarded,
        };
    }

    // Nothing above the quote: the sender may have written below it.
    if let Some((from, to)) = bottom_posted(history) {
        let below = &history[from..to];
        let (sig_at, signature) = split_signature(below);
        let new_text = join(&below[..sig_at]);
        if !new_text.is_empty() {
            return ReplyParts {
                new_text,
                quoted: Some(join(&history[..from])),
                signature,
                forwarded: false,
            };
        }
    }

    // Only a quote and nothing new: show everything rather than an empty bubble.
    ReplyParts {
        new_text: normalized.trim().to_string(),
        quoted: None,
        signature: None,
        forwarded: false,
    }
}

/// A one-line preview of the new text, for thread lists and notifications.
pub fn snippet(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let keep = max_chars.saturating_sub(1);
    let mut cut: String = collapsed.chars().take(keep).collect();
    let mid_word = collapsed.chars().nth(keep).is_some_and(|c| c != ' ');
    if mid_word && let Some(space) = cut.rfind(' ').filter(|&p| p > max_chars / 2) {
        cut.truncate(space);
    }
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_message_is_untouched() {
        let parts = split_reply("Hello!\n\nSee you soon.");
        assert_eq!(parts.new_text, "Hello!\n\nSee you soon.");
        assert!(!parts.has_hidden());
    }

    #[test]
    fn inline_reply_is_kept_whole() {
        let text = "> Can you do Tuesday?\nYes.\n> And bring the slides?\nWill do.";
        let parts = split_reply(text);
        assert_eq!(parts.new_text, text);
        assert!(parts.quoted.is_none());
    }

    #[test]
    fn mid_sentence_wrote_is_not_an_attribution() {
        let text = "On Monday the team wrote: a plan.\nIt looks good to me.";
        assert_eq!(split_reply(text).new_text, text);
    }

    #[test]
    fn snippets_collapse_whitespace_and_truncate_at_a_word() {
        assert_eq!(snippet("Hello\n\n  world", 50), "Hello world");
        assert_eq!(
            snippet("The quick brown fox jumps over", 16),
            "The quick brown…"
        );
    }
}
