//! Listening statistics, kept in the chanceify folder.
//!
//! Every song that counts as played (see [`crate::history::counts_after`]) is
//! written down here with the time, so the Home page can say what was heard
//! this week, this month and ever. Spotify's own history only reaches back a
//! few dozen plays; this file keeps tens of thousands. It never leaves the
//! computer.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The most plays kept; the oldest go first.
const KEPT: usize = 40_000;

const DAY: i64 = 86_400;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Play {
    /// Seconds since 1970.
    pub t: i64,
    pub uri: String,
    pub title: String,
    pub artist: String,
    /// The artist's Spotify id, when it was known, so the name can be opened.
    #[serde(default)]
    pub artist_id: String,
    /// The song's length, so hours listened can be added up.
    pub ms: u32,
    /// The song's album, when it was known, so a new release can tell it
    /// has been heard.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub album_id: String,
}

#[derive(Default)]
pub struct Stats {
    /// Oldest first.
    plays: Vec<Play>,
    dirty: bool,
}

/// One row of a ranking.
#[derive(Clone, Debug, PartialEq)]
pub struct Ranked {
    /// The artist's name, or the song's title.
    pub name: String,
    /// For a song, its artist; empty for an artist.
    pub sub: String,
    pub plays: u32,
    /// For a song, its link.
    pub uri: String,
    pub artist_id: String,
}

/// How far back a ranking looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Week,
    Month,
    Ever,
}

impl Range {
    /// From the saved number: 0 last 30 days, 1 last 7 days, 2 all time.
    pub fn from_saved(value: u8) -> Self {
        match value {
            1 => Self::Week,
            2 => Self::Ever,
            _ => Self::Month,
        }
    }

    pub fn saved(self) -> u8 {
        match self {
            Self::Month => 0,
            Self::Week => 1,
            Self::Ever => 2,
        }
    }

    fn days(self) -> Option<i64> {
        match self {
            Self::Week => Some(7),
            Self::Month => Some(30),
            Self::Ever => None,
        }
    }
}

/// A stretch of the reader's days, as day numbers counted from 1970-01-01
/// in the reader's time: `first` is in it, `end` is the first day after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Days {
    pub first: i64,
    pub end: i64,
}

impl Days {
    /// The days `range` looks back over, up to and including today.
    pub fn last(range: Range, now: i64, offset: i64) -> Self {
        let today = (now + offset).div_euclid(DAY);
        Self {
            first: range.days().map_or(i64::MIN, |days| today - days + 1),
            end: today + 1,
        }
    }

    /// Every day of one calendar month.
    pub fn month(month: Month) -> Self {
        let next = month.next();
        Self {
            first: days_from_civil(month.year, month.month),
            end: days_from_civil(next.year, next.month),
        }
    }

    fn holds(self, t: i64, offset: i64) -> bool {
        let day = (t + offset).div_euclid(DAY);
        self.first <= day && day < self.end
    }
}

/// A calendar month, January being 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Month {
    pub year: i32,
    pub month: u8,
}

const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];

impl Month {
    /// From the saved number, year * 100 + month (202610); 0 is none.
    pub fn from_saved(value: u32) -> Option<Self> {
        let month = (value % 100) as u8;
        (value != 0 && (1..=12).contains(&month)).then(|| Self {
            year: (value / 100) as i32,
            month,
        })
    }

    pub fn saved(self) -> u32 {
        self.year.max(0) as u32 * 100 + u32::from(self.month)
    }

    /// "October 2026".
    pub fn label(self) -> String {
        let index = usize::from(self.month.clamp(1, 12) - 1);
        format!("{} {}", MONTH_NAMES[index], self.year)
    }

    fn next(self) -> Self {
        if self.month >= 12 {
            Self {
                year: self.year + 1,
                month: 1,
            }
        } else {
            Self {
                year: self.year,
                month: self.month + 1,
            }
        }
    }

    fn of_day(day: i64) -> Self {
        let (year, month) = civil_from_days(day);
        Self { year, month }
    }
}

/// The year and month of a day number (days since 1970-01-01), after
/// Howard Hinnant's `civil_from_days`.
fn civil_from_days(day: i64) -> (i32, u8) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year as i32, month as u8)
}

/// The day number of the first of a month, after `days_from_civil`.
fn days_from_civil(year: i32, month: u8) -> i64 {
    let month = i64::from(month);
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * shifted + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub week: u32,
    pub month: u32,
    pub ever: u32,
    pub minutes_week: u64,
    pub minutes_month: u64,
    pub minutes_ever: u64,
    /// Plays on each of the last 14 days, the oldest first.
    pub per_day: [u32; 14],
    /// The hour of the day (0 to 23) with the most plays this month.
    pub busiest_hour: Option<u8>,
    /// How many days in a row, up to today, had a play.
    pub streak_days: u32,
}

impl Stats {
    pub fn load(path: &Path) -> Self {
        let plays = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<Play>>(&text).ok())
            .unwrap_or_default();
        Self {
            plays,
            dirty: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.plays.is_empty()
    }

    pub fn len(&self) -> usize {
        self.plays.len()
    }

    pub fn record(&mut self, play: Play) {
        self.plays.push(play);
        if self.plays.len() > KEPT {
            let extra = self.plays.len() - KEPT;
            self.plays.drain(..extra);
        }
        self.dirty = true;
    }

    /// When each song was last heard (seconds since 1970).
    pub fn last_played(&self) -> std::collections::HashMap<&str, i64> {
        let mut last: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
        for play in &self.plays {
            let entry = last.entry(play.uri.as_str()).or_insert(play.t);
            if play.t > *entry {
                *entry = play.t;
            }
        }
        last
    }

    pub fn clear(&mut self) {
        if !self.plays.is_empty() {
            self.plays.clear();
            self.dirty = true;
        }
    }

    pub fn save(&mut self, path: &Path) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        crate::util::save_json_in_background(self.plays.clone(), path.to_path_buf());
    }

    /// The artists and the songs heard most within `range`, most first.
    pub fn ranking(&self, now: i64, offset: i64, range: Range) -> (Vec<Ranked>, Vec<Ranked>) {
        self.ranking_in(offset, Days::last(range, now, offset))
    }

    /// How many songs were heard within `days`, and for how many minutes.
    pub fn totals_in(&self, offset: i64, days: Days) -> (u32, u64) {
        self.plays
            .iter()
            .filter(|play| days.holds(play.t, offset))
            .fold((0, 0), |(plays, minutes), play| {
                (plays + 1, minutes + u64::from(play.ms) / 60_000)
            })
    }

    /// The months that have a play, the newest first.
    pub fn months(&self, offset: i64) -> Vec<Month> {
        let mut months: Vec<Month> = Vec::new();
        for play in &self.plays {
            let month = Month::of_day((play.t + offset).div_euclid(DAY));
            if months.last() != Some(&month) && !months.contains(&month) {
                months.push(month);
            }
        }
        months.sort_by(|a, b| b.cmp(a));
        months
    }

    /// The albums of the songs heard since `since` (seconds since 1970).
    pub fn albums_heard_since(&self, since: i64) -> std::collections::HashSet<&str> {
        self.plays
            .iter()
            .rev()
            .take_while(|play| play.t >= since)
            .filter(|play| !play.album_id.is_empty())
            .map(|play| play.album_id.as_str())
            .collect()
    }

    /// The artists and the songs heard most within `days`, most first.
    pub fn ranking_in(&self, offset: i64, days: Days) -> (Vec<Ranked>, Vec<Ranked>) {
        let mut artists: HashMap<&str, Ranked> = HashMap::new();
        let mut songs: HashMap<String, Ranked> = HashMap::new();
        for play in &self.plays {
            if !days.holds(play.t, offset) {
                continue;
            }
            if !play.artist.is_empty() {
                let entry = artists.entry(play.artist.as_str()).or_insert_with(|| Ranked {
                    name: play.artist.clone(),
                    sub: String::new(),
                    plays: 0,
                    uri: String::new(),
                    artist_id: String::new(),
                });
                entry.plays += 1;
                if entry.artist_id.is_empty() {
                    entry.artist_id = play.artist_id.clone();
                }
            }
            let key = if play.uri.is_empty() {
                format!("{}\u{1}{}", play.title, play.artist)
            } else {
                play.uri.clone()
            };
            let entry = songs.entry(key).or_insert_with(|| Ranked {
                name: play.title.clone(),
                sub: play.artist.clone(),
                plays: 0,
                uri: play.uri.clone(),
                artist_id: play.artist_id.clone(),
            });
            entry.plays += 1;
        }
        let sort = |mut rows: Vec<Ranked>| {
            rows.sort_by(|a, b| b.plays.cmp(&a.plays).then_with(|| a.name.cmp(&b.name)));
            rows.truncate(50);
            rows
        };
        (sort(artists.into_values().collect()), sort(songs.into_values().collect()))
    }

    /// What was heard, as of `now` (seconds since 1970) in a place `offset`
    /// seconds from UTC, so that "today" is the reader's today.
    pub fn summary(&self, now: i64, offset: i64) -> Summary {
        let today = (now + offset).div_euclid(DAY);
        let mut summary = Summary::default();
        let mut hours = [0u32; 24];
        let mut days_with_play = std::collections::BTreeSet::new();
        for play in &self.plays {
            let day = (play.t + offset).div_euclid(DAY);
            let age = today - day;
            if age < 0 {
                continue;
            }
            let minutes = u64::from(play.ms) / 60_000;
            summary.ever += 1;
            summary.minutes_ever += minutes;
            days_with_play.insert(day);
            if age < 30 {
                summary.month += 1;
                summary.minutes_month += minutes;
                let hour = (play.t + offset).rem_euclid(DAY) / 3600;
                hours[hour as usize] += 1;
            }
            if age < 7 {
                summary.week += 1;
                summary.minutes_week += minutes;
            }
            if age < 14 {
                summary.per_day[(13 - age) as usize] += 1;
            }
        }
        let best = hours.iter().copied().max().unwrap_or(0);
        if best > 0 {
            summary.busiest_hour = hours.iter().position(|count| *count == best).map(|h| h as u8);
        }
        // Today may not have a play yet; the streak then counts from yesterday.
        let mut day = if days_with_play.contains(&today) {
            today
        } else {
            today - 1
        };
        while days_with_play.contains(&day) {
            summary.streak_days += 1;
            day -= 1;
        }
        summary
    }
}

/// How far the computer's clock is from UTC right now, in seconds.
pub fn local_offset() -> i64 {
    i64::from(jiff::Zoned::now().offset().seconds())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(t: i64, title: &str, artist: &str) -> Play {
        Play {
            t,
            uri: format!("spotify:track:{title}"),
            title: title.into(),
            artist: artist.into(),
            artist_id: String::new(),
            ms: 180_000,
            album_id: String::new(),
        }
    }

    #[test]
    fn days_and_months_line_up() {
        assert_eq!(days_from_civil(1970, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1));
        assert_eq!(civil_from_days(days_from_civil(2026, 10)), (2026, 10));
        assert_eq!(civil_from_days(days_from_civil(2024, 3) - 1), (2024, 2));
        let october = Month { year: 2026, month: 10 };
        assert_eq!(Month::from_saved(october.saved()), Some(october));
        assert_eq!(Month::from_saved(0), None);
        assert_eq!(october.label(), "October 2026");
        let december = Days::month(Month { year: 2025, month: 12 });
        assert_eq!(december.end - december.first, 31);
    }

    #[test]
    fn a_month_counts_only_its_own_plays() {
        let october = days_from_civil(2026, 10) * DAY;
        let mut stats = Stats::default();
        stats.record(play(october - 3600, "a", "X"));
        stats.record(play(october + 3600, "b", "Y"));
        stats.record(play(october + 20 * DAY, "b", "Y"));
        let days = Days::month(Month { year: 2026, month: 10 });
        assert_eq!(stats.totals_in(0, days), (2, 6));
        let (artists, _) = stats.ranking_in(0, days);
        assert_eq!((artists[0].name.as_str(), artists.len()), ("Y", 1));
        assert_eq!(
            stats.months(0),
            [Month { year: 2026, month: 10 }, Month { year: 2026, month: 9 }]
        );
        // Two hours west, the first play of October is still in September.
        assert_eq!(stats.totals_in(-2 * 3600, days).0, 1);
    }

    #[test]
    fn heard_albums_come_from_recent_plays() {
        let mut stats = Stats::default();
        let mut old = play(10, "a", "X");
        old.album_id = "old".into();
        let mut new = play(100, "b", "X");
        new.album_id = "new".into();
        stats.record(old);
        stats.record(new);
        stats.record(play(200, "c", "X"));
        let heard = stats.albums_heard_since(50);
        assert!(heard.contains("new") && !heard.contains("old"));
    }

    #[test]
    fn plays_are_counted_by_the_readers_days() {
        let now = 100 * DAY + 12 * 3600;
        let mut stats = Stats::default();
        stats.record(play(now - 100, "a", "X"));
        stats.record(play(now - 2 * DAY, "a", "X"));
        stats.record(play(now - 10 * DAY, "b", "Y"));
        stats.record(play(now - 60 * DAY, "c", "Y"));
        let summary = stats.summary(now, 0);
        assert_eq!(summary.ever, 4);
        assert_eq!(summary.month, 3);
        assert_eq!(summary.week, 2);
        assert_eq!(summary.minutes_week, 6);
        assert_eq!(summary.per_day[13], 1);
        assert_eq!(summary.per_day[11], 1);
        assert_eq!(summary.per_day[3], 1);
        let (artists, songs) = stats.ranking(now, 0, Range::Month);
        assert_eq!((artists[0].name.as_str(), artists[0].plays), ("X", 2));
        assert_eq!((songs[0].name.as_str(), songs[0].sub.as_str()), ("a", "X"));
        assert_eq!(stats.ranking(now, 0, Range::Week).0.len(), 1);
        assert_eq!(stats.ranking(now, 0, Range::Ever).0.len(), 2);
    }

    #[test]
    fn the_streak_counts_back_from_today_or_yesterday() {
        let now = 100 * DAY + 3600;
        let mut stats = Stats::default();
        for back in [1, 2, 3, 5] {
            stats.record(play(now - back * DAY, "a", "X"));
        }
        assert_eq!(stats.summary(now, 0).streak_days, 3);
        stats.record(play(now, "a", "X"));
        assert_eq!(stats.summary(now, 0).streak_days, 4);
    }

    #[test]
    fn the_offset_moves_the_day_boundary() {
        // 23:30 UTC is already tomorrow three hours east.
        let now = 100 * DAY + 23 * 3600 + 1800;
        let mut stats = Stats::default();
        stats.record(play(now, "a", "X"));
        let east = stats.summary(now + 3600 * 2, 3 * 3600);
        assert_eq!(east.per_day[13], 1);
        assert_eq!(east.busiest_hour, Some(2));
    }

    #[test]
    fn only_the_newest_plays_are_kept() {
        let mut stats = Stats::default();
        for index in 0..(KEPT as i64 + 5) {
            stats.record(play(index, "a", "X"));
        }
        assert_eq!(stats.len(), KEPT);
    }

    #[test]
    fn saved_plays_come_back() {
        let dir = std::env::temp_dir().join(format!("chanceify-stats-{}", std::process::id()));
        let path = dir.join("stats.json");
        let mut stats = Stats::default();
        stats.record(play(5, "a", "X"));
        stats.save(&path);
        assert_eq!(Stats::load(&path).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
