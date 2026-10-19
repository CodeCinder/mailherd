//! Labels for dates, avatars and names, shared by every UI.

use lp_model::{Address, Timestamp};
use time::{Date, OffsetDateTime, UtcOffset};

/// Avatar background colours (0xRRGGBB), chosen to stay readable with white initials.
const AVATAR_COLORS: [u32; 8] = [
    0x2f6fdb, 0x16a085, 0x8e44ad, 0xd35400, 0xc0392b, 0x2c3e50, 0x7f8c00, 0xb03a8c,
];

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_LONG: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const WEEKDAYS_LONG: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// Formats timestamps in the user's time zone, relative to "now".
#[derive(Debug, Clone, Copy)]
pub struct Clock {
    pub now: Timestamp,
    pub offset: UtcOffset,
}

impl Clock {
    pub fn new(now: Timestamp, utc_offset_seconds: i32) -> Self {
        Self {
            now,
            offset: UtcOffset::from_whole_seconds(utc_offset_seconds).unwrap_or(UtcOffset::UTC),
        }
    }

    fn local(&self, ts: Timestamp) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(ts)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH)
            .to_offset(self.offset)
    }

    fn today(&self) -> Date {
        self.local(self.now).date()
    }

    /// "14:05", "Yesterday", "Mon", "19 Oct" or "19 Oct 2025", for thread lists.
    pub fn list_label(&self, ts: Timestamp) -> String {
        let dt = self.local(ts);
        let days = (self.today() - dt.date()).whole_days();
        match days {
            i64::MIN..=0 => self.time_label(ts),
            1 => "Yesterday".to_string(),
            2..=6 => WEEKDAYS[dt.weekday().number_days_from_monday() as usize].to_string(),
            _ if dt.year() == self.today().year() => {
                format!(
                    "{} {}",
                    dt.day(),
                    MONTHS[usize::from(u8::from(dt.month())) - 1]
                )
            }
            _ => format!(
                "{} {} {}",
                dt.day(),
                MONTHS[usize::from(u8::from(dt.month())) - 1],
                dt.year()
            ),
        }
    }

    /// "14:05", for chat bubbles.
    pub fn time_label(&self, ts: Timestamp) -> String {
        let dt = self.local(ts);
        format!("{:02}:{:02}", dt.hour(), dt.minute())
    }

    /// "Today", "Yesterday", "Monday 19 October" or "19 October 2025", for day separators.
    pub fn day_label(&self, ts: Timestamp) -> String {
        let dt = self.local(ts);
        let days = (self.today() - dt.date()).whole_days();
        let month = MONTHS_LONG[usize::from(u8::from(dt.month())) - 1];
        match days {
            i64::MIN..=0 => "Today".to_string(),
            1 => "Yesterday".to_string(),
            _ if dt.year() == self.today().year() => format!(
                "{} {} {}",
                WEEKDAYS_LONG[dt.weekday().number_days_from_monday() as usize],
                dt.day(),
                month
            ),
            _ => format!("{} {} {}", dt.day(), month, dt.year()),
        }
    }

    /// Calendar day key used to place day separators.
    pub fn day_key(&self, ts: Timestamp) -> i32 {
        self.local(ts).date().to_julian_day()
    }

    /// "Mon, 19 Oct 2026 at 14:05", for the attribution line of quoted replies.
    pub fn attribution_label(&self, ts: Timestamp) -> String {
        let dt = self.local(ts);
        format!(
            "{}, {} {} {} at {}",
            WEEKDAYS[dt.weekday().number_days_from_monday() as usize],
            dt.day(),
            MONTHS[usize::from(u8::from(dt.month())) - 1],
            dt.year(),
            self.time_label(ts)
        )
    }
}

/// Up to two initials from a name, or the first letter of the address.
pub fn initials(address: &Address) -> String {
    let source = address.display();
    let words: Vec<&str> = source
        .split(|c: char| c.is_whitespace() || c == '.' || c == '_' || c == '-')
        .filter(|w| w.chars().next().is_some_and(char::is_alphanumeric))
        .collect();
    let letters: String = match words.as_slice() {
        [] => address.email.chars().take(1).collect(),
        [one] => one.chars().take(1).collect(),
        [first, .., last] => first.chars().take(1).chain(last.chars().take(1)).collect(),
    };
    letters.to_uppercase()
}

/// A stable avatar colour for an email address.
pub fn avatar_color(email: &str) -> u32 {
    // FNV-1a: stable across runs and platforms, unlike the std hasher.
    let mut hash: u32 = 0x811c_9dc5;
    for byte in email.to_lowercase().bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    AVATAR_COLORS[(hash % AVATAR_COLORS.len() as u32) as usize]
}

/// "Ada", "Ada and Grace", or "Ada, Grace and 3 others".
pub fn names_label(names: &[String]) -> String {
    match names {
        [] => "Me".to_string(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, rest @ ..] if rest.len() == 1 => format!("{a}, {b} and {}", rest[0]),
        [a, b, rest @ ..] => format!("{a}, {b} and {} others", rest.len()),
    }
}

/// "12 KB", "3.4 MB".
pub fn size_label(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.0} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else {
        format!("{:.1} GB", b / (KB * KB * KB))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Monday 19 October 2026, 15:00 UTC.
    const NOW: Timestamp = 1_792_422_000;

    #[test]
    fn list_labels_are_relative() {
        let clock = Clock::new(NOW, 0);
        assert_eq!(clock.list_label(NOW - 3_600), "14:00");
        assert_eq!(clock.list_label(NOW - 86_400), "Yesterday");
        assert_eq!(clock.list_label(NOW - 3 * 86_400), "Fri");
        assert_eq!(clock.list_label(NOW - 30 * 86_400), "19 Sep");
        assert_eq!(clock.list_label(NOW - 400 * 86_400), "14 Sep 2025");
    }

    #[test]
    fn labels_follow_the_users_time_zone() {
        // 23:30 UTC on Sunday is already Monday in Berlin (UTC+2 in October).
        let sunday_night = NOW - 15 * 3_600 - 1_800;
        assert_eq!(Clock::new(NOW, 0).day_label(sunday_night), "Yesterday");
        assert_eq!(Clock::new(NOW, 7_200).day_label(sunday_night), "Today");
        assert_eq!(Clock::new(NOW, 7_200).time_label(sunday_night), "01:30");
    }

    #[test]
    fn day_and_attribution_labels() {
        let clock = Clock::new(NOW, 0);
        assert_eq!(clock.day_label(NOW - 2 * 86_400), "Saturday 17 October");
        assert_eq!(clock.attribution_label(NOW), "Mon, 19 Oct 2026 at 15:00");
    }

    #[test]
    fn initials_and_colors() {
        assert_eq!(
            initials(&Address::new(Some("Ada Lovelace"), "a@x.org")),
            "AL"
        );
        assert_eq!(
            initials(&Address::new(Some("Grace B. Hopper"), "g@x.org")),
            "GH"
        );
        assert_eq!(initials(&Address::new(None, "bob.smith@x.org")), "BS");
        assert_eq!(initials(&Address::new(Some("李雷"), "l@x.org")), "李");
        assert_eq!(avatar_color("Ada@x.org"), avatar_color("ada@x.org"));
    }

    #[test]
    fn names_and_sizes() {
        assert_eq!(
            names_label(&["Ada".into(), "Grace".into()]),
            "Ada and Grace"
        );
        assert_eq!(
            names_label(&["A".into(), "B".into(), "C".into(), "D".into()]),
            "A, B and 2 others"
        );
        assert_eq!(size_label(512), "512 B");
        assert_eq!(size_label(2_500_000), "2.4 MB");
    }
}
