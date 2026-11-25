//! The file list's orders: Obsidian's file explorer "Change sort order" menu. Folders always
//! come first; they follow the name order (A to Z, or Z to A when that is the order) and stay
//! A to Z under the time orders, as in Obsidian. Names compare "alphanumerically": numbers by
//! value (`Lecture 2` before `Lecture 10`), ignoring case and accents (`è` sorts with `e`).

use std::cmp::Ordering;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortOrder {
    /// File name, A to Z.
    #[default]
    Alphabetical,
    /// File name, Z to A.
    AlphabeticalReverse,
    /// Modified time, new to old.
    ByModifiedTime,
    /// Modified time, old to new.
    ByModifiedTimeReverse,
    /// Created time, new to old.
    ByCreatedTime,
    /// Created time, old to new.
    ByCreatedTimeReverse,
}

impl SortOrder {
    /// In the order of Obsidian's menu.
    pub const ALL: [SortOrder; 6] = [Self::Alphabetical, Self::AlphabeticalReverse, Self::ByModifiedTime, Self::ByModifiedTimeReverse, Self::ByCreatedTime, Self::ByCreatedTimeReverse];

    /// Obsidian's name for the order (`sortOrder` in `.obsidian/workspace.json`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Alphabetical => "alphabetical",
            Self::AlphabeticalReverse => "alphabeticalReverse",
            Self::ByModifiedTime => "byModifiedTime",
            Self::ByModifiedTimeReverse => "byModifiedTimeReverse",
            Self::ByCreatedTime => "byCreatedTime",
            Self::ByCreatedTimeReverse => "byCreatedTimeReverse",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|o| o.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Alphabetical => "File name (A to Z)",
            Self::AlphabeticalReverse => "File name (Z to A)",
            Self::ByModifiedTime => "Modified time (new to old)",
            Self::ByModifiedTimeReverse => "Modified time (old to new)",
            Self::ByCreatedTime => "Created time (new to old)",
            Self::ByCreatedTimeReverse => "Created time (old to new)",
        }
    }

    /// Whether the order needs the files' times (a save can move a file).
    pub fn uses_times(self) -> bool {
        !matches!(self, Self::Alphabetical | Self::AlphabeticalReverse)
    }
}

/// A file's times; None where the filesystem doesn't record them (sorted as oldest).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileTimes {
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
}

impl FileTimes {
    pub fn of(path: &std::path::Path) -> Self {
        match std::fs::metadata(path) {
            Ok(m) => Self { modified: m.modified().ok(), created: m.created().ok() },
            Err(_) => Self::default(),
        }
    }
}

/// `c` lowercased, without its accent (Latin letters), for comparing names.
fn base(c: char) -> char {
    let l = c.to_lowercase().next().unwrap_or(c);
    if l.is_ascii() {
        return l;
    }
    const MAP: &[(&str, char)] = &[
        ("àáâãäåāăąǎ", 'a'),
        ("çćĉċč", 'c'),
        ("ďđ", 'd'),
        ("èéêëēĕėęě", 'e'),
        ("ĝğġģ", 'g'),
        ("ĥħ", 'h'),
        ("ìíîïĩīĭįıǐ", 'i'),
        ("ĵ", 'j'),
        ("ķ", 'k'),
        ("ĺļľŀł", 'l'),
        ("ñńņňŉ", 'n'),
        ("òóôõöøōŏőǒ", 'o'),
        ("ŕŗř", 'r'),
        ("śŝşšș", 's'),
        ("ţťŧț", 't'),
        ("ùúûüũūŭůűųǔ", 'u'),
        ("ŵ", 'w'),
        ("ýÿŷ", 'y'),
        ("źżž", 'z'),
    ];
    MAP.iter().find(|(set, _)| set.contains(l)).map_or(l, |(_, b)| *b)
}

/// Compares two names alphanumerically: digit runs by value, other characters ignoring case
/// and accents; names equal that way fall back to the plain characters (so the order is total
/// and stable).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => break,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let run = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (m, n) = (run(&mut x), run(&mut y));
                let (mt, nt) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let o = mt.len().cmp(&nt.len()).then_with(|| mt.cmp(nt));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = base(c).cmp(&base(d));
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
    // equal ignoring case, accents and leading zeros: lowercase first, then the raw text
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

/// Sorts folder names (one level) for `order`.
pub fn sort_folders(names: &mut [String], order: SortOrder) {
    names.sort_by(|a, b| natural_cmp(a, b));
    if order == SortOrder::AlphabeticalReverse {
        names.reverse();
    }
}

/// Sorts files (one level) for `order`. `name` gives the name shown for a file (notes without
/// `.md`), `times` its times (only asked for the time orders). Ties fall back to A to Z.
pub fn sort_files<T>(files: &mut [T], order: SortOrder, name: impl Fn(&T) -> &str, times: impl Fn(&T) -> FileTimes) {
    let by_name = |a: &T, b: &T| natural_cmp(name(a), name(b));
    match order {
        SortOrder::Alphabetical => files.sort_by(by_name),
        SortOrder::AlphabeticalReverse => files.sort_by(|a, b| by_name(b, a)),
        _ => {
            let key = |t: FileTimes| match order {
                SortOrder::ByModifiedTime | SortOrder::ByModifiedTimeReverse => t.modified,
                _ => t.created.or(t.modified),
            };
            let newest_first = matches!(order, SortOrder::ByModifiedTime | SortOrder::ByCreatedTime);
            let mut keyed: Vec<(Option<SystemTime>, usize)> = files.iter().enumerate().map(|(i, f)| (key(times(f)), i)).collect();
            keyed.sort_by(|(ta, ia), (tb, ib)| {
                let o = if newest_first { tb.cmp(ta) } else { ta.cmp(tb) };
                o.then_with(|| by_name(&files[*ia], &files[*ib]))
            });
            let order: Vec<usize> = keyed.into_iter().map(|(_, i)| i).collect();
            apply_permutation(files, &order);
        }
    }
}

/// Reorders `v` so that `v[k]` becomes the old `v[order[k]]`.
fn apply_permutation<T>(v: &mut [T], order: &[usize]) {
    let mut pos: Vec<usize> = (0..v.len()).collect(); // where each original element is now
    let mut at: Vec<usize> = (0..v.len()).collect(); // which original element is at each slot
    for (k, &want) in order.iter().enumerate() {
        let from = pos[want];
        if from != k {
            v.swap(k, from);
            let displaced = at[k];
            at[k] = want;
            at[from] = displaced;
            pos[displaced] = from;
            pos[want] = k;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn sorted(names: &[&str], order: SortOrder) -> Vec<String> {
        let mut v: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        sort_files(&mut v, order, |s| s.as_str(), |_| FileTimes::default());
        v
    }

    #[test]
    fn alphanumeric_by_value_case_and_accents() {
        let names = ["Lecture 10", "lecture 2", "Lecture 1", "Zeta", "Élite", "elite", "Über", "Uber 2", "a", "B"];
        assert_eq!(sorted(&names, SortOrder::Alphabetical), ["a", "B", "elite", "Élite", "Lecture 1", "lecture 2", "Lecture 10", "Über", "Uber 2", "Zeta"]);
        assert_eq!(sorted(&names, SortOrder::AlphabeticalReverse), ["Zeta", "Uber 2", "Über", "Lecture 10", "lecture 2", "Lecture 1", "Élite", "elite", "B", "a"]);
        assert_eq!(natural_cmp("x 007", "x 7"), Ordering::Less); // equal value: still a total order
        assert_eq!(natural_cmp("x 7", "x 7"), Ordering::Equal);
        assert_eq!(natural_cmp("a9", "a10b"), Ordering::Less);
        assert_eq!(natural_cmp("99999999999999999999999", "100000000000000000000000"), Ordering::Less); // no overflow
    }

    #[test]
    fn time_orders_with_name_ties_and_missing_times() {
        let t = |s: u64| Some(UNIX_EPOCH + Duration::from_secs(s));
        // (name, modified, created)
        let files = [("b", t(30), t(1)), ("a", t(30), t(3)), ("c", t(10), t(2)), ("d", None, None)];
        let run = |order| {
            let mut v = files.to_vec();
            sort_files(&mut v, order, |f| f.0, |f| FileTimes { modified: f.1, created: f.2 });
            v.into_iter().map(|f| f.0).collect::<Vec<_>>()
        };
        assert_eq!(run(SortOrder::ByModifiedTime), ["a", "b", "c", "d"]);
        assert_eq!(run(SortOrder::ByModifiedTimeReverse), ["d", "c", "a", "b"]);
        assert_eq!(run(SortOrder::ByCreatedTime), ["a", "c", "b", "d"]);
        assert_eq!(run(SortOrder::ByCreatedTimeReverse), ["d", "b", "c", "a"]);
    }

    #[test]
    fn created_falls_back_to_modified() {
        let t = |s: u64| Some(UNIX_EPOCH + Duration::from_secs(s));
        let mut v = vec![("old", t(1), None), ("new", t(9), None)];
        sort_files(&mut v, SortOrder::ByCreatedTime, |f| f.0, |f| FileTimes { modified: f.1, created: f.2 });
        assert_eq!(v[0].0, "new");
    }

    #[test]
    fn folders_follow_name_order_only() {
        let mut d = vec!["Beta".to_string(), "alpha 10".into(), "Alpha 9".into()];
        sort_folders(&mut d, SortOrder::ByModifiedTime);
        assert_eq!(d, ["Alpha 9", "alpha 10", "Beta"]);
        sort_folders(&mut d, SortOrder::AlphabeticalReverse);
        assert_eq!(d, ["Beta", "alpha 10", "Alpha 9"]);
    }

    #[test]
    fn ids_round_trip() {
        for o in SortOrder::ALL {
            assert_eq!(SortOrder::from_id(o.id()), Some(o));
        }
        assert_eq!(SortOrder::from_id("frequency"), None);
    }

    #[test]
    fn permutation_is_applied_exactly() {
        let mut v = vec!['a', 'b', 'c', 'd', 'e'];
        apply_permutation(&mut v, &[3, 0, 4, 1, 2]);
        assert_eq!(v, ['d', 'a', 'e', 'b', 'c']);
    }
}
