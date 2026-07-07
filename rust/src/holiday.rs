//! Holiday rule definitions evaluated lazily per year.

use chrono::{Datelike, Duration, NaiveDate, Weekday as ChronoWeekday};

pub use chrono::Weekday;

/// Weekend observance roll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeekendRoll {
    /// No adjustment.
    None,
    /// Sat → Fri, Sun → Mon (US/Western convention).
    NearestWeekday,
    /// Sat → Mon, Sun → Mon (UK/Commonwealth bank-holiday substitution).
    ForwardMonday,
    /// Sun → Mon only; Saturday holidays are not substituted (South Africa).
    SundayToMonday,
    /// Sat → Fri, Sun → preceding Fri; a weekend holiday moves to the last
    /// weekday before it (SIX New Year's Eve).
    PrecedingFriday,
}

/// A holiday whose date depends only on the calendar year.
#[derive(Clone, Debug)]
pub enum HolidayRule {
    /// Fixed month/day (e.g. Christmas = 12/25).
    Fixed {
        month: u32,
        day: u32,
        roll: WeekendRoll,
        since_year: Option<i32>,
        until_year: Option<i32>,
    },
    /// Nth weekday of a month (e.g. 3rd Monday in January = MLK Day).
    /// `n` is 1-based; negative `n` counts from the end (-1 = last).
    NthWeekday {
        month: u32,
        weekday: Weekday,
        n: i32,
        since_year: Option<i32>,
        until_year: Option<i32>,
    },
    /// Easter Sunday plus offset days (e.g. Good Friday = -2, Easter Monday = +1).
    EasterOffset {
        offset_days: i32,
        since_year: Option<i32>,
        until_year: Option<i32>,
    },
    /// Two consecutive calendar days (`month`/`day` and the next day), each
    /// observed via `roll`, with the second bumped past the first's observed
    /// date so they never collide. Models Christmas+Boxing (ForwardMonday),
    /// South African Christmas+Goodwill (SundayToMonday), and NZ New Year pair.
    ConsecutivePair {
        month: u32,
        day: u32,
        roll: WeekendRoll,
        since_year: Option<i32>,
        until_year: Option<i32>,
    },
    /// Latest `weekday` on or before `month`/`day` (e.g. Canadian Victoria Day =
    /// the Monday on or before May 24).
    WeekdayOnOrBefore {
        month: u32,
        day: u32,
        weekday: Weekday,
        since_year: Option<i32>,
        until_year: Option<i32>,
    },
    /// Earliest `weekday` on or after `month`/`day` (e.g. Colombian Emiliani-law
    /// holidays that move to the following Monday).
    WeekdayOnOrAfter {
        month: u32,
        day: u32,
        weekday: Weekday,
        since_year: Option<i32>,
        until_year: Option<i32>,
    },
    /// Japanese vernal (`spring = true`) or autumnal equinox day, computed from
    /// the standard astronomical approximation (valid ~1980-2099).
    JapaneseEquinox {
        spring: bool,
        since_year: Option<i32>,
        until_year: Option<i32>,
    },
    /// A Chinese lunisolar date (`month`/`day`, non-leap) plus an offset in days,
    /// resolved astronomically in China Standard Time.
    ChineseLunar {
        month: u32,
        day: u32,
        offset_days: i64,
        since_year: Option<i32>,
        until_year: Option<i32>,
    },
    /// Qingming / Ching Ming (solar term at 15° solar longitude) plus an offset.
    Qingming {
        offset_days: i64,
        since_year: Option<i32>,
        until_year: Option<i32>,
    },
    /// A static lookup table keyed by year (e.g. lunar holidays we don't compute).
    Tabulated { table: &'static [(i32, u32, u32)] },
}

/// Japanese vernal or autumnal equinox day (valid ~1980-2099).
pub fn japanese_equinox(year: i32, spring: bool) -> Option<NaiveDate> {
    let y = year as f64;
    let (base, month) = if spring { (20.8431, 3) } else { (23.2488, 9) };
    let leap = ((year - 1980) as f64 / 4.0).floor();
    let day = (base + 0.242194 * (y - 1980.0) - leap).floor() as u32;
    NaiveDate::from_ymd_opt(year, month, day)
}

fn in_window(year: i32, since_year: Option<i32>, until_year: Option<i32>) -> bool {
    if let Some(s) = since_year {
        if year < s {
            return false;
        }
    }
    if let Some(u) = until_year {
        if year > u {
            return false;
        }
    }
    true
}

/// Roll a date forward to the next weekday (Sat/Sun → Mon).
fn bump_to_weekday(mut d: NaiveDate) -> NaiveDate {
    while matches!(d.weekday(), ChronoWeekday::Sat | ChronoWeekday::Sun) {
        d += Duration::days(1);
    }
    d
}

/// Observed dates for a consecutive-day pair (`month`/`day` and the next day),
/// each rolled via `roll`, with the second bumped to the next weekday if it
/// would collide with the first. A component left on a weekend by `roll` (e.g.
/// SundayToMonday leaving a Saturday date) is harmless: weekend days are already
/// non-trading.
pub fn consecutive_pair_observed(
    year: i32,
    month: u32,
    day: u32,
    roll: WeekendRoll,
) -> Option<(NaiveDate, NaiveDate)> {
    let first_raw = NaiveDate::from_ymd_opt(year, month, day)?;
    let first = apply_roll(first_raw, roll);
    let mut second = apply_roll(first_raw + Duration::days(1), roll);
    if second <= first {
        second = bump_to_weekday(first + Duration::days(1));
    }
    Some((first, second))
}

impl HolidayRule {
    /// Return the observed date in `year`, or `None` if not yet observed.
    pub fn observed_in(&self, year: i32) -> Option<NaiveDate> {
        match self {
            HolidayRule::Fixed {
                month,
                day,
                roll,
                since_year,
                until_year,
            } => {
                if !in_window(year, *since_year, *until_year) {
                    return None;
                }
                let raw = NaiveDate::from_ymd_opt(year, *month, *day)?;
                Some(apply_roll(raw, *roll))
            }
            HolidayRule::NthWeekday {
                month,
                weekday,
                n,
                since_year,
                until_year,
            } => {
                if !in_window(year, *since_year, *until_year) {
                    return None;
                }
                nth_weekday_of_month(year, *month, *weekday, *n)
            }
            HolidayRule::EasterOffset {
                offset_days,
                since_year,
                until_year,
            } => {
                if !in_window(year, *since_year, *until_year) {
                    return None;
                }
                let easter = easter_sunday(year)?;
                Some(easter + Duration::days(*offset_days as i64))
            }
            // Multi-date rule; single-date accessor returns the first component.
            HolidayRule::ConsecutivePair {
                month,
                day,
                roll,
                since_year,
                until_year,
            } => {
                if !in_window(year, *since_year, *until_year) {
                    return None;
                }
                consecutive_pair_observed(year, *month, *day, *roll).map(|(a, _)| a)
            }
            HolidayRule::WeekdayOnOrBefore {
                month,
                day,
                weekday,
                since_year,
                until_year,
            } => {
                if !in_window(year, *since_year, *until_year) {
                    return None;
                }
                let anchor = NaiveDate::from_ymd_opt(year, *month, *day)?;
                let back = (anchor.weekday().num_days_from_monday() as i64
                    - weekday.num_days_from_monday() as i64)
                    .rem_euclid(7);
                Some(anchor - Duration::days(back))
            }
            HolidayRule::WeekdayOnOrAfter {
                month,
                day,
                weekday,
                since_year,
                until_year,
            } => {
                if !in_window(year, *since_year, *until_year) {
                    return None;
                }
                let anchor = NaiveDate::from_ymd_opt(year, *month, *day)?;
                let fwd = (weekday.num_days_from_monday() as i64
                    - anchor.weekday().num_days_from_monday() as i64)
                    .rem_euclid(7);
                Some(anchor + Duration::days(fwd))
            }
            HolidayRule::JapaneseEquinox {
                spring,
                since_year,
                until_year,
            } => {
                if !in_window(year, *since_year, *until_year) {
                    return None;
                }
                japanese_equinox(year, *spring)
            }
            HolidayRule::ChineseLunar {
                month,
                day,
                offset_days,
                since_year,
                until_year,
            } => {
                if !in_window(year, *since_year, *until_year) {
                    return None;
                }
                crate::lunar::lunar_to_gregorian(year, *month, *day, false)
                    .map(|d| d + Duration::days(*offset_days))
            }
            HolidayRule::Qingming {
                offset_days,
                since_year,
                until_year,
            } => {
                if !in_window(year, *since_year, *until_year) {
                    return None;
                }
                Some(crate::lunar::qingming(year) + Duration::days(*offset_days))
            }
            HolidayRule::Tabulated { table } => table
                .iter()
                .find(|(y, _, _)| *y == year)
                .and_then(|(_, m, d)| NaiveDate::from_ymd_opt(year, *m, *d)),
        }
    }

    /// Return all dates this rule produces in `year`. Equivalent to
    /// `observed_in` for single-date rules; for `Tabulated` returns every
    /// row matching the year so multi-day closures are captured.
    pub fn dates_in(&self, year: i32) -> Vec<NaiveDate> {
        match self {
            HolidayRule::Tabulated { table } => table
                .iter()
                .filter(|(y, _, _)| *y == year)
                .filter_map(|(_, m, d)| NaiveDate::from_ymd_opt(year, *m, *d))
                .collect(),
            HolidayRule::ConsecutivePair {
                month,
                day,
                roll,
                since_year,
                until_year,
            } => {
                if !in_window(year, *since_year, *until_year) {
                    return Vec::new();
                }
                consecutive_pair_observed(year, *month, *day, *roll)
                    .map(|(a, b)| vec![a, b])
                    .unwrap_or_default()
            }
            _ => self.observed_in(year).into_iter().collect(),
        }
    }
}

fn apply_roll(d: NaiveDate, roll: WeekendRoll) -> NaiveDate {
    match roll {
        WeekendRoll::None => d,
        WeekendRoll::NearestWeekday => match d.weekday() {
            ChronoWeekday::Sat => d - Duration::days(1),
            ChronoWeekday::Sun => d + Duration::days(1),
            _ => d,
        },
        WeekendRoll::ForwardMonday => match d.weekday() {
            ChronoWeekday::Sat => d + Duration::days(2),
            ChronoWeekday::Sun => d + Duration::days(1),
            _ => d,
        },
        WeekendRoll::SundayToMonday => match d.weekday() {
            ChronoWeekday::Sun => d + Duration::days(1),
            _ => d,
        },
        WeekendRoll::PrecedingFriday => match d.weekday() {
            ChronoWeekday::Sat => d - Duration::days(1),
            ChronoWeekday::Sun => d - Duration::days(2),
            _ => d,
        },
    }
}

/// Nth occurrence of `weekday` in the given month/year. `n` may be negative
/// to count from the end (-1 = last).
pub fn nth_weekday_of_month(year: i32, month: u32, weekday: Weekday, n: i32) -> Option<NaiveDate> {
    if n == 0 {
        return None;
    }
    if n > 0 {
        let first = NaiveDate::from_ymd_opt(year, month, 1)?;
        let offset = (weekday.num_days_from_monday() as i64
            - first.weekday().num_days_from_monday() as i64)
            .rem_euclid(7)
            + 7 * (n as i64 - 1);
        let candidate = first + Duration::days(offset);
        if candidate.month() == month {
            Some(candidate)
        } else {
            None
        }
    } else {
        // last day of month
        let last_of_month = match month {
            12 => NaiveDate::from_ymd_opt(year + 1, 1, 1)? - Duration::days(1),
            _ => NaiveDate::from_ymd_opt(year, month + 1, 1)? - Duration::days(1),
        };
        let back = (last_of_month.weekday().num_days_from_monday() as i64
            - weekday.num_days_from_monday() as i64)
            .rem_euclid(7);
        let last_of_kind = last_of_month - Duration::days(back);
        let candidate = last_of_kind - Duration::days(((-n - 1) as i64) * 7);
        if candidate.month() == month {
            Some(candidate)
        } else {
            None
        }
    }
}

/// Anonymous Gregorian (Meeus/Jones/Butcher) algorithm for Easter Sunday.
pub fn easter_sunday(year: i32) -> Option<NaiveDate> {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = ((h + l - 7 * m + 114) % 31) + 1;
    NaiveDate::from_ymd_opt(year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easter_known_dates() {
        assert_eq!(
            easter_sunday(2024).unwrap(),
            NaiveDate::from_ymd_opt(2024, 3, 31).unwrap()
        );
        assert_eq!(
            easter_sunday(2025).unwrap(),
            NaiveDate::from_ymd_opt(2025, 4, 20).unwrap()
        );
        assert_eq!(
            easter_sunday(2000).unwrap(),
            NaiveDate::from_ymd_opt(2000, 4, 23).unwrap()
        );
    }

    #[test]
    fn nth_weekday_mlk_day() {
        let d = nth_weekday_of_month(2024, 1, Weekday::Mon, 3).unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());
    }

    #[test]
    fn nth_weekday_last_memorial_day() {
        let d = nth_weekday_of_month(2024, 5, Weekday::Mon, -1).unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2024, 5, 27).unwrap());
    }

    #[test]
    fn nth_weekday_thanksgiving_2024() {
        // 4th Thursday in November 2024 = Nov 28.
        let d = nth_weekday_of_month(2024, 11, Weekday::Thu, 4).unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2024, 11, 28).unwrap());
    }

    #[test]
    fn weekend_roll_christmas_2022() {
        let r = HolidayRule::Fixed {
            month: 12,
            day: 25,
            roll: WeekendRoll::NearestWeekday,
            since_year: None,
            until_year: None,
        };
        assert_eq!(
            r.observed_in(2022).unwrap(),
            NaiveDate::from_ymd_opt(2022, 12, 26).unwrap()
        );
    }

    #[test]
    fn juneteenth_since_2021() {
        let r = HolidayRule::Fixed {
            month: 6,
            day: 19,
            roll: WeekendRoll::NearestWeekday,
            since_year: Some(2021),
            until_year: None,
        };
        assert!(r.observed_in(2020).is_none());
        assert!(r.observed_in(2021).is_some());
    }

    #[test]
    fn christmas_boxing_substitution() {
        let cb = |y| consecutive_pair_observed(y, 12, 25, WeekendRoll::ForwardMonday).unwrap();
        // 2021: Dec 25 Sat → Mon 27, Dec 26 Sun → Tue 28 (bumped past Christmas).
        let (x, b) = cb(2021);
        assert_eq!(x, NaiveDate::from_ymd_opt(2021, 12, 27).unwrap());
        assert_eq!(b, NaiveDate::from_ymd_opt(2021, 12, 28).unwrap());
        // 2016: Dec 25 Sun → Mon 26, Dec 26 Mon collides → Tue 27.
        let (x, b) = cb(2016);
        assert_eq!(x, NaiveDate::from_ymd_opt(2016, 12, 26).unwrap());
        assert_eq!(b, NaiveDate::from_ymd_opt(2016, 12, 27).unwrap());
    }

    #[test]
    fn sunday_only_pair_south_africa() {
        // SA Christmas+Goodwill: 2021 Dec 25 Sat NOT substituted, Dec 26 Sun → Mon 27.
        let (a, b) = consecutive_pair_observed(2021, 12, 25, WeekendRoll::SundayToMonday).unwrap();
        assert_eq!(a, NaiveDate::from_ymd_opt(2021, 12, 25).unwrap()); // Saturday, non-trading
        assert_eq!(b, NaiveDate::from_ymd_opt(2021, 12, 27).unwrap());
    }

    #[test]
    fn forward_monday_roll() {
        let r = HolidayRule::Fixed {
            month: 1,
            day: 1,
            roll: WeekendRoll::ForwardMonday,
            since_year: None,
            until_year: None,
        };
        // 2022-01-01 Sat → Mon Jan 3.
        assert_eq!(
            r.observed_in(2022).unwrap(),
            NaiveDate::from_ymd_opt(2022, 1, 3).unwrap()
        );
    }
}
