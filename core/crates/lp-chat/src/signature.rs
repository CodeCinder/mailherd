//! Signatures learned from a sender's earlier messages.
//!
//! Many people sign off without the standard "-- " delimiter. When the same
//! block of lines ends several messages from one sender, it is their signature.

// Two-line sign-offs ("Thanks,\nGrace") are part of the message, not a signature.
const MIN_LINES: usize = 3;
const MAX_LINES: usize = 12;

fn content_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect()
}

/// Find a signature shared by the ends of at least two messages from one sender.
///
/// Returns the signature lines joined by newlines, or `None` when the samples
/// share no trailing block of 3–12 lines that is shorter than every message.
pub fn learn_signature(samples: &[&str]) -> Option<String> {
    if samples.len() < 2 {
        return None;
    }
    let all: Vec<Vec<String>> = samples.iter().map(|s| content_lines(s)).collect();
    let shortest = all.iter().map(Vec::len).min()?;
    let mut common = 0;
    while common < shortest.min(MAX_LINES) {
        let line = &all[0][all[0].len() - 1 - common];
        if all
            .iter()
            .all(|lines| &lines[lines.len() - 1 - common] == line)
        {
            common += 1;
        } else {
            break;
        }
    }
    // The signature must leave some real text in every message.
    if common < MIN_LINES || common >= shortest {
        return None;
    }
    let first = &all[0];
    Some(first[first.len() - common..].join("\n"))
}

/// Remove a learned signature from the end of `text`, if it is there.
/// Returns the text without it and whether it was removed.
pub fn strip_learned_signature(text: &str, signature: &str) -> (String, bool) {
    let sig = content_lines(signature);
    if sig.is_empty() {
        return (text.to_string(), false);
    }
    let raw_lines: Vec<&str> = text.lines().collect();
    // Indices of non-blank lines, so whitespace differences don't matter.
    let content: Vec<(usize, String)> = raw_lines
        .iter()
        .enumerate()
        .map(|(i, l)| (i, l.split_whitespace().collect::<Vec<_>>().join(" ")))
        .filter(|(_, l)| !l.is_empty())
        .collect();
    if content.len() <= sig.len() {
        return (text.to_string(), false);
    }
    let tail = &content[content.len() - sig.len()..];
    if tail.iter().map(|(_, l)| l).ne(sig.iter()) {
        return (text.to_string(), false);
    }
    let cut = tail[0].0;
    (raw_lines[..cut].join("\n").trim_end().to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str =
        "Thanks, see you Tuesday.\n\nAda Lovelace\nAnalytical Engines Ltd\n+44 20 7946 0000";
    const B: &str = "Attached is the plan.\nLet me know.\n\nAda  Lovelace\nAnalytical Engines Ltd\n+44 20 7946 0000\n";

    #[test]
    fn learns_a_shared_trailing_block() {
        assert_eq!(
            learn_signature(&[A, B]).as_deref(),
            Some("Ada Lovelace\nAnalytical Engines Ltd\n+44 20 7946 0000")
        );
    }

    #[test]
    fn needs_two_samples_and_three_lines() {
        assert!(learn_signature(&[A]).is_none());
        assert!(learn_signature(&["Hi\nThanks", "Hello\nThanks"]).is_none());
        assert!(learn_signature(&["Yes.\nThanks,\nGrace", "No.\nThanks,\nGrace"]).is_none());
    }

    #[test]
    fn never_learns_a_whole_message() {
        assert!(learn_signature(&["Yes\nok", "Yes\nok"]).is_none());
    }

    #[test]
    fn strips_a_learned_signature() {
        let sig = learn_signature(&[A, B]).expect("learned");
        let (text, removed) = strip_learned_signature(
            "New note.\n\nAda Lovelace\nAnalytical Engines Ltd\n+44 20 7946 0000",
            &sig,
        );
        assert!(removed);
        assert_eq!(text, "New note.");
        let (text, removed) = strip_learned_signature("Different ending.\nBye", &sig);
        assert!(!removed);
        assert_eq!(text, "Different ending.\nBye");
    }
}
