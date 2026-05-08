//! Holiday rule definitions evaluated lazily per year.

use chrono::{Datelike, Duration, NaiveDate, Weekday as ChronoWeekday};

pub use chrono::Weekday;

/// Saturday→Friday, Sunday→Monday observance roll, US-style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeekendRoll {
    /// No adjustment.
    None,
    /// Sat → Fri, Sun → Mon (US/Western convention).
    NearestWeekday,
}

/// A holiday whose date depends only on the calendar year.
#[derive(Clone, Debug)]
pub enum HolidayRule {
    /// Fixed month/day (e.g. Christmas = 12/25).
    Fixed { month: u32, day: u32, roll: WeekendRoll, since_year: Option<i32> },
    /// Nth weekday of a month (e.g. 3rd Monday in January = MLK Day).
    /// `n` is 1-based; negative `n` counts from the end (-1 = last).
    NthWeekday { month: u32, weekday: Weekday, n: i32, since_year: Option<i32> },
    /// Easter Sunday plus offset days (e.g. Good Friday = -2, Easter Monday = +1).
    EasterOffset { offset_days: i32, since_year: Option<i32> },
    /// A static lookup table keyed by year (e.g. lunar holidays we don't compute).
    Tabulated { table: &'static [(i32, u32, u32)] },
}

impl HolidayRule {
    /// Return the observed date in `year`, or `None` if not yet observed.
    pub fn observed_in(&self, year: i32) -> Option<NaiveDate> {
        match self {
            HolidayRule::Fixed { month, day, roll, since_year } => {
                if let Some(y) = since_year {
                    if year < *y {
                        return None;
                    }
                }
                let raw = NaiveDate::from_ymd_opt(year, *month, *day)?;
                Some(apply_roll(raw, *roll))
            }
            HolidayRule::NthWeekday { month, weekday, n, since_year } => {
                if let Some(y) = since_year {
                    if year < *y {
                        return None;
                    }
                }
                nth_weekday_of_month(year, *month, *weekday, *n)
            }
            HolidayRule::EasterOffset { offset_days, since_year } => {
                if let Some(y) = since_year {
                    if year < *y {
                        return None;
                    }
                }
                let easter = easter_sunday(year)?;
                Some(easter + Duration::days(*offset_days as i64))
            }
            HolidayRule::Tabulated { table } => {
                table
                    .iter()
                    .find(|(y, _, _)| *y == year)
                    .and_then(|(_, m, d)| NaiveDate::from_ymd_opt(year, *m, *d))
            }
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
        if candidate.month() == month { Some(candidate) } else { None }
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
        if candidate.month() == month { Some(candidate) } else { None }
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
        assert_eq!(easter_sunday(2024).unwrap(), NaiveDate::from_ymd_opt(2024, 3, 31).unwrap());
        assert_eq!(easter_sunday(2025).unwrap(), NaiveDate::from_ymd_opt(2025, 4, 20).unwrap());
        assert_eq!(easter_sunday(2000).unwrap(), NaiveDate::from_ymd_opt(2000, 4, 23).unwrap());
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
            month: 12, day: 25, roll: WeekendRoll::NearestWeekday, since_year: None,
        };
        assert_eq!(r.observed_in(2022).unwrap(), NaiveDate::from_ymd_opt(2022, 12, 26).unwrap());
    }

    #[test]
    fn juneteenth_since_2021() {
        let r = HolidayRule::Fixed {
            month: 6, day: 19, roll: WeekendRoll::NearestWeekday, since_year: Some(2021),
        };
        assert!(r.observed_in(2020).is_none());
        assert!(r.observed_in(2021).is_some());
    }
}
