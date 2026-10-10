//! What this build is: the crate version, the revision it was built from, and
//! when. Local builds carry a revision the released binaries do not, so a
//! window can always say which one is on screen.
//!
//! The values come from `build.rs`, which reads Git and the build clock. They
//! are baked in at compile time and never change while the process runs.

/// The revision this build came from, `dirty` when the tree had uncommitted
/// changes, or `unknown` outside a Git checkout.
pub const REVISION: &str = env!("CHANCEIFY_REVISION");

/// What this build calls itself on screen: the window title, the tray, the
/// About box.
///
/// A local build is not the released Chanceify, so it should not present
/// itself as one. Deliberately only the display name: the profile directory,
/// the updater slug, the single-instance lock, the `spotify:` link handler and
/// the HTTP user agent all keep their upstream names, so an existing sign-in
/// and install keep working and this build still updates and hands off links
/// correctly.
pub const DISPLAY_NAME: &str = "chanceify™";

/// Chance's GitHub, linked from Settings and the sign-in page. One place to
/// change it.
pub const GITHUB_URL: &str = "https://github.com/madlygeeked/chanceify";

/// Seconds since the Unix epoch when this build was compiled, from
/// `SOURCE_DATE_EPOCH` when the build asked to be reproducible.
const BUILD_EPOCH: &str = env!("CHANCEIFY_BUILD_EPOCH");

/// The crate version, for example `0.11.2`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Whether this build came from a working tree with uncommitted changes.
pub fn is_dirty() -> bool {
    REVISION.ends_with("+dirty")
}

/// The revision without its `dirty` marker.
pub fn revision() -> &'static str {
    REVISION.split('+').next().unwrap_or(REVISION)
}

/// Seconds since the Unix epoch to `YYYY-MM-DD HH:MM` in UTC.
///
/// Spelled out because a build script has no date crate to lean on, and the
/// arithmetic is short enough to keep here where the tests can reach it.
/// Days-to-civil is Howard Hinnant's algorithm: it shifts the year to start in
/// March so the leap day lands last and drops out of the month lengths.
fn format_epoch(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let (hour, minute) = (time / 3_600, (time % 3_600) / 60);
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = shifted_month + if shifted_month < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// The date and time this build was compiled, or `None` when
/// `SOURCE_DATE_EPOCH` was not a number this build understood.
pub fn built_at() -> Option<String> {
    Some(format_epoch(BUILD_EPOCH.parse().ok()?))
}

/// The one line the About box and the window title report: the version, the
/// revision, and the date, so two builds are told apart at a glance.
pub fn stamp() -> String {
    let mut line = format!("{} ({})", version(), REVISION);
    if let Some(built) = built_at() {
        line.push_str(&format!(" built {built}"));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stamp_carries_the_version_and_the_revision() {
        let stamp = stamp();
        assert!(stamp.starts_with(version()), "{stamp}");
        assert!(stamp.contains(REVISION), "{stamp}");
    }

    #[test]
    fn a_dirty_revision_reports_the_revision_without_the_marker() {
        if is_dirty() {
            assert!(!revision().contains("+dirty"));
        }
    }

    #[test]
    fn the_build_date_is_a_well_formed_timestamp() {
        let built = built_at().expect("the build stamp is a number");
        let (date, time) = built.split_once(' ').expect("a date and a time");
        assert_eq!(date.len(), 10, "{built}");
        assert_eq!(time.len(), 5, "{built}");
    }

    #[test]
    fn epochs_convert_to_their_own_dates() {
        // The leap day and the non-leap century are the two cases a naive
        // implementation gets wrong, so both are pinned here.
        for (epoch, expected) in [
            (0, "1970-01-01 00:00"),
            (1_000_000_000, "2001-09-09 01:46"),
            (1_709_164_800, "2024-02-29 00:00"),
            (1_709_164_800 + 86_399, "2024-02-29 23:59"),
            (4_107_542_400, "2100-03-01 00:00"),
        ] {
            assert_eq!(format_epoch(epoch), expected, "epoch {epoch}");
        }
    }

    #[test]
    fn times_before_the_epoch_stay_readable() {
        // A clock set wrong can hand the build a negative stamp; it must not
        // panic or print something absurd.
        assert_eq!(format_epoch(-1), "1969-12-31 23:59");
    }
}
