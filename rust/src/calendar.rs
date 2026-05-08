//! Per-exchange / per-region calendars.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, Utc};
use std::collections::BTreeSet;
use std::sync::Arc;

use crate::holiday::{HolidayRule, Weekday, WeekendRoll};
use crate::range::{
    business_day_range, business_days_between, next_business_day, previous_business_day,
    STANDARD_WEEKMASK,
};
use crate::trading_hours::TradingHours;

/// Canonical exchange MIC codes recognised by `calendar_for_exchange`.
pub const EXCHANGE_CODES: &[&str] = &[
    "XNYS", "XNAS", "XLON", "XTKS", "XHKG", "XSHG", "XEUR", "XPAR", "XFRA",
    "XTSE", "XASX", "XBOM", "XNSE",
];

/// ISO region codes recognised by `calendar_for_region`.
pub const REGION_CODES: &[&str] = &[
    "US", "UK", "EU", "JP", "HK", "CN", "CA", "AU", "IN", "DE", "FR",
];

/// A holiday calendar with optional trading hours.
pub struct Calendar {
    pub name: String,
    pub weekmask: [bool; 7],
    pub rules: Vec<HolidayRule>,
    pub trading_hours: Option<TradingHours>,
    cache: HolidayCache,
}

#[derive(Default)]
struct HolidayCache {
    inner: parking_lot_dummy::RwLock<std::collections::HashMap<i32, Arc<BTreeSet<NaiveDate>>>>,
}

// minimal RwLock to avoid pulling parking_lot as a dep
mod parking_lot_dummy {
    use std::sync::RwLock as StdRwLock;
    pub struct RwLock<T>(pub StdRwLock<T>);
    impl<T: Default> Default for RwLock<T> {
        fn default() -> Self {
            Self(StdRwLock::new(T::default()))
        }
    }
    impl<T> RwLock<T> {
        pub fn read(&self) -> std::sync::RwLockReadGuard<'_, T> {
            self.0.read().unwrap()
        }
        pub fn write(&self) -> std::sync::RwLockWriteGuard<'_, T> {
            self.0.write().unwrap()
        }
    }
}

impl Calendar {
    pub fn new(
        name: impl Into<String>,
        weekmask: [bool; 7],
        rules: Vec<HolidayRule>,
        trading_hours: Option<TradingHours>,
    ) -> Self {
        Self {
            name: name.into(),
            weekmask,
            rules,
            trading_hours,
            cache: HolidayCache::default(),
        }
    }

    /// Return holiday dates observed in `year`.
    pub fn holidays(&self, year: i32) -> Arc<BTreeSet<NaiveDate>> {
        if let Some(h) = self.cache.inner.read().get(&year).cloned() {
            return h;
        }
        let mut set = BTreeSet::new();
        for r in &self.rules {
            if let Some(d) = r.observed_in(year) {
                set.insert(d);
            }
        }
        let arc = Arc::new(set);
        self.cache.inner.write().insert(year, arc.clone());
        arc
    }

    /// Holidays that fall in [start, end] inclusive.
    pub fn holidays_between(&self, start: NaiveDate, end: NaiveDate) -> BTreeSet<NaiveDate> {
        let mut out = BTreeSet::new();
        for y in start.year()..=end.year() {
            for d in self.holidays(y).iter() {
                if *d >= start && *d <= end {
                    out.insert(*d);
                }
            }
        }
        out
    }

    pub fn is_holiday(&self, d: NaiveDate) -> bool {
        self.holidays(d.year()).contains(&d)
    }

    pub fn is_business_day(&self, d: NaiveDate) -> bool {
        let i = d.weekday().num_days_from_monday() as usize;
        self.weekmask[i] && !self.is_holiday(d)
    }

    pub fn next_business_day(&self, d: NaiveDate) -> NaiveDate {
        // Build a small holiday set covering the next ~year window.
        let years = [d.year(), d.year() + 1];
        let mut h = BTreeSet::new();
        for y in years {
            for x in self.holidays(y).iter() {
                h.insert(*x);
            }
        }
        next_business_day(d, &self.weekmask, &h)
    }

    pub fn previous_business_day(&self, d: NaiveDate) -> NaiveDate {
        let years = [d.year() - 1, d.year()];
        let mut h = BTreeSet::new();
        for y in years {
            for x in self.holidays(y).iter() {
                h.insert(*x);
            }
        }
        previous_business_day(d, &self.weekmask, &h)
    }

    pub fn business_days_between(&self, start: NaiveDate, end: NaiveDate) -> i64 {
        let h = self.holidays_between(start, end);
        business_days_between(start, end, &self.weekmask, &h)
    }

    pub fn business_day_range(&self, start: NaiveDate, end: NaiveDate) -> Vec<NaiveDate> {
        let h = self.holidays_between(start, end);
        business_day_range(start, end, &self.weekmask, &h)
    }

    pub fn is_open(&self, when: DateTime<Utc>) -> bool {
        let Some(th) = &self.trading_hours else { return false; };
        let local_date = when.with_timezone(&th.timezone).date_naive();
        if !self.is_business_day(local_date) {
            return false;
        }
        th.contains_local_time(when)
    }

    /// First open instant on or after `when`. Walks forward business days.
    pub fn next_open(&self, when: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let th = self.trading_hours.as_ref()?;
        let mut local_date = when.with_timezone(&th.timezone).date_naive();
        for _ in 0..400 {
            if self.is_business_day(local_date) {
                let open = th.open_at(local_date.year(), local_date.month(), local_date.day())?;
                if open >= when {
                    return Some(open);
                }
            }
            local_date += Duration::days(1);
        }
        None
    }

    pub fn next_close(&self, when: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let th = self.trading_hours.as_ref()?;
        let mut local_date = when.with_timezone(&th.timezone).date_naive();
        for _ in 0..400 {
            if self.is_business_day(local_date) {
                let close = th.close_at(local_date.year(), local_date.month(), local_date.day())?;
                if close >= when {
                    return Some(close);
                }
            }
            local_date += Duration::days(1);
        }
        None
    }
}

// ---------- Built-in calendars ----------

fn fixed(month: u32, day: u32, since_year: Option<i32>) -> HolidayRule {
    HolidayRule::Fixed { month, day, roll: WeekendRoll::NearestWeekday, since_year }
}

fn fixed_no_roll(month: u32, day: u32, since_year: Option<i32>) -> HolidayRule {
    HolidayRule::Fixed { month, day, roll: WeekendRoll::None, since_year }
}

fn nth(month: u32, weekday: Weekday, n: i32) -> HolidayRule {
    HolidayRule::NthWeekday { month, weekday, n, since_year: None }
}

fn easter(offset_days: i32) -> HolidayRule {
    HolidayRule::EasterOffset { offset_days, since_year: None }
}

fn nyse_rules() -> Vec<HolidayRule> {
    vec![
        fixed(1, 1, None),                              // New Year's Day
        nth(1, Weekday::Mon, 3),                        // MLK Day
        nth(2, Weekday::Mon, 3),                        // Presidents Day
        easter(-2),                                     // Good Friday
        nth(5, Weekday::Mon, -1),                       // Memorial Day
        fixed(6, 19, Some(2021)),                       // Juneteenth
        fixed(7, 4, None),                              // Independence Day
        nth(9, Weekday::Mon, 1),                        // Labor Day
        nth(11, Weekday::Thu, 4),                       // Thanksgiving
        fixed(12, 25, None),                            // Christmas
    ]
}

fn nyse_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        chrono_tz::America::New_York,
    )
}

fn lse_rules() -> Vec<HolidayRule> {
    vec![
        fixed(1, 1, None),                              // New Year's Day
        easter(-2),                                     // Good Friday
        easter(1),                                      // Easter Monday
        nth(5, Weekday::Mon, 1),                        // Early May Bank
        nth(5, Weekday::Mon, -1),                       // Spring Bank
        nth(8, Weekday::Mon, -1),                       // Summer Bank
        fixed(12, 25, None),                            // Christmas
        fixed(12, 26, None),                            // Boxing Day
    ]
}

fn lse_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 30, 0).unwrap(),
        chrono_tz::Europe::London,
    )
}

fn tse_rules() -> Vec<HolidayRule> {
    vec![
        fixed_no_roll(1, 1, None),                      // New Year
        fixed_no_roll(1, 2, None),
        fixed_no_roll(1, 3, None),
        nth(1, Weekday::Mon, 2),                        // Coming of Age
        fixed_no_roll(2, 11, None),                     // National Foundation
        fixed_no_roll(2, 23, Some(2020)),               // Emperor's Birthday
        fixed_no_roll(4, 29, None),                     // Showa Day
        fixed_no_roll(5, 3, None),                      // Constitution
        fixed_no_roll(5, 4, None),                      // Greenery
        fixed_no_roll(5, 5, None),                      // Children's
        nth(7, Weekday::Mon, 3),                        // Marine
        fixed_no_roll(8, 11, None),                     // Mountain
        nth(9, Weekday::Mon, 3),                        // Respect for the Aged
        nth(10, Weekday::Mon, 2),                       // Sports
        fixed_no_roll(11, 3, None),                     // Culture
        fixed_no_roll(11, 23, None),                    // Labour Thanksgiving
        fixed_no_roll(12, 31, None),                    // Year-end
    ]
}

fn tse_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
        chrono_tz::Asia::Tokyo,
    )
}

// HK: New Year, Christmas, Boxing Day, Labour Day, National Day, plus a
// small Lunar-NY table 2020-2030. Approximate; users can override.
fn hkex_rules() -> Vec<HolidayRule> {
    let lny: &'static [(i32, u32, u32)] = &[
        (2020, 1, 27), (2021, 2, 12), (2022, 2, 1), (2023, 1, 23),
        (2024, 2, 12), (2025, 1, 29), (2026, 2, 17), (2027, 2, 8),
        (2028, 1, 26), (2029, 2, 13), (2030, 2, 4),
    ];
    vec![
        fixed(1, 1, None),
        HolidayRule::Tabulated { table: lny },
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        fixed(7, 1, None),                              // HK SAR Establishment
        fixed(10, 1, None),                             // National Day
        fixed(12, 25, None),
        fixed(12, 26, None),
    ]
}

fn hkex_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        chrono_tz::Asia::Hong_Kong,
    )
}

fn sse_rules() -> Vec<HolidayRule> {
    let lny: &'static [(i32, u32, u32)] = &[
        (2020, 1, 25), (2021, 2, 12), (2022, 2, 1), (2023, 1, 22),
        (2024, 2, 10), (2025, 1, 29), (2026, 2, 17), (2027, 2, 6),
        (2028, 1, 26), (2029, 2, 13), (2030, 2, 3),
    ];
    vec![
        fixed(1, 1, None),
        HolidayRule::Tabulated { table: lny },
        fixed(5, 1, None),
        fixed(10, 1, None),
        fixed(10, 2, None),
        fixed(10, 3, None),
    ]
}

fn sse_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
        chrono_tz::Asia::Shanghai,
    )
}

fn xetra_rules() -> Vec<HolidayRule> {
    vec![
        fixed(1, 1, None),
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        fixed(10, 3, None),                             // German Unity Day
        fixed(12, 24, None),                            // Christmas Eve
        fixed(12, 25, None),
        fixed(12, 26, None),
        fixed(12, 31, None),
    ]
}

fn xetra_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 30, 0).unwrap(),
        chrono_tz::Europe::Berlin,
    )
}

fn euronext_paris_rules() -> Vec<HolidayRule> {
    vec![
        fixed(1, 1, None),
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
    ]
}

fn euronext_paris_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 30, 0).unwrap(),
        chrono_tz::Europe::Paris,
    )
}

fn tsx_rules() -> Vec<HolidayRule> {
    vec![
        fixed(1, 1, None),
        nth(2, Weekday::Mon, 3),                        // Family Day (third Monday Feb)
        easter(-2),
        nth(5, Weekday::Mon, -1),                       // Victoria Day (last Mon May before/on May 24) - approx
        fixed(7, 1, None),                              // Canada Day
        nth(8, Weekday::Mon, 1),                        // Civic Holiday
        nth(9, Weekday::Mon, 1),                        // Labour Day
        nth(10, Weekday::Mon, 2),                       // Thanksgiving (CA)
        fixed(12, 25, None),
        fixed(12, 26, None),                            // Boxing Day
    ]
}

fn tsx_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        chrono_tz::America::Toronto,
    )
}

fn asx_rules() -> Vec<HolidayRule> {
    vec![
        fixed(1, 1, None),
        fixed(1, 26, None),                             // Australia Day
        easter(-2),
        easter(1),
        fixed(4, 25, None),                             // ANZAC Day
        nth(6, Weekday::Mon, 2),                        // Queen's/King's Birthday
        fixed(12, 25, None),
        fixed(12, 26, None),
    ]
}

fn asx_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        chrono_tz::Australia::Sydney,
    )
}

fn nse_rules() -> Vec<HolidayRule> {
    // India: Republic Day, Independence Day, Gandhi Jayanti, Christmas + a
    // few. Religious holidays vary; this is a stable subset.
    vec![
        fixed(1, 26, None),
        fixed(8, 15, None),
        fixed(10, 2, None),
        fixed(12, 25, None),
    ]
}

fn nse_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 15, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 30, 0).unwrap(),
        chrono_tz::Asia::Kolkata,
    )
}

/// Build a calendar from its MIC code. Returns `None` if unknown.
pub fn calendar_for_exchange(code: &str) -> Option<Calendar> {
    let upper = code.to_ascii_uppercase();
    let cal = match upper.as_str() {
        "XNYS" | "XNAS" => Calendar::new(
            upper.clone(), STANDARD_WEEKMASK, nyse_rules(), Some(nyse_trading_hours()),
        ),
        "XLON" => Calendar::new(
            "XLON", STANDARD_WEEKMASK, lse_rules(), Some(lse_trading_hours()),
        ),
        "XTKS" => Calendar::new(
            "XTKS", STANDARD_WEEKMASK, tse_rules(), Some(tse_trading_hours()),
        ),
        "XHKG" => Calendar::new(
            "XHKG", STANDARD_WEEKMASK, hkex_rules(), Some(hkex_trading_hours()),
        ),
        "XSHG" => Calendar::new(
            "XSHG", STANDARD_WEEKMASK, sse_rules(), Some(sse_trading_hours()),
        ),
        "XEUR" | "XFRA" => Calendar::new(
            upper.clone(), STANDARD_WEEKMASK, xetra_rules(), Some(xetra_trading_hours()),
        ),
        "XPAR" => Calendar::new(
            "XPAR", STANDARD_WEEKMASK, euronext_paris_rules(), Some(euronext_paris_trading_hours()),
        ),
        "XTSE" => Calendar::new(
            "XTSE", STANDARD_WEEKMASK, tsx_rules(), Some(tsx_trading_hours()),
        ),
        "XASX" => Calendar::new(
            "XASX", STANDARD_WEEKMASK, asx_rules(), Some(asx_trading_hours()),
        ),
        "XBOM" | "XNSE" => Calendar::new(
            upper.clone(), STANDARD_WEEKMASK, nse_rules(), Some(nse_trading_hours()),
        ),
        _ => return None,
    };
    Some(cal)
}

/// Build a calendar from a region code (US, UK, JP, ...). Returns `None` if unknown.
pub fn calendar_for_region(code: &str) -> Option<Calendar> {
    match code.to_ascii_uppercase().as_str() {
        "US" => calendar_for_exchange("XNYS"),
        "UK" | "GB" => calendar_for_exchange("XLON"),
        "JP" => calendar_for_exchange("XTKS"),
        "HK" => calendar_for_exchange("XHKG"),
        "CN" => calendar_for_exchange("XSHG"),
        "DE" | "EU" => calendar_for_exchange("XFRA"),
        "FR" => calendar_for_exchange("XPAR"),
        "CA" => calendar_for_exchange("XTSE"),
        "AU" => calendar_for_exchange("XASX"),
        "IN" => calendar_for_exchange("XNSE"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nyse_2024_business_day_count() {
        // 2024 NYSE has 252 trading days.
        let cal = calendar_for_exchange("XNYS").unwrap();
        let n = cal.business_days_between(
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2024, 12, 31).unwrap(),
        );
        assert_eq!(n, 252);
    }

    #[test]
    fn nyse_christmas_2022_observed_monday() {
        let cal = calendar_for_exchange("XNYS").unwrap();
        // Dec 25 2022 is Sunday. Observed Monday Dec 26.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2022, 12, 26).unwrap()));
    }

    #[test]
    fn nyse_juneteenth_first_year_2021() {
        let cal = calendar_for_exchange("XNYS").unwrap();
        assert!(!cal.is_holiday(NaiveDate::from_ymd_opt(2020, 6, 19).unwrap()));
        // Jun 19 2021 was Saturday → observed Friday Jun 18.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2021, 6, 18).unwrap()));
    }

    #[test]
    fn lse_easter_monday_2024() {
        let cal = calendar_for_exchange("XLON").unwrap();
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2024, 4, 1).unwrap()));
    }

    #[test]
    fn region_us_resolves_to_xnys() {
        let cal = calendar_for_region("US").unwrap();
        assert_eq!(cal.name, "XNYS");
    }

    #[test]
    fn nyse_is_open_at_market_open() {
        let cal = calendar_for_exchange("XNYS").unwrap();
        // 2024-01-08 09:30 America/New_York → open.
        let inst = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 8, 9, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
        // Three minutes before open, same business day → closed.
        let inst_b = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 8, 9, 27, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(!cal.is_open(inst_b));
    }

    #[test]
    fn nyse_is_open_handles_dst() {
        // 2024-03-11 (first Monday after DST start) at 09:30 NY local should be open.
        let cal = calendar_for_exchange("XNYS").unwrap();
        let inst = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 3, 11, 9, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
    }
}
