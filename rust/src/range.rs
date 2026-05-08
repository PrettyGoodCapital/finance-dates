//! Plain and business-day range iteration helpers.

use chrono::{Datelike, Duration, NaiveDate};
use std::collections::BTreeSet;

/// Inclusive calendar-day range with a fixed integer step (days).
pub fn date_range(start: NaiveDate, end: NaiveDate, step_days: u32) -> Vec<NaiveDate> {
    if step_days == 0 || end < start {
        return Vec::new();
    }
    let total = (end - start).num_days();
    let cap = (total / step_days as i64) as usize + 1;
    let mut out = Vec::with_capacity(cap);
    let mut d = start;
    while d <= end {
        out.push(d);
        d += Duration::days(step_days as i64);
    }
    out
}

/// Inclusive business-day range. `weekmask[i]` is true if weekday `i`
/// (Mon=0 … Sun=6) is a business day. `holidays` is a set of dates
/// to exclude regardless of weekday.
pub fn business_day_range(
    start: NaiveDate,
    end: NaiveDate,
    weekmask: &[bool; 7],
    holidays: &BTreeSet<NaiveDate>,
) -> Vec<NaiveDate> {
    if end < start {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(((end - start).num_days() as usize).saturating_add(1));
    let mut d = start;
    while d <= end {
        let i = d.weekday().num_days_from_monday() as usize;
        if weekmask[i] && !holidays.contains(&d) {
            out.push(d);
        }
        d += Duration::days(1);
    }
    out
}

/// Standard Mon–Fri weekmask.
pub const STANDARD_WEEKMASK: [bool; 7] =
    [true, true, true, true, true, false, false];

/// Move to the next business day strictly after `d`.
pub fn next_business_day(
    d: NaiveDate,
    weekmask: &[bool; 7],
    holidays: &BTreeSet<NaiveDate>,
) -> NaiveDate {
    let mut x = d + Duration::days(1);
    loop {
        let i = x.weekday().num_days_from_monday() as usize;
        if weekmask[i] && !holidays.contains(&x) {
            return x;
        }
        x += Duration::days(1);
    }
}

/// Move to the previous business day strictly before `d`.
pub fn previous_business_day(
    d: NaiveDate,
    weekmask: &[bool; 7],
    holidays: &BTreeSet<NaiveDate>,
) -> NaiveDate {
    let mut x = d - Duration::days(1);
    loop {
        let i = x.weekday().num_days_from_monday() as usize;
        if weekmask[i] && !holidays.contains(&x) {
            return x;
        }
        x -= Duration::days(1);
    }
}

/// Number of business days in [start, end] inclusive.
pub fn business_days_between(
    start: NaiveDate,
    end: NaiveDate,
    weekmask: &[bool; 7],
    holidays: &BTreeSet<NaiveDate>,
) -> i64 {
    if end < start {
        return 0;
    }
    let mut n = 0i64;
    let mut d = start;
    while d <= end {
        let i = d.weekday().num_days_from_monday() as usize;
        if weekmask[i] && !holidays.contains(&d) {
            n += 1;
        }
        d += Duration::days(1);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn date_range_unit() {
        let out = date_range(
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2024, 1, 5).unwrap(),
            1,
        );
        assert_eq!(out.len(), 5);
    }

    #[test]
    fn business_days_2024_no_holidays() {
        // 2024 has 262 weekdays.
        let bd = business_days_between(
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2024, 12, 31).unwrap(),
            &STANDARD_WEEKMASK,
            &BTreeSet::new(),
        );
        assert_eq!(bd, 262);
    }

    #[test]
    fn next_business_day_skips_weekend() {
        let fri = NaiveDate::from_ymd_opt(2024, 5, 24).unwrap(); // Friday
        let next = next_business_day(fri, &STANDARD_WEEKMASK, &BTreeSet::new());
        assert_eq!(next, NaiveDate::from_ymd_opt(2024, 5, 27).unwrap());
    }
}
