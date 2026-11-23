//! Date formatting with moment.js tokens (the syntax Obsidian's daily notes and templates use,
//! e.g. `DD-MM-YY`, `YYYY-MM-DD`, `dddd, MMMM Do`, `HH:mm`). English names, as moment's
//! default locale. Text in `[brackets]` is literal.

use chrono::{Datelike, NaiveDateTime, Timelike};

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// Tokens, longest first so `YYYY` wins over `YY`.
const TOKENS: &[&str] = &[
    "YYYY", "YY", "Q", "MMMM", "MMM", "MM", "M", "DDDD", "DDD", "Do", "DD", "D", "dddd", "ddd", "dd", "d", "E", "e", "WW", "W", "ww", "w", "GGGG", "gggg", "HH", "H", "hh", "h", "kk", "k",
    "mm", "m", "ss", "s", "SSS", "A", "a", "X", "x",
];

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

fn token(t: &str, d: &NaiveDateTime) -> String {
    let wd = d.weekday().num_days_from_sunday() as usize;
    let h12 = match d.hour() % 12 {
        0 => 12,
        h => h,
    };
    match t {
        "YYYY" => format!("{:04}", d.year()),
        "YY" => format!("{:02}", d.year().rem_euclid(100)),
        "Q" => ((d.month() - 1) / 3 + 1).to_string(),
        "MMMM" => MONTHS[d.month0() as usize].to_string(),
        "MMM" => MONTHS[d.month0() as usize][..3].to_string(),
        "MM" => format!("{:02}", d.month()),
        "M" => d.month().to_string(),
        "DDDD" => format!("{:03}", d.ordinal()),
        "DDD" => d.ordinal().to_string(),
        "Do" => ordinal(d.day()),
        "DD" => format!("{:02}", d.day()),
        "D" => d.day().to_string(),
        "dddd" => DAYS[wd].to_string(),
        "ddd" => DAYS[wd][..3].to_string(),
        "dd" => DAYS[wd][..2].to_string(),
        "d" | "e" => wd.to_string(),
        "E" => d.weekday().number_from_monday().to_string(),
        // ISO week (moment's W) and locale week (w; en: weeks start on Sunday)
        "WW" | "ww" => format!("{:02}", if t == "WW" { d.iso_week().week() } else { (d.ordinal() + 6 - d.weekday().num_days_from_sunday()) / 7 + 1 }),
        "W" | "w" => (if t == "W" { d.iso_week().week() } else { (d.ordinal() + 6 - d.weekday().num_days_from_sunday()) / 7 + 1 }).to_string(),
        "GGGG" | "gggg" => format!("{:04}", d.iso_week().year()),
        "HH" => format!("{:02}", d.hour()),
        "H" => d.hour().to_string(),
        "hh" => format!("{h12:02}"),
        "h" => h12.to_string(),
        "kk" => format!("{:02}", if d.hour() == 0 { 24 } else { d.hour() }),
        "k" => (if d.hour() == 0 { 24 } else { d.hour() }).to_string(),
        "mm" => format!("{:02}", d.minute()),
        "m" => d.minute().to_string(),
        "ss" => format!("{:02}", d.second()),
        "s" => d.second().to_string(),
        "SSS" => format!("{:03}", d.nanosecond() / 1_000_000),
        "A" => (if d.hour() < 12 { "AM" } else { "PM" }).to_string(),
        "a" => (if d.hour() < 12 { "am" } else { "pm" }).to_string(),
        "X" => d.and_utc().timestamp().to_string(),
        "x" => d.and_utc().timestamp_millis().to_string(),
        _ => t.to_string(),
    }
}

/// Formats `d` with a moment.js format string.
pub fn format(fmt: &str, d: &NaiveDateTime) -> String {
    let mut out = String::new();
    let mut rest = fmt;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('[') {
            let end = r.find(']').unwrap_or(r.len());
            out.push_str(&r[..end]);
            rest = r.get(end + 1..).unwrap_or("");
            continue;
        }
        match TOKENS.iter().find(|t| rest.starts_with(**t)) {
            Some(t) => {
                out.push_str(&token(t, d));
                rest = &rest[t.len()..];
            }
            None => {
                let c = rest.chars().next().expect("non-empty");
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// The local date and time now.
pub fn now() -> NaiveDateTime {
    chrono::Local::now().naive_local()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 5).unwrap()
    }

    #[test]
    fn obsidian_formats() {
        let d = at(2026, 11, 3, 14, 7);
        assert_eq!(format("DD-MM-YY", &d), "03-11-26");
        assert_eq!(format("YYYY-MM-DD", &d), "2026-11-03");
        assert_eq!(format("dddd, MMMM Do YYYY", &d), "Tuesday, November 3rd 2026");
        assert_eq!(format("HH:mm", &d), "14:07");
        assert_eq!(format("h:mm A", &d), "2:07 PM");
        assert_eq!(format("[Week] W, YYYY", &d), "Week 45, 2026");
        assert_eq!(format("D/M ddd", &at(2026, 1, 11, 0, 0)), "11/1 Sun");
        assert_eq!(format("Do", &at(2026, 1, 12, 0, 0)), "12th");
        assert_eq!(format("Do", &at(2026, 1, 22, 0, 0)), "22nd");
    }
}
