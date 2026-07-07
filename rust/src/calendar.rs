//! Per-exchange / per-region calendars.
//!
//! Distinguishes equity, options, futures, FX, bond, and crypto markets, each
//! of which can have very different holiday calendars and trading hours. The
//! `calendar_for_exchange` lookup covers every MIC currently exposed by
//! finance-enums plus a few common futures venues.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, Utc};
use std::collections::BTreeSet;
use std::sync::Arc;

use crate::holiday::{HolidayRule, Weekday, WeekendRoll};
use crate::range::STANDARD_WEEKMASK;
use crate::trading_hours::{ExtendedSession, Session, TradingHours};

pub use finance_enums::data::{
    AgricultureType_VARIANTS as AGRICULTURE_TYPES, CommodityType_VARIANTS as COMMODITY_TYPES,
    CountryCode3_VARIANTS as COUNTRY_CODES3, CountryCode_VARIANTS as COUNTRY_CODES,
    EnergyType_VARIANTS as ENERGY_TYPES, ExchangeCode_VARIANTS as EXCHANGE_CODES,
    MarketType_VARIANTS as MARKET_TYPES, MetalsType_VARIANTS as METALS_TYPES,
    UnderlyingAssetClass_VARIANTS as UNDERLYING_ASSET_CLASSES,
};

/// Mon-Sun all-true weekmask (used by 24x7 crypto venues).
pub const CRYPTO_WEEKMASK: [bool; 7] = [true, true, true, true, true, true, true];

/// Sun-Fri weekmask used by 24x5 FX. Monday=index 0; Sunday=index 6.
pub const FX_WEEKMASK: [bool; 7] = [true, true, true, true, true, false, true];

const MARKET_TYPE_ENUM: &str = "MarketType";

fn finance_enum_variant(
    enum_name: &str,
    variants: &'static [&'static str],
    variant: &str,
) -> &'static str {
    variants
        .iter()
        .copied()
        .find(|candidate| *candidate == variant)
        .unwrap_or_else(|| panic!("finance-enums missing {enum_name}.{variant}"))
}

fn market_type(variant: &str) -> &'static str {
    finance_enum_variant(MARKET_TYPE_ENUM, MARKET_TYPES, variant)
}

/// Date-effective schedule data for a calendar family.
#[derive(Clone, Debug)]
pub struct CalendarSchedule {
    pub effective_start: NaiveDate,
    pub weekmask: [bool; 7],
    pub rules: Vec<HolidayRule>,
    pub trading_hours: Option<TradingHours>,
}

impl CalendarSchedule {
    pub fn new(
        effective_start: NaiveDate,
        weekmask: [bool; 7],
        rules: Vec<HolidayRule>,
        trading_hours: Option<TradingHours>,
    ) -> Self {
        Self {
            effective_start,
            weekmask,
            rules,
            trading_hours,
        }
    }
}

struct ResolvedSchedule<'a> {
    weekmask: &'a [bool; 7],
    rules: &'a [HolidayRule],
    trading_hours: Option<&'a TradingHours>,
}

/// A holiday calendar with optional trading hours and a market classification.
pub struct Calendar {
    pub name: String,
    /// One of the `MARKET_TYPES` entries, aligned with `finance-enums` `MarketType` variants.
    pub market_type: &'static str,
    pub weekmask: [bool; 7],
    pub rules: Vec<HolidayRule>,
    pub trading_hours: Option<TradingHours>,
    pub schedules: Vec<CalendarSchedule>,
    /// Days when the venue closes earlier than usual. Each rule resolves to
    /// at most one date per year, paired with a local close time that
    /// replaces the normal session close on that date.
    pub early_closes: Vec<EarlyCloseRule>,
    /// Dates a rule would produce but that the venue did not actually observe
    /// (e.g. a bank holiday moved to a different day in a single year).
    pub exceptions: BTreeSet<NaiveDate>,
    /// Post-processing applied to the computed holiday set (e.g. Japanese
    /// substitute and citizens' holidays).
    pub adjustment: HolidayAdjustment,
    cache: HolidayCache,
    early_cache: EarlyCloseCache,
}

/// Country-specific holiday post-processing applied after the base rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HolidayAdjustment {
    #[default]
    None,
    /// Japan: a national holiday on Sunday moves to the next non-holiday day
    /// (substitute holiday), and a weekday between two holidays becomes a
    /// holiday (citizens' holiday). The exchange-only year-end/new-year days
    /// (Jan 2-3, Dec 31) are excluded from this computation.
    Japanese,
}

/// An early-close rule. `rule` resolves to a date (using the same machinery
/// as holiday rules); `close_time` is the local time at which the venue
/// closes on that date instead of its regular session close.
#[derive(Clone, Debug)]
pub struct EarlyCloseRule {
    pub rule: HolidayRule,
    pub close_time: NaiveTime,
}

#[derive(Default)]
struct EarlyCloseCache {
    inner: parking_lot_dummy::RwLock<
        std::collections::HashMap<i32, Arc<std::collections::HashMap<NaiveDate, NaiveTime>>>,
    >,
}

#[derive(Default)]
struct HolidayCache {
    inner: parking_lot_dummy::RwLock<std::collections::HashMap<i32, Arc<BTreeSet<NaiveDate>>>>,
}

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
        Self::with_type(
            name,
            market_type("Equities"),
            weekmask,
            rules,
            trading_hours,
        )
    }

    pub fn with_type(
        name: impl Into<String>,
        market_type: &'static str,
        weekmask: [bool; 7],
        rules: Vec<HolidayRule>,
        trading_hours: Option<TradingHours>,
    ) -> Self {
        Self {
            name: name.into(),
            market_type,
            weekmask,
            rules,
            trading_hours,
            schedules: Vec::new(),
            early_closes: Vec::new(),
            exceptions: BTreeSet::new(),
            adjustment: HolidayAdjustment::None,
            cache: HolidayCache::default(),
            early_cache: EarlyCloseCache::default(),
        }
    }

    /// Builder: set country-specific holiday post-processing.
    pub fn with_adjustment(mut self, adjustment: HolidayAdjustment) -> Self {
        self.adjustment = adjustment;
        self.cache = HolidayCache::default();
        self
    }

    /// Builder: attach early-close rules.
    pub fn with_early_closes(mut self, ec: Vec<EarlyCloseRule>) -> Self {
        self.early_closes = ec;
        self
    }

    /// Builder: mark `(year, month, day)` dates that rules would produce but the
    /// venue did not actually close on (holiday moved to another day that year).
    pub fn with_exceptions(mut self, dates: &[(i32, u32, u32)]) -> Self {
        self.exceptions = dates
            .iter()
            .filter_map(|(y, m, d)| NaiveDate::from_ymd_opt(*y, *m, *d))
            .collect();
        self.cache = HolidayCache::default();
        self
    }

    /// Builder: attach date-effective schedules sorted by effective date.
    pub fn with_schedules(mut self, mut schedules: Vec<CalendarSchedule>) -> Self {
        schedules.sort_by_key(|s| s.effective_start);
        self.schedules = schedules;
        self.cache = HolidayCache::default();
        self.early_cache = EarlyCloseCache::default();
        self
    }

    fn schedule_for(&self, date: NaiveDate) -> ResolvedSchedule<'_> {
        let mut selected = None;
        for schedule in &self.schedules {
            if schedule.effective_start <= date {
                selected = Some(schedule);
            } else {
                break;
            }
        }
        match selected {
            Some(schedule) => ResolvedSchedule {
                weekmask: &schedule.weekmask,
                rules: &schedule.rules,
                trading_hours: schedule.trading_hours.as_ref(),
            },
            None => ResolvedSchedule {
                weekmask: &self.weekmask,
                rules: &self.rules,
                trading_hours: self.trading_hours.as_ref(),
            },
        }
    }

    fn is_holiday_uncached(&self, date: NaiveDate) -> bool {
        if self.exceptions.contains(&date) {
            return false;
        }
        let schedule = self.schedule_for(date);
        schedule
            .rules
            .iter()
            .flat_map(|rule| rule.dates_in(date.year()))
            .any(|holiday| holiday == date)
    }

    /// Cached map of `date -> local early-close time` for the given year.
    fn early_close_map(&self, year: i32) -> Arc<std::collections::HashMap<NaiveDate, NaiveTime>> {
        if let Some(m) = self.early_cache.inner.read().get(&year).cloned() {
            return m;
        }
        let mut m = std::collections::HashMap::new();
        for ec in &self.early_closes {
            if let Some(d) = ec.rule.observed_in(year) {
                // Only register if the resulting date is itself a business
                // day (skip rolled-into-weekend cases).
                let i = d.weekday().num_days_from_monday() as usize;
                if !self.schedule_for(d).weekmask[i] {
                    continue;
                }
                if self.holidays(year).contains(&d) {
                    continue;
                }
                m.insert(d, ec.close_time);
            }
        }
        let arc = Arc::new(m);
        self.early_cache.inner.write().insert(year, arc.clone());
        arc
    }

    /// Local early-close time for `date`, if any.
    pub fn early_close_for(&self, date: NaiveDate) -> Option<NaiveTime> {
        self.early_close_map(date.year()).get(&date).copied()
    }

    pub fn holidays(&self, year: i32) -> Arc<BTreeSet<NaiveDate>> {
        if let Some(h) = self.cache.inner.read().get(&year).cloned() {
            return h;
        }
        let mut set = BTreeSet::new();
        if let Some(mut d) = NaiveDate::from_ymd_opt(year, 1, 1) {
            while d.year() == year {
                if self.is_holiday_uncached(d) {
                    set.insert(d);
                }
                d += Duration::days(1);
            }
        }
        if self.adjustment == HolidayAdjustment::Japanese {
            apply_japanese_adjustment(&mut set);
        }
        let arc = Arc::new(set);
        self.cache.inner.write().insert(year, arc.clone());
        arc
    }

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
        self.schedule_for(d).weekmask[i] && !self.is_holiday(d)
    }

    pub fn next_business_day(&self, d: NaiveDate) -> NaiveDate {
        let mut x = d + Duration::days(1);
        loop {
            if self.is_business_day(x) {
                return x;
            }
            x += Duration::days(1);
        }
    }

    pub fn previous_business_day(&self, d: NaiveDate) -> NaiveDate {
        let mut x = d - Duration::days(1);
        loop {
            if self.is_business_day(x) {
                return x;
            }
            x -= Duration::days(1);
        }
    }

    pub fn business_days_between(&self, start: NaiveDate, end: NaiveDate) -> i64 {
        if end < start {
            return 0;
        }
        let mut n = 0;
        let mut d = start;
        while d <= end {
            if self.is_business_day(d) {
                n += 1;
            }
            d += Duration::days(1);
        }
        n
    }

    pub fn business_day_range(&self, start: NaiveDate, end: NaiveDate) -> Vec<NaiveDate> {
        if end < start {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(((end - start).num_days() as usize).saturating_add(1));
        let mut d = start;
        while d <= end {
            if self.is_business_day(d) {
                out.push(d);
            }
            d += Duration::days(1);
        }
        out
    }

    /// True iff the venue is currently in any trading session.
    ///
    /// For sessions that span midnight, the trading-day check considers both
    /// the local calendar day of `when` and the next local calendar day, so a
    /// Sun-evening CME open correctly maps to Mon's trading day. If an
    /// early-close is in effect for that trading day, the last session's
    /// close is shortened.
    pub fn is_open(&self, when: DateTime<Utc>) -> bool {
        let Some(th) = &self.trading_hours else {
            return false;
        };
        let local_today = when.with_timezone(&th.timezone).date_naive();
        for delta in [0i64, 1] {
            let trading_day = local_today + Duration::days(delta);
            if !self.is_business_day(trading_day) {
                continue;
            }
            let Some(th) = self.schedule_for(trading_day).trading_hours else {
                continue;
            };
            let early = self.early_close_for(trading_day);
            let last_idx = th.sessions.len().saturating_sub(1);
            for (i, s) in th.sessions.iter().enumerate() {
                let Some((o, mut c)) = s.instants(th.timezone, trading_day) else {
                    continue;
                };
                if i == last_idx {
                    if let Some(t) = early {
                        if let Some(early_c) = adjust_close(th.timezone, trading_day, s, t) {
                            c = early_c;
                        }
                    }
                }
                if when >= o && when < c {
                    return true;
                }
            }
        }
        false
    }

    pub fn next_open(&self, when: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let th = self.trading_hours.as_ref()?;
        let local_today = when.with_timezone(&th.timezone).date_naive();
        for delta in 0..400i64 {
            let trading_day = local_today + Duration::days(delta);
            if !self.is_business_day(trading_day) {
                continue;
            }
            let Some(th) = self.schedule_for(trading_day).trading_hours else {
                continue;
            };
            for s in &th.sessions {
                if let Some((o, _)) = s.instants(th.timezone, trading_day) {
                    if o >= when {
                        return Some(o);
                    }
                }
            }
        }
        None
    }

    pub fn next_close(&self, when: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let th = self.trading_hours.as_ref()?;
        let local_today = when.with_timezone(&th.timezone).date_naive();
        for delta in 0..400i64 {
            let trading_day = local_today + Duration::days(delta);
            if !self.is_business_day(trading_day) {
                continue;
            }
            let Some(th) = self.schedule_for(trading_day).trading_hours else {
                continue;
            };
            let early = self.early_close_for(trading_day);
            let last_idx = th.sessions.len().saturating_sub(1);
            for (i, s) in th.sessions.iter().enumerate() {
                let Some((_, mut c)) = s.instants(th.timezone, trading_day) else {
                    continue;
                };
                if i == last_idx {
                    if let Some(t) = early {
                        if let Some(early_c) = adjust_close(th.timezone, trading_day, s, t) {
                            c = early_c;
                        }
                    }
                }
                if c >= when {
                    return Some(c);
                }
            }
        }
        None
    }

    /// All `(open, close)` UTC instants for every business day in
    /// `[start, end]` (inclusive). Each business day contributes one entry
    /// per trading session, with the last session's close adjusted for any
    /// early-close rule. Returns an empty vector when no trading hours are
    /// configured.
    pub fn sessions_between(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
        let mut out = Vec::new();
        let mut d = start;
        while d <= end {
            if self.is_business_day(d) {
                let Some(th) = self.schedule_for(d).trading_hours else {
                    d += Duration::days(1);
                    continue;
                };
                let last_idx = th.sessions.len().saturating_sub(1);
                let early = self.early_close_for(d);
                for (i, s) in th.sessions.iter().enumerate() {
                    let Some((o, mut c)) = s.instants(th.timezone, d) else {
                        continue;
                    };
                    if i == last_idx {
                        if let Some(t) = early {
                            if let Some(early_c) = adjust_close(th.timezone, d, s, t) {
                                c = early_c;
                            }
                        }
                    }
                    out.push((o, c));
                }
            }
            d += Duration::days(1);
        }
        out
    }

    /// Named non-regular trading windows for every business day in
    /// `[start, end]` (inclusive), such as pre-open and after-close.
    pub fn extended_sessions_between(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Vec<(&'static str, DateTime<Utc>, DateTime<Utc>)> {
        let mut out = Vec::new();
        let mut d = start;
        while d <= end {
            if self.is_business_day(d) {
                let Some(th) = self.schedule_for(d).trading_hours else {
                    d += Duration::days(1);
                    continue;
                };
                let early = self.early_close_for(d);
                for s in &th.extended_sessions {
                    let Some((mut o, c)) = s.session.instants(th.timezone, d) else {
                        continue;
                    };
                    if s.name == "after_close" {
                        if let Some(t) = early {
                            if let Some(early_o) = adjust_open(th.timezone, d, &s.session, t) {
                                o = early_o;
                            }
                        }
                    }
                    if o < c {
                        out.push((s.name, o, c));
                    }
                }
            }
            d += Duration::days(1);
        }
        out
    }
}

fn adjust_open(
    tz: chrono_tz::Tz,
    trading_day: NaiveDate,
    session: &Session,
    local_open_time: NaiveTime,
) -> Option<DateTime<Utc>> {
    use chrono::TimeZone;
    let open_local_day = trading_day + Duration::days(session.open_day_offset as i64);
    let open = tz
        .from_local_datetime(&open_local_day.and_time(local_open_time))
        .single()?;
    Some(open.with_timezone(&Utc))
}

/// Recompute the close instant of `session` on `trading_day` using the
/// override `local_close_time`. The day-offset of the original close is
/// preserved so cross-midnight sessions still resolve correctly.
fn adjust_close(
    tz: chrono_tz::Tz,
    trading_day: NaiveDate,
    session: &Session,
    local_close_time: NaiveTime,
) -> Option<DateTime<Utc>> {
    use chrono::TimeZone;
    let close_local_day = trading_day + Duration::days(session.close_day_offset as i64);
    let close = tz
        .from_local_datetime(&close_local_day.and_time(local_close_time))
        .single()?;
    Some(close.with_timezone(&Utc))
}

// Holiday rule constructors

fn fixed(month: u32, day: u32, since_year: Option<i32>) -> HolidayRule {
    HolidayRule::Fixed {
        month,
        day,
        roll: WeekendRoll::NearestWeekday,
        since_year,
        until_year: None,
    }
}

fn fixed_no_roll(month: u32, day: u32, since_year: Option<i32>) -> HolidayRule {
    HolidayRule::Fixed {
        month,
        day,
        roll: WeekendRoll::None,
        since_year,
        until_year: None,
    }
}

/// Non-rolling fixed date restricted to `[since, until]` inclusive year bounds.
fn fixed_between(
    month: u32,
    day: u32,
    since_year: Option<i32>,
    until_year: Option<i32>,
) -> HolidayRule {
    HolidayRule::Fixed {
        month,
        day,
        roll: WeekendRoll::None,
        since_year,
        until_year,
    }
}

/// Fixed date rolled back to the preceding Friday when it lands on a weekend
/// (year-end closure convention at SIX, B3, BVC).
fn fixed_prev_fri(month: u32, day: u32) -> HolidayRule {
    HolidayRule::Fixed {
        month,
        day,
        roll: WeekendRoll::PrecedingFriday,
        since_year: None,
        until_year: None,
    }
}

/// Fixed date rolled forward to Monday when it lands on a weekend (UK/
/// Commonwealth substitution).
fn fixed_fwd(month: u32, day: u32, since_year: Option<i32>) -> HolidayRule {
    HolidayRule::Fixed {
        month,
        day,
        roll: WeekendRoll::ForwardMonday,
        since_year,
        until_year: None,
    }
}

fn nth(month: u32, weekday: Weekday, n: i32) -> HolidayRule {
    HolidayRule::NthWeekday {
        month,
        weekday,
        n,
        since_year: None,
        until_year: None,
    }
}

/// Nth weekday restricted to `[since, until]` inclusive year bounds.
fn nth_between(
    month: u32,
    weekday: Weekday,
    n: i32,
    since_year: Option<i32>,
    until_year: Option<i32>,
) -> HolidayRule {
    HolidayRule::NthWeekday {
        month,
        weekday,
        n,
        since_year,
        until_year,
    }
}

fn easter(offset_days: i32) -> HolidayRule {
    HolidayRule::EasterOffset {
        offset_days,
        since_year: None,
        until_year: None,
    }
}

/// Easter offset restricted to `[since, until]` inclusive year bounds.
fn easter_between(offset_days: i32, since_year: Option<i32>, until_year: Option<i32>) -> HolidayRule {
    HolidayRule::EasterOffset {
        offset_days,
        since_year,
        until_year,
    }
}

/// Christmas Day + Boxing Day with UK/Commonwealth weekend substitution.
fn christmas_boxing() -> HolidayRule {
    consecutive_pair(12, 25, WeekendRoll::ForwardMonday)
}

/// A consecutive-day holiday pair observed via `roll` with collision bump.
fn consecutive_pair(month: u32, day: u32, roll: WeekendRoll) -> HolidayRule {
    HolidayRule::ConsecutivePair {
        month,
        day,
        roll,
        since_year: None,
        until_year: None,
    }
}

/// Fixed date substituted to Monday only when it lands on a Sunday (South Africa).
fn fixed_sun(month: u32, day: u32, since_year: Option<i32>) -> HolidayRule {
    HolidayRule::Fixed {
        month,
        day,
        roll: WeekendRoll::SundayToMonday,
        since_year,
        until_year: None,
    }
}

/// Latest `weekday` on or before `month`/`day` (e.g. Victoria Day).
fn weekday_on_or_before(month: u32, day: u32, weekday: Weekday, since_year: Option<i32>) -> HolidayRule {
    HolidayRule::WeekdayOnOrBefore {
        month,
        day,
        weekday,
        since_year,
        until_year: None,
    }
}

/// Apply Japanese substitute and citizens' holidays to a computed holiday set.
fn apply_japanese_adjustment(set: &mut BTreeSet<NaiveDate>) {
    let is_exchange_only = |d: &NaiveDate| {
        (d.month() == 1 && (d.day() == 2 || d.day() == 3)) || (d.month() == 12 && d.day() == 31)
    };
    // National holidays only (exclude the exchange-only year-end/new-year days).
    let mut national: BTreeSet<NaiveDate> =
        set.iter().filter(|d| !is_exchange_only(d)).cloned().collect();

    // Substitute holiday: a national holiday on Sunday moves to the next day
    // that is not already a national holiday.
    let sundays: Vec<NaiveDate> = national
        .iter()
        .filter(|d| d.weekday() == Weekday::Sun)
        .cloned()
        .collect();
    for d in sundays {
        let mut e = d + Duration::days(1);
        while national.contains(&e) {
            e += Duration::days(1);
        }
        national.insert(e);
    }

    // Citizens' holiday: a single weekday sandwiched between two national holidays.
    let extra: Vec<NaiveDate> = national
        .iter()
        .filter_map(|d| {
            let mid = *d + Duration::days(1);
            let next = *d + Duration::days(2);
            if !national.contains(&mid) && national.contains(&next) && mid.weekday() != Weekday::Sun
            {
                Some(mid)
            } else {
                None
            }
        })
        .collect();
    national.extend(extra);

    set.extend(national);
}

/// Colombian Emiliani-law holiday: observed the Monday on or after `month`/`day`.
fn emiliani(month: u32, day: u32) -> HolidayRule {
    HolidayRule::WeekdayOnOrAfter {
        month,
        day,
        weekday: Weekday::Mon,
        since_year: None,
        until_year: None,
    }
}

// Built-in calendars

/// Ad-hoc, unscheduled full-day US equity/derivative closures (national days
/// of mourning, 9/11, Hurricane Sandy). These do not follow any recurring rule
/// and must be tabulated explicitly. Shared by NYSE/NASDAQ and CFE (Cboe).
static US_SPECIAL_CLOSURES: &[(i32, u32, u32)] = &[
    (2001, 9, 11), // September 11 attacks
    (2001, 9, 12),
    (2001, 9, 13),
    (2001, 9, 14),
    (2004, 6, 11),  // President Reagan, day of mourning
    (2007, 1, 2),   // President Ford, day of mourning
    (2012, 10, 29), // Hurricane Sandy
    (2012, 10, 30),
    (2018, 12, 5), // President G.H.W. Bush, day of mourning
    (2025, 1, 9),  // President Carter, day of mourning
];

fn nyse_rules() -> Vec<HolidayRule> {
    vec![
        fixed(1, 1, None),
        nth(1, Weekday::Mon, 3),
        nth(2, Weekday::Mon, 3),
        easter(-2),
        nth(5, Weekday::Mon, -1),
        fixed(6, 19, Some(2022)),
        fixed(7, 4, None),
        nth(9, Weekday::Mon, 1),
        nth(11, Weekday::Thu, 4),
        fixed(12, 25, None),
        HolidayRule::Tabulated {
            table: US_SPECIAL_CLOSURES,
        },
    ]
}

fn nyse_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        chrono_tz::America::New_York,
    )
    .with_extended_sessions(vec![
        ExtendedSession::new(
            "pre_open",
            Session::regular(
                NaiveTime::from_hms_opt(4, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
            ),
        ),
        ExtendedSession::new(
            "after_close",
            Session::regular(
                NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            ),
        ),
    ])
}

/// Listed options use the same holidays as NYSE; market hours run from 09:30
/// to 16:15 ET (index options often 16:15, single-name 16:00). We pick 16:15
/// as the default close to maximize coverage.
fn options_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 15, 0).unwrap(),
        chrono_tz::America::New_York,
    )
}

/// CME Globex baseline holidays — only days when *all* products close:
/// New Year's Day, Good Friday, Christmas. Product-specific closures
/// (Memorial Day, Independence Day, Thanksgiving, …) are typically partial
/// closes / early closes which this layer does not yet model.
fn cme_globex_rules() -> Vec<HolidayRule> {
    vec![fixed(1, 1, None), easter(-2), fixed(12, 25, None)]
}

/// CME Globex equity-index, FX, fixed-income futures: 17:00 prev — 16:00 today CT.
fn cme_globex_overnight_hours() -> TradingHours {
    TradingHours::from_sessions(
        vec![Session::overnight(
            NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        )],
        chrono_tz::America::Chicago,
    )
}

/// CME Globex energy & metals: 17:00 prev — 16:00 today CT. The 16:00-17:00
/// daily maintenance break appears as the gap before the next trading day's
/// overnight session. CME WTI Crude Oil contract specs page checked 2026-05-25.
fn cme_globex_energy_hours() -> TradingHours {
    TradingHours::from_sessions(
        vec![Session::overnight(
            NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        )],
        chrono_tz::America::Chicago,
    )
}

/// CBOT grain and oilseed futures: evening session 19:00 prev — 07:45 today CT,
/// then day session 08:30 — 13:20 CT. This models the common agricultural
/// futures split rather than the broad CME financial-futures template. CME Corn
/// contract specs page checked 2026-05-25.
fn cbot_grain_futures_hours() -> TradingHours {
    TradingHours::from_sessions(
        vec![
            Session {
                open: NaiveTime::from_hms_opt(19, 0, 0).unwrap(),
                open_day_offset: -1,
                close: NaiveTime::from_hms_opt(7, 45, 0).unwrap(),
                close_day_offset: 0,
            },
            Session::regular(
                NaiveTime::from_hms_opt(8, 30, 0).unwrap(),
                NaiveTime::from_hms_opt(13, 20, 0).unwrap(),
            ),
        ],
        chrono_tz::America::Chicago,
    )
}

/// CME livestock futures (Live Cattle / Feeder Cattle / Lean Hogs):
/// 08:30-13:05 CT regular trading session. Source: local
/// `cme/Trading Hours Export.xlsx` row set for LE/GF/HE captured 2026-05-25.
fn cme_livestock_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(8, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(13, 5, 0).unwrap(),
        chrono_tz::America::Chicago,
    )
}

/// CME lumber futures regular session: 09:00-15:05 CT.
/// Source: local `cme/Trading Hours Export.xlsx` row for LBR captured 2026-05-25.
fn cme_lumber_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 5, 0).unwrap(),
        chrono_tz::America::Chicago,
    )
}

/// CBOE Futures Exchange (CFE) — full US holiday set, 08:30–15:15 CT regular.
fn cfe_rules() -> Vec<HolidayRule> {
    vec![
        fixed(1, 1, None),
        nth(1, Weekday::Mon, 3),
        nth(2, Weekday::Mon, 3),
        easter(-2),
        nth(5, Weekday::Mon, -1),
        fixed(6, 19, Some(2022)),
        fixed(7, 4, None),
        nth(9, Weekday::Mon, 1),
        nth(11, Weekday::Thu, 4),
        fixed(12, 25, None),
        HolidayRule::Tabulated {
            table: US_SPECIAL_CLOSURES,
        },
    ]
}

fn cfe_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(8, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 15, 0).unwrap(),
        chrono_tz::America::Chicago,
    )
}

/// ICE US futures (energy, softs): 20:00 prev — 18:00 today ET.
fn ice_us_rules() -> Vec<HolidayRule> {
    vec![fixed(1, 1, None), easter(-2), fixed(12, 25, None)]
}

fn ice_us_hours() -> TradingHours {
    TradingHours::from_sessions(
        vec![Session::overnight(
            NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
        )],
        chrono_tz::America::New_York,
    )
}

/// SIFMA US bond market — recommended fixed-income closures.
fn sifma_us_rules() -> Vec<HolidayRule> {
    vec![
        fixed(1, 1, None),
        nth(1, Weekday::Mon, 3),
        nth(2, Weekday::Mon, 3),
        easter(-2),
        nth(5, Weekday::Mon, -1),
        fixed(6, 19, Some(2022)),
        fixed(7, 4, None),
        nth(9, Weekday::Mon, 1),
        nth(10, Weekday::Mon, 2), // Columbus Day
        fixed(11, 11, None),      // Veterans Day
        nth(11, Weekday::Thu, 4),
        fixed(12, 25, None),
    ]
}

fn sifma_us_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 30, 0).unwrap(),
        chrono_tz::America::New_York,
    )
}

/// 24x5 spot FX — opens Sun 17:00 NY, closes Fri 17:00 NY. New Year's Day +
/// Christmas remain closed; otherwise no holidays.
fn forex_rules() -> Vec<HolidayRule> {
    vec![fixed(1, 1, None), fixed(12, 25, None)]
}

/// 24x7 crypto — no holidays.
fn crypto_rules() -> Vec<HolidayRule> {
    vec![]
}

/// One-off LSE closures and moved bank holidays (our own record of UK events;
/// pmc is used only to cross-check).
static LSE_ONE_OFFS: &[(i32, u32, u32)] = &[
    (2020, 5, 8),  // VE Day 75th anniversary (early May BH moved from May 4)
    (2022, 6, 2),  // Spring bank holiday moved here for the Platinum Jubilee
    (2022, 6, 3),  // Platinum Jubilee extra bank holiday
    (2022, 9, 19), // State funeral of Queen Elizabeth II
    (2023, 5, 8),  // Coronation of King Charles III
];

/// Dates the recurring bank-holiday rules produce but that were moved elsewhere
/// in that year (so the exchange actually traded on them).
static LSE_MOVED: &[(i32, u32, u32)] = &[
    (2020, 5, 4),  // Early May BH moved to May 8
    (2022, 5, 30), // Spring BH moved to Jun 2
];

fn lse_rules() -> Vec<HolidayRule> {
    vec![
        fixed_fwd(1, 1, None),        // New Year (substitute to Monday)
        easter(-2),                   // Good Friday
        easter(1),                    // Easter Monday
        nth(5, Weekday::Mon, 1),      // Early May bank holiday
        nth(5, Weekday::Mon, -1),     // Spring bank holiday
        nth(8, Weekday::Mon, -1),     // Summer bank holiday
        christmas_boxing(),           // Christmas + Boxing (substitute)
        HolidayRule::Tabulated { table: LSE_ONE_OFFS },
    ]
}

fn lse_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 30, 0).unwrap(),
        chrono_tz::Europe::London,
    )
}

/// Japanese vernal / autumnal equinox rule constructor.
fn jp_equinox(spring: bool) -> HolidayRule {
    HolidayRule::JapaneseEquinox {
        spring,
        since_year: None,
        until_year: None,
    }
}

/// One-off JPX closures: 2019 imperial transition and the Tokyo 2020 Olympics
/// holiday moves (Games held in 2021). Substitute/citizens' days are derived
/// automatically by the Japanese adjustment.
static TSE_ONE_OFFS: &[(i32, u32, u32)] = &[
    (2019, 5, 1),   // Enthronement of Emperor Naruhito
    (2019, 10, 22), // Enthronement ceremony
    (2020, 7, 23),  // Marine Day (moved for Olympics)
    (2020, 7, 24),  // Sports Day (moved)
    (2020, 8, 10),  // Mountain Day (moved)
    (2020, 10, 1),  // Full-day outage (Arrowhead system failure)
    (2021, 7, 22),  // Marine Day (moved)
    (2021, 7, 23),  // Sports Day (moved)
    (2021, 8, 8),   // Mountain Day (moved)
];

/// Normal Happy-Monday dates suppressed in the Olympic years (moved above).
static TSE_MOVED: &[(i32, u32, u32)] = &[
    (2020, 7, 20),
    (2020, 10, 12),
    (2020, 8, 11),
    (2021, 7, 19),
    (2021, 10, 11),
    (2021, 8, 11),
];

fn tse_rules() -> Vec<HolidayRule> {
    vec![
        fixed_no_roll(1, 1, None),
        fixed_no_roll(1, 2, None),
        fixed_no_roll(1, 3, None),
        fixed_between(1, 15, None, Some(1999)), // Coming of Age (fixed pre-2000)
        nth_between(1, Weekday::Mon, 2, Some(2000), None), // Coming of Age (2nd Mon)
        fixed_no_roll(2, 11, None),             // National Foundation Day
        fixed_between(12, 23, Some(1989), Some(2018)), // Emperor Akihito's birthday
        fixed_no_roll(2, 23, Some(2020)),       // Emperor Naruhito's birthday
        jp_equinox(true),                       // Vernal Equinox
        fixed_no_roll(4, 29, None),             // Showa Day / Greenery Day
        fixed_no_roll(5, 3, None),              // Constitution Memorial Day
        fixed_no_roll(5, 4, Some(2007)),        // Greenery Day (citizens' day before 2007)
        fixed_no_roll(5, 5, None),              // Children's Day
        fixed_between(7, 20, Some(1996), Some(2002)), // Marine Day (fixed)
        nth_between(7, Weekday::Mon, 3, Some(2003), None), // Marine Day (3rd Mon)
        fixed_no_roll(8, 11, Some(2016)),       // Mountain Day (since 2016)
        fixed_between(9, 15, None, Some(2002)), // Respect for the Aged (fixed)
        nth_between(9, Weekday::Mon, 3, Some(2003), None), // Respect for the Aged (3rd Mon)
        jp_equinox(false),                      // Autumnal Equinox
        fixed_between(10, 10, None, Some(1999)), // Health-Sports Day (fixed)
        nth_between(10, Weekday::Mon, 2, Some(2000), None), // Sports Day (2nd Mon)
        fixed_no_roll(11, 3, None),             // Culture Day
        fixed_no_roll(11, 23, None),            // Labour Thanksgiving Day
        fixed_no_roll(12, 31, None),            // Exchange year-end
        HolidayRule::Tabulated { table: TSE_ONE_OFFS },
    ]
}

fn tse_trading_hours_with_afternoon_close(close: NaiveTime) -> TradingHours {
    TradingHours::from_sessions(
        vec![
            Session::regular(
                NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(11, 30, 0).unwrap(),
            ),
            Session::regular(NaiveTime::from_hms_opt(12, 30, 0).unwrap(), close),
        ],
        chrono_tz::Asia::Tokyo,
    )
}

fn tse_trading_hours() -> TradingHours {
    // JPX domestic stock auction trading since 2024-11-05: morning
    // 09:00-11:30, afternoon 12:30-15:30. Source checked 2026-05-25.
    tse_trading_hours_with_afternoon_close(NaiveTime::from_hms_opt(15, 30, 0).unwrap())
}

fn tse_historical_trading_hours() -> TradingHours {
    // JPX domestic stock auction trading before the 2024-11-05 close extension.
    tse_trading_hours_with_afternoon_close(NaiveTime::from_hms_opt(15, 0, 0).unwrap())
}

fn tse_schedules() -> Vec<CalendarSchedule> {
    vec![
        CalendarSchedule::new(
            NaiveDate::from_ymd_opt(1900, 1, 1).unwrap(),
            STANDARD_WEEKMASK,
            tse_rules(),
            Some(tse_historical_trading_hours()),
        ),
        CalendarSchedule::new(
            NaiveDate::from_ymd_opt(2024, 11, 5).unwrap(),
            STANDARD_WEEKMASK,
            tse_rules(),
            Some(tse_trading_hours()),
        ),
    ]
}

fn hkex_rules() -> Vec<HolidayRule> {
    let lny: &'static [(i32, u32, u32)] = &[
        (2020, 1, 27),
        (2021, 2, 12),
        (2022, 2, 1),
        (2023, 1, 23),
        (2024, 2, 12),
        (2025, 1, 29),
        (2026, 2, 17),
        (2027, 2, 8),
        (2028, 1, 26),
        (2029, 2, 13),
        (2030, 2, 4),
    ];
    vec![
        fixed(1, 1, None),
        HolidayRule::Tabulated { table: lny },
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        fixed(7, 1, None),
        fixed(10, 1, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
    ]
}

fn hkex_trading_hours() -> TradingHours {
    // HKEX securities market continuous trading: morning 09:30-12:00,
    // afternoon 13:00-16:00. Source checked 2026-05-25.
    TradingHours::from_sessions(
        vec![
            Session::regular(
                NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
                NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
            ),
            Session::regular(
                NaiveTime::from_hms_opt(13, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            ),
        ],
        chrono_tz::Asia::Hong_Kong,
    )
}

fn sse_rules() -> Vec<HolidayRule> {
    let lny: &'static [(i32, u32, u32)] = &[
        (2020, 1, 25),
        (2021, 2, 12),
        (2022, 2, 1),
        (2023, 1, 22),
        (2024, 2, 10),
        (2025, 1, 29),
        (2026, 2, 17),
        (2027, 2, 6),
        (2028, 1, 26),
        (2029, 2, 13),
        (2030, 2, 3),
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
    // SSE stocks: continuous auction 09:30-11:30 and 13:00-14:57,
    // followed by the 14:57-15:00 closing call auction; modeled as an
    // afternoon regular session through 15:00. Source checked 2026-05-25.
    TradingHours::from_sessions(
        vec![
            Session::regular(
                NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
                NaiveTime::from_hms_opt(11, 30, 0).unwrap(),
            ),
            Session::regular(
                NaiveTime::from_hms_opt(13, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
            ),
        ],
        chrono_tz::Asia::Shanghai,
    )
}

/// One-off Xetra closure: Reformation Day 500th anniversary (nationwide 2017).
static XETR_ONE_OFFS: &[(i32, u32, u32)] = &[(2017, 10, 31)];

fn xetra_rules() -> Vec<HolidayRule> {
    // Xetra/Frankfurt: NY, Good Friday, Easter Monday, Labour Day, and the
    // festive block Dec 24-26 + Dec 31. Whit Monday was observed through 2021;
    // German Unity Day only 2016-2019.
    vec![
        fixed_no_roll(1, 1, None),
        easter(-2),
        easter(1),
        fixed_no_roll(5, 1, None),
        easter_between(50, None, Some(2021)),   // Whit Monday (until 2021)
        fixed_between(10, 3, Some(2016), Some(2019)), // German Unity Day (2016-2019)
        fixed_no_roll(12, 24, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
        fixed_no_roll(12, 31, None),
        HolidayRule::Tabulated { table: XETR_ONE_OFFS },
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
    // Euronext does not observe substitute days: a holiday falling on a weekend
    // is simply lost, so all fixed dates use no-roll.
    vec![
        fixed_no_roll(1, 1, None),
        easter(-2),
        easter(1),
        fixed_no_roll(5, 1, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
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
        fixed_fwd(1, 1, None),                          // New Year (substitute)
        nth_between(2, Weekday::Mon, 3, Some(2008), None), // Family Day (since 2008)
        easter(-2),                                     // Good Friday
        weekday_on_or_before(5, 24, Weekday::Mon, None), // Victoria Day
        fixed_fwd(7, 1, None),                          // Canada Day (substitute)
        nth(8, Weekday::Mon, 1),                        // Civic Holiday
        nth(9, Weekday::Mon, 1),                        // Labour Day
        nth(10, Weekday::Mon, 2),                       // Thanksgiving
        christmas_boxing(),                             // Christmas + Boxing
    ]
}

fn tsx_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        chrono_tz::America::Toronto,
    )
}

/// One-off ASX closure: national day of mourning for Queen Elizabeth II.
static ASX_ONE_OFFS: &[(i32, u32, u32)] = &[(2022, 9, 22)];

fn asx_rules() -> Vec<HolidayRule> {
    vec![
        fixed_fwd(1, 1, None),      // New Year (substitute)
        fixed_fwd(1, 26, None),     // Australia Day (substitute)
        easter(-2),                 // Good Friday
        easter(1),                  // Easter Monday
        fixed_no_roll(4, 25, None), // ANZAC Day (no substitute)
        nth(6, Weekday::Mon, 2),    // King's Birthday (2nd Mon Jun)
        christmas_boxing(),         // Christmas + Boxing
        HolidayRule::Tabulated { table: ASX_ONE_OFFS },
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

// Early-close rule helpers

fn ec(rule: HolidayRule, h: u32, m: u32) -> EarlyCloseRule {
    EarlyCloseRule {
        rule,
        close_time: NaiveTime::from_hms_opt(h, m, 0).unwrap(),
    }
}

/// NYSE/NASDAQ early closes (13:00 ET):
/// - Day after Thanksgiving (Black Friday)
/// - Christmas Eve when it falls on a weekday (Dec 24)
/// - July 3 when it falls on a weekday (day before Independence Day)
///
/// These are "best-effort" rules; the SEC/NYSE may publish ad-hoc deviations.
fn nyse_early_closes() -> Vec<EarlyCloseRule> {
    // Black Friday is the day after the 4th Thursday of November (i.e.
    // Thanksgiving + 1). Tabulated through 2035 — easily extended.
    static BLACK_FRIDAY: &[(i32, u32, u32)] = &[
        (2020, 11, 27),
        (2021, 11, 26),
        (2022, 11, 25),
        (2023, 11, 24),
        (2024, 11, 29),
        (2025, 11, 28),
        (2026, 11, 27),
        (2027, 11, 26),
        (2028, 11, 24),
        (2029, 11, 23),
        (2030, 11, 29),
        (2031, 11, 28),
        (2032, 11, 26),
        (2033, 11, 25),
        (2034, 11, 24),
        (2035, 11, 23),
    ];
    vec![
        ec(
            HolidayRule::Tabulated {
                table: BLACK_FRIDAY,
            },
            13,
            0,
        ),
        ec(fixed_no_roll(12, 24, None), 13, 0),
        ec(fixed_no_roll(7, 3, None), 13, 0),
    ]
}

// Additional non-US equity calendars

/// Generic European Christian-calendar holidays: NY, Good Friday,
/// Easter Monday, May Day, Christmas, Boxing Day. Used as a baseline.
fn euro_basic_rules() -> Vec<HolidayRule> {
    // Continental European exchanges do not observe substitute days.
    vec![
        fixed_no_roll(1, 1, None),
        easter(-2),
        easter(1),
        fixed_no_roll(5, 1, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
    ]
}

/// Euronext Amsterdam: same hours as Paris/Brussels/Lisbon (09:00–17:30 CET).
fn euronext_hours(tz: chrono_tz::Tz) -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 30, 0).unwrap(),
        tz,
    )
}

// Euronext harmonized its calendar: Amsterdam, Brussels and Lisbon observe the
// same six exchange holidays as Paris (NY, Good Friday, Easter Monday, May 1,
// Christmas, Boxing Day). National holidays (King's Day, Ascension, Whit
// Monday, Carnival) are not exchange closures.
fn xams_rules() -> Vec<HolidayRule> {
    euro_basic_rules()
}

fn xbru_rules() -> Vec<HolidayRule> {
    euro_basic_rules()
}

fn xlis_rules() -> Vec<HolidayRule> {
    euro_basic_rules()
}

fn xmil_rules() -> Vec<HolidayRule> {
    // Euronext Milan trades the harmonized Euronext holidays plus Assumption
    // (Aug 15) and the festive block Dec 24-26 + 31. It does not close for the
    // other Italian national holidays.
    vec![
        fixed_no_roll(1, 1, None),
        easter(-2),
        easter(1),
        fixed_no_roll(5, 1, None),
        fixed_no_roll(8, 15, None),
        fixed_no_roll(12, 24, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
        fixed_no_roll(12, 31, None),
    ]
}

fn xmil_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 30, 0).unwrap(),
        chrono_tz::Europe::Rome,
    )
}

fn xmad_rules() -> Vec<HolidayRule> {
    // BME Madrid closes NY, Good Friday, Easter Monday, Labour Day, Christmas
    // and Boxing Day only; it trades through the other Spanish national holidays.
    vec![
        fixed_no_roll(1, 1, None),
        easter(-2),
        easter(1),
        fixed_no_roll(5, 1, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
        HolidayRule::Tabulated { table: XMAD_ONE_OFFS },
    ]
}

/// One-off BME Madrid closures (festive-season closures in 2021).
static XMAD_ONE_OFFS: &[(i32, u32, u32)] = &[(2021, 12, 24), (2021, 12, 31)];

fn xmad_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 30, 0).unwrap(),
        chrono_tz::Europe::Madrid,
    )
}

fn xswx_rules() -> Vec<HolidayRule> {
    // SIX Swiss: NY, Berchtold (Jan 2), Good Friday, Easter Mon, Labour,
    // Ascension, Whit Mon, Swiss National (Aug 1), festive block Dec 24-26 + 31.
    vec![
        fixed_no_roll(1, 1, None),
        fixed_no_roll(1, 2, None),
        easter(-2),
        easter(1),
        fixed_no_roll(5, 1, None),
        easter(39),
        easter(50),
        fixed_no_roll(8, 1, None),
        fixed_no_roll(12, 24, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
        fixed_prev_fri(12, 31), // New Year's Eve (preceding Friday if weekend)
    ]
}

fn xswx_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 30, 0).unwrap(),
        chrono_tz::Europe::Zurich,
    )
}

fn xosl_rules() -> Vec<HolidayRule> {
    // Oslo Børs: NY, Maundy Thu, Good Friday, Easter Mon, Labour, Constitution
    // (May 17), Ascension, Whit Mon, festive block Dec 24-26 + 31.
    vec![
        fixed_no_roll(1, 1, None),
        easter(-3),
        easter(-2),
        easter(1),
        fixed_no_roll(5, 1, None),
        fixed_no_roll(5, 17, None),
        easter(39),
        easter(50),
        fixed_no_roll(12, 24, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
        fixed_no_roll(12, 31, None),
    ]
}

fn xosl_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 20, 0).unwrap(),
        chrono_tz::Europe::Oslo,
    )
}

fn xsto_rules() -> Vec<HolidayRule> {
    // Stockholm OMX: NY, Epiphany, Good Friday, Easter Mon, Labour, Ascension,
    // National Day (Jun 6, since 2005), Midsummer Eve (Fri on/before Jun 25),
    // festive block Dec 24-26 + 31.
    vec![
        fixed_no_roll(1, 1, None),
        fixed_no_roll(1, 6, None),
        easter(-2),
        easter(1),
        fixed_no_roll(5, 1, None),
        easter(39),
        fixed_no_roll(6, 6, Some(2005)),
        weekday_on_or_before(6, 25, Weekday::Fri, None), // Midsummer Eve
        fixed_no_roll(12, 24, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
        fixed_no_roll(12, 31, None),
    ]
}

fn xsto_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 30, 0).unwrap(),
        chrono_tz::Europe::Stockholm,
    )
}

fn xhel_rules() -> Vec<HolidayRule> {
    // Helsinki: NY, Epiphany, Good Friday, Easter Mon, Labour, Ascension,
    // Midsummer Eve (Fri on/before Jun 25), Independence Day (Dec 6),
    // festive block Dec 24-26 + 31.
    vec![
        fixed_no_roll(1, 1, None),
        fixed_no_roll(1, 6, None),
        easter(-2),
        easter(1),
        fixed_no_roll(5, 1, None),
        easter(39),
        weekday_on_or_before(6, 25, Weekday::Fri, None), // Midsummer Eve
        fixed_no_roll(12, 6, None),
        fixed_no_roll(12, 24, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
        fixed_no_roll(12, 31, None),
    ]
}

fn xhel_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(18, 30, 0).unwrap(),
        chrono_tz::Europe::Helsinki,
    )
}

fn xcse_rules() -> Vec<HolidayRule> {
    // Copenhagen: NY, Maundy Thu, Good Friday, Easter Mon, Great Prayer Day
    // (Easter+26, abolished after 2023), Ascension, Day after Ascension,
    // Whit Monday, Constitution (Jun 5), festive block Dec 24-26 + 31.
    vec![
        fixed_no_roll(1, 1, None),
        easter(-3),
        easter(-2),
        easter(1),
        easter_between(26, None, Some(2023)), // Great Prayer Day (until 2023)
        easter(39),
        easter(40), // Day after Ascension (bank closing day)
        easter(50), // Whit Monday
        fixed_no_roll(6, 5, None),
        fixed_no_roll(12, 24, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
        fixed_no_roll(12, 31, None),
    ]
}

fn xcse_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        chrono_tz::Europe::Copenhagen,
    )
}

fn xice_rules() -> Vec<HolidayRule> {
    // Iceland: NY, Maundy Thu, Good Fri, Easter Mon, First Day of Summer (Thu
    // on/before Apr 25), Labour, Ascension, Whit Mon, National Day (Jun 17),
    // Commerce Day (1st Mon Aug), festive block Dec 24-26 + 31.
    vec![
        fixed_no_roll(1, 1, None),
        easter(-3),
        easter(-2),
        easter(1),
        weekday_on_or_before(4, 25, Weekday::Thu, None), // First Day of Summer
        fixed_no_roll(5, 1, None),
        easter(39),
        easter(50),
        fixed_no_roll(6, 17, None),
        nth(8, Weekday::Mon, 1), // Commerce Day
        fixed_no_roll(12, 24, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
        fixed_no_roll(12, 31, None),
    ]
}

fn xice_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 30, 0).unwrap(),
        chrono_tz::Atlantic::Reykjavik,
    )
}

fn xwar_rules() -> Vec<HolidayRule> {
    // Warsaw (GPW): NY, Epiphany, Good Friday, Easter Mon, Labour, Constitution
    // (May 3), Corpus Christi (+60), Assumption, All Saints, Independence
    // (Nov 11), festive block Dec 24-26 + 31.
    vec![
        fixed_no_roll(1, 1, None),
        fixed_no_roll(1, 6, None),
        easter(-2),
        easter(1),
        fixed_no_roll(5, 1, None),
        fixed_no_roll(5, 3, None),
        easter(60),
        fixed_no_roll(8, 15, None),
        fixed_no_roll(11, 1, None),
        fixed_no_roll(11, 11, None),
        fixed_no_roll(12, 24, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
        fixed_no_roll(12, 31, None),
        HolidayRule::Tabulated { table: XWAR_ONE_OFFS },
    ]
}

/// One-off GPW Warsaw closures: an exchange holiday in Jan 2018 and the
/// centenary of Polish independence (Nov 12, 2018).
static XWAR_ONE_OFFS: &[(i32, u32, u32)] = &[(2018, 1, 2), (2018, 11, 12)];

fn xwar_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        chrono_tz::Europe::Warsaw,
    )
}

fn xpra_rules() -> Vec<HolidayRule> {
    // Prague (PSE): NY, Good Friday (public holiday since 2016), Easter Mon,
    // Labour, Liberation (May 8), Ss Cyril & Methodius (Jul 5), Jan Hus (Jul 6),
    // Statehood (Sep 28), Independence (Oct 28), Freedom (Nov 17), festive block
    // Dec 24-26 + 31.
    vec![
        fixed_no_roll(1, 1, None),
        easter(-2), // Good Friday (PSE closed even before it became a public holiday)
        easter(1),
        fixed_no_roll(5, 1, None),
        fixed_no_roll(5, 8, None),
        fixed_no_roll(7, 5, None),
        fixed_no_roll(7, 6, None),
        fixed_no_roll(9, 28, None),
        fixed_no_roll(10, 28, None),
        fixed_no_roll(11, 17, None),
        fixed_no_roll(12, 24, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
        fixed_no_roll(12, 31, None),
    ]
}

fn xpra_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 25, 0).unwrap(),
        chrono_tz::Europe::Prague,
    )
}

fn xbud_rules() -> Vec<HolidayRule> {
    // Budapest (BSE): NY, 1848 Revolution (Mar 15), Good Friday, Easter Mon,
    // Labour, Whit Mon, State Foundation (Aug 20), 1956 Revolution (Oct 23),
    // All Saints, festive block Dec 24-26 + 31. Hungary also observes irregular
    // "bridge days" that swap a working day for a long weekend; these are
    // announced annually and tabulated below.
    vec![
        fixed_no_roll(1, 1, None),
        fixed_no_roll(3, 15, None),
        easter(-2),
        easter(1),
        fixed_no_roll(5, 1, None),
        easter(50),
        fixed_no_roll(8, 20, None),
        fixed_no_roll(10, 23, None),
        fixed_no_roll(11, 1, None),
        fixed_no_roll(12, 24, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
        fixed_no_roll(12, 31, None),
        HolidayRule::Tabulated { table: XBUD_BRIDGE_DAYS },
    ]
}

/// Hungarian "bridge days" (munkaszüneti nap around a public holiday) — annually
/// announced, not derivable from a rule. Filled from the exchange calendar.
static XBUD_BRIDGE_DAYS: &[(i32, u32, u32)] = &[
    (2015, 1, 2),
    (2015, 8, 21),
    (2016, 3, 14),
    (2016, 10, 31),
    (2018, 3, 16),
    (2018, 4, 30),
    (2018, 10, 22),
    (2018, 11, 2),
    (2019, 8, 19),
    (2019, 12, 27),
    (2020, 8, 21),
    (2022, 3, 14),
    (2022, 10, 31),
    (2024, 8, 19),
    (2024, 12, 27),
    (2025, 5, 2),
    (2025, 10, 24),
    (2026, 1, 2),
    (2026, 8, 21),
    (2029, 3, 16),
    (2029, 4, 30),
    (2029, 10, 22),
    (2029, 11, 2),
    (2030, 8, 19),
    (2030, 12, 27),
];

fn xbud_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        chrono_tz::Europe::Budapest,
    )
}

fn xwbo_rules() -> Vec<HolidayRule> {
    // Wiener Börse trimmed its calendar during 2018-2022 to the current set: NY,
    // Good Friday, Easter Monday, Labour Day, National Day (Oct 26), and the
    // festive block. Whit Monday was observed through 2022; Corpus Christi,
    // Assumption, All Saints and Immaculate Conception only through 2018.
    vec![
        fixed_no_roll(1, 1, None),
        easter(-2),
        easter(1),
        fixed_no_roll(5, 1, None),
        fixed_between(1, 6, None, Some(2017)), // Epiphany (until 2017)
        easter_between(39, None, Some(2018)), // Ascension (until 2018)
        easter_between(50, None, Some(2022)), // Whit Monday (until 2022)
        easter_between(60, None, Some(2018)), // Corpus Christi (until 2018)
        fixed_between(8, 15, None, Some(2018)), // Assumption (until 2018)
        fixed_between(11, 1, None, Some(2018)), // All Saints (until 2018)
        fixed_between(12, 8, None, Some(2018)), // Immaculate Conception (until 2018)
        fixed_no_roll(10, 26, None),          // National Day
        fixed_no_roll(12, 24, None),
        fixed_no_roll(12, 25, None),
        fixed_no_roll(12, 26, None),
        fixed_no_roll(12, 31, None),
    ]
}

fn xwbo_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 30, 0).unwrap(),
        chrono_tz::Europe::Vienna,
    )
}

/// One-off Euronext Dublin closure: Storm Emma.
static XDUB_ONE_OFFS: &[(i32, u32, u32)] = &[(2018, 3, 2)];

fn xdub_rules() -> Vec<HolidayRule> {
    // Dublin migrated from the Irish Stock Exchange calendar to Euronext during
    // 2018-2021. Pre-migration it closed the Irish June bank holiday and the May
    // bank holiday; from 2019 it added the Euronext Labour Day (May 1), and the
    // Irish May bank holiday resumed from 2021. It never closes St Patrick's Day.
    vec![
        fixed_fwd(1, 1, None),                          // New Year (substitute)
        easter(-2),                                     // Good Friday
        easter(1),                                      // Easter Monday
        fixed_no_roll(5, 1, Some(2019)),                // Labour Day (Euronext) since 2019
        nth_between(5, Weekday::Mon, 1, None, Some(2018)), // May bank holiday (pre-migration)
        nth_between(5, Weekday::Mon, 1, Some(2021), None), // May bank holiday (resumed 2021)
        nth_between(6, Weekday::Mon, 1, None, Some(2018)), // June bank holiday (pre-migration)
        christmas_boxing(),                             // Christmas + Boxing
        HolidayRule::Tabulated { table: XDUB_ONE_OFFS },
    ]
}

fn xdub_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 28, 0).unwrap(),
        chrono_tz::Europe::Dublin,
    )
}

// Asia / Pacific

fn xkrx_rules() -> Vec<HolidayRule> {
    // Korea Exchange: tabulated lunar holidays (Seollal, Chuseok). For
    // accuracy these are baked in as lookup tables 2020-2030.
    let seollal: &'static [(i32, u32, u32)] = &[
        (2020, 1, 24),
        (2020, 1, 27),
        (2021, 2, 11),
        (2021, 2, 12),
        (2022, 1, 31),
        (2022, 2, 1),
        (2022, 2, 2),
        (2023, 1, 23),
        (2023, 1, 24),
        (2024, 2, 9),
        (2024, 2, 12),
        (2025, 1, 28),
        (2025, 1, 29),
        (2025, 1, 30),
        (2026, 2, 16),
        (2026, 2, 17),
        (2026, 2, 18),
    ];
    let chuseok: &'static [(i32, u32, u32)] = &[
        (2020, 9, 30),
        (2020, 10, 1),
        (2020, 10, 2),
        (2021, 9, 20),
        (2021, 9, 21),
        (2021, 9, 22),
        (2022, 9, 9),
        (2022, 9, 12),
        (2023, 9, 28),
        (2023, 9, 29),
        (2024, 9, 16),
        (2024, 9, 17),
        (2024, 9, 18),
        (2025, 10, 6),
        (2025, 10, 7),
        (2025, 10, 8),
        (2026, 9, 24),
        (2026, 9, 25),
    ];
    vec![
        fixed(1, 1, None),
        HolidayRule::Tabulated { table: seollal },
        fixed_no_roll(3, 1, None),  // Independence Movement
        fixed_no_roll(5, 5, None),  // Children's Day
        fixed_no_roll(6, 6, None),  // Memorial Day
        fixed_no_roll(8, 15, None), // Liberation Day
        HolidayRule::Tabulated { table: chuseok },
        fixed_no_roll(10, 3, None), // National Foundation
        fixed_no_roll(10, 9, None), // Hangul Day
        fixed(12, 25, None),
    ]
}

fn xkrx_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 30, 0).unwrap(),
        chrono_tz::Asia::Seoul,
    )
}

fn xses_rules() -> Vec<HolidayRule> {
    // Singapore Exchange: NY, Lunar NY (use Shanghai's table), Good Friday,
    // Labour, Vesak Day (varies), National Day (Aug 9), Christmas. Vesak
    // and others use simplified handling.
    let lny: &'static [(i32, u32, u32)] = &[
        (2020, 1, 24),
        (2021, 2, 12),
        (2022, 2, 1),
        (2023, 1, 23),
        (2024, 2, 12),
        (2025, 1, 29),
        (2026, 2, 17),
    ];
    let lny2: &'static [(i32, u32, u32)] = &[
        (2020, 1, 27),
        (2021, 2, 15),
        (2022, 2, 2),
        (2023, 1, 24),
        (2024, 2, 13),
        (2025, 1, 30),
        (2026, 2, 18),
    ];
    vec![
        fixed(1, 1, None),
        HolidayRule::Tabulated { table: lny },
        HolidayRule::Tabulated { table: lny2 },
        easter(-2),
        fixed(5, 1, None),
        fixed(8, 9, None),
        fixed(12, 25, None),
    ]
}

fn xses_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        chrono_tz::Asia::Singapore,
    )
}

fn xtai_rules() -> Vec<HolidayRule> {
    // Taiwan Stock Exchange: tabulated Lunar NY (5-7 day closure), Children's
    // Day (Apr 4), Tomb Sweeping (Apr 5), Dragon Boat, Mid-Autumn, ROC
    // National (Oct 10).
    let lny: &'static [(i32, u32, u32)] = &[
        (2020, 1, 23),
        (2021, 2, 8),
        (2022, 1, 27),
        (2023, 1, 19),
        (2024, 2, 5),
        (2025, 1, 23),
        (2026, 2, 13),
    ];
    vec![
        fixed(1, 1, None),
        HolidayRule::Tabulated { table: lny },
        fixed_no_roll(2, 28, None), // Peace Memorial
        fixed_no_roll(4, 4, None),  // Children's
        fixed_no_roll(4, 5, None),  // Tomb Sweeping
        fixed(5, 1, None),
        fixed_no_roll(10, 10, None), // ROC National
    ]
}

fn xtai_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(13, 30, 0).unwrap(),
        chrono_tz::Asia::Taipei,
    )
}

fn xbkk_rules() -> Vec<HolidayRule> {
    // SET Bangkok: NY, Chakri Day (Apr 6), Songkran (Apr 13-15), Labour,
    // Coronation (May 4), Visakha (varies), Asanha (varies), Queen's
    // Birthday (Aug 12), King Bhumibol Memorial (Oct 13), Chulalongkorn
    // (Oct 23), King's Birthday (Dec 5), Constitution (Dec 10), NYE.
    vec![
        fixed(1, 1, None),
        fixed(4, 6, None),
        fixed_no_roll(4, 13, None),
        fixed_no_roll(4, 14, None),
        fixed_no_roll(4, 15, None),
        fixed(5, 1, None),
        fixed_no_roll(5, 4, None),
        fixed_no_roll(8, 12, None),
        fixed(10, 13, None),
        fixed(10, 23, None),
        fixed(12, 5, None),
        fixed(12, 10, None),
        fixed_no_roll(12, 31, None),
    ]
}

fn xbkk_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 30, 0).unwrap(),
        chrono_tz::Asia::Bangkok,
    )
}

fn xkls_rules() -> Vec<HolidayRule> {
    // Bursa Malaysia: NY, Lunar NY, Labour, Wesak, Yang di-Pertuan
    // Agong's Birthday (1st Mon Jun), National (Aug 31), Malaysia Day
    // (Sep 16), Christmas. Eid/Hari Raya are tabulated.
    let lny: &'static [(i32, u32, u32)] = &[
        (2020, 1, 27),
        (2021, 2, 12),
        (2022, 2, 1),
        (2023, 1, 23),
        (2024, 2, 12),
        (2025, 1, 29),
        (2026, 2, 17),
    ];
    vec![
        fixed(1, 1, None),
        HolidayRule::Tabulated { table: lny },
        fixed(5, 1, None),
        nth(6, Weekday::Mon, 1),
        fixed(8, 31, None),
        fixed(9, 16, None),
        fixed(12, 25, None),
    ]
}

fn xkls_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        chrono_tz::Asia::Kuala_Lumpur,
    )
}

fn xidx_rules() -> Vec<HolidayRule> {
    // Indonesia: NY, Lunar NY, Labour, Pancasila (Jun 1), Independence
    // (Aug 17), Christmas. Religious dates simplified.
    let lny: &'static [(i32, u32, u32)] = &[
        (2020, 1, 27),
        (2021, 2, 12),
        (2022, 2, 1),
        (2023, 1, 23),
        (2024, 2, 8),
        (2025, 1, 29),
        (2026, 2, 17),
    ];
    vec![
        fixed(1, 1, None),
        HolidayRule::Tabulated { table: lny },
        fixed(5, 1, None),
        fixed(6, 1, None),
        fixed(8, 17, None),
        fixed(12, 25, None),
    ]
}

fn xidx_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 50, 0).unwrap(),
        chrono_tz::Asia::Jakarta,
    )
}

fn xphs_rules() -> Vec<HolidayRule> {
    // Philippine Stock Exchange: NY, Maundy Thu, Good Fri, Araw ng Kagitingan
    // (Apr 9), Labour, Independence (Jun 12), Ninoy Aquino (Aug 21), National
    // Heroes (last Mon Aug), All Saints, Bonifacio (Nov 30), Christmas,
    // Rizal Day (Dec 30), NYE.
    vec![
        fixed(1, 1, None),
        easter(-3),
        easter(-2),
        fixed(4, 9, None),
        fixed(5, 1, None),
        fixed(6, 12, None),
        fixed(8, 21, None),
        nth(8, Weekday::Mon, -1),
        fixed_no_roll(11, 1, None),
        fixed(11, 30, None),
        fixed(12, 25, None),
        fixed(12, 30, None),
        fixed_no_roll(12, 31, None),
    ]
}

fn xphs_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
        chrono_tz::Asia::Manila,
    )
}

/// Matariki (NZ public holiday since 2022) — official government-announced
/// dates; not derivable from a simple rule.
static NZ_MATARIKI: &[(i32, u32, u32)] = &[
    (2022, 6, 24),
    (2023, 7, 14),
    (2024, 6, 28),
    (2025, 6, 20),
    (2026, 7, 10),
    (2027, 6, 25),
    (2028, 7, 14),
    (2029, 7, 6),
    (2030, 6, 21),
    (2031, 7, 11),
    (2032, 7, 2),
    (2033, 6, 24),
    (2034, 7, 14),
    (2035, 7, 6),
];

fn xnze_rules() -> Vec<HolidayRule> {
    vec![
        consecutive_pair(1, 1, WeekendRoll::ForwardMonday), // New Year + day after
        fixed_fwd(2, 6, None),   // Waitangi Day (mondayised)
        easter(-2),              // Good Friday
        easter(1),               // Easter Monday
        fixed_fwd(4, 25, None),  // ANZAC Day (mondayised)
        nth(6, Weekday::Mon, 1), // King's Birthday (1st Mon Jun)
        nth(10, Weekday::Mon, 4), // Labour Day (4th Mon Oct)
        christmas_boxing(),      // Christmas + Boxing
        HolidayRule::Tabulated { table: NZ_MATARIKI },
        HolidayRule::Tabulated { table: NZ_ONE_OFFS },
    ]
}

/// One-off NZX closure: national day of mourning for Queen Elizabeth II.
static NZ_ONE_OFFS: &[(i32, u32, u32)] = &[(2022, 9, 26)];

fn xnze_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 45, 0).unwrap(),
        chrono_tz::Pacific::Auckland,
    )
}

// EMEA

fn xjse_rules() -> Vec<HolidayRule> {
    // Johannesburg: NY, Human Rights (Mar 21), Good Fri, Family Day (Easter
    // Mon), Freedom (Apr 27), Workers (May 1), Youth (Jun 16), National
    // Women's (Aug 9), Heritage (Sep 24), Day of Reconciliation (Dec 16),
    // Christmas, Day of Goodwill (Dec 26).
    // South African public holidays substitute only Sunday → Monday (Saturday
    // holidays are lost). Christmas and Day of Goodwill are independent Sunday-
    // rolled dates; there is no automatic cascade when a substitute collides
    // with the next holiday (extra days require a special proclamation, tabulated
    // below as one-offs).
    vec![
        fixed_sun(1, 1, None),   // New Year
        fixed_sun(3, 21, None),  // Human Rights Day
        easter(-2),              // Good Friday
        easter(1),               // Family Day (Easter Monday)
        fixed_sun(4, 27, None),  // Freedom Day
        fixed_sun(5, 1, None),   // Workers' Day
        fixed_sun(6, 16, None),  // Youth Day
        fixed_sun(8, 9, None),   // National Women's Day
        fixed_sun(9, 24, None),  // Heritage Day
        fixed_sun(12, 16, None), // Day of Reconciliation
        fixed_sun(12, 25, None), // Christmas
        fixed_sun(12, 26, None), // Day of Goodwill
        HolidayRule::Tabulated { table: XJSE_ONE_OFFS },
    ]
}

/// One-off JSE closures declared by special proclamation (elections; extra
/// festive-season day when Christmas fell on a Sunday in 2016).
static XJSE_ONE_OFFS: &[(i32, u32, u32)] = &[
    (2016, 8, 3),   // Municipal elections
    (2016, 12, 27), // Proclaimed festive-season public holiday
    (2019, 5, 8),   // National elections
];

fn xjse_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        chrono_tz::Africa::Johannesburg,
    )
}

/// Sun-Thu weekmask used by Saudi/Gulf venues. Mon=0, Sun=6.
const MIDEAST_WEEKMASK: [bool; 7] = [true, true, true, true, false, false, true];

fn xsau_rules() -> Vec<HolidayRule> {
    // Saudi Tadawul: National Day (Sep 23), Founding Day (Feb 22). Eid
    // dates vary by lunar calendar — kept tabulated for accuracy 2020-2026.
    let eid_fitr: &'static [(i32, u32, u32)] = &[
        (2020, 5, 24),
        (2021, 5, 13),
        (2022, 5, 2),
        (2023, 4, 21),
        (2024, 4, 10),
        (2025, 3, 30),
        (2026, 3, 20),
    ];
    let eid_adha: &'static [(i32, u32, u32)] = &[
        (2020, 7, 31),
        (2021, 7, 20),
        (2022, 7, 9),
        (2023, 6, 28),
        (2024, 6, 16),
        (2025, 6, 6),
        (2026, 5, 27),
    ];
    vec![
        fixed_no_roll(2, 22, Some(2022)),
        fixed_no_roll(9, 23, None),
        HolidayRule::Tabulated { table: eid_fitr },
        HolidayRule::Tabulated { table: eid_adha },
    ]
}

fn xsau_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
        chrono_tz::Asia::Riyadh,
    )
}

fn xist_rules() -> Vec<HolidayRule> {
    // Borsa Istanbul: NY, National Sovereignty (Apr 23), Labour (May 1),
    // Commemoration of Atatürk (May 19), Democracy (Jul 15), Victory (Aug 30),
    // Republic (Oct 29). Eid dates vary; tabulated.
    let eid_fitr: &'static [(i32, u32, u32)] = &[
        (2020, 5, 24),
        (2021, 5, 13),
        (2022, 5, 2),
        (2023, 4, 21),
        (2024, 4, 10),
        (2025, 3, 30),
        (2026, 3, 20),
    ];
    let eid_adha: &'static [(i32, u32, u32)] = &[
        (2020, 7, 31),
        (2021, 7, 20),
        (2022, 7, 9),
        (2023, 6, 28),
        (2024, 6, 16),
        (2025, 6, 6),
        (2026, 5, 27),
    ];
    vec![
        fixed(1, 1, None),
        fixed(4, 23, None),
        fixed(5, 1, None),
        fixed(5, 19, None),
        fixed(7, 15, None),
        fixed(8, 30, None),
        fixed(10, 29, None),
        HolidayRule::Tabulated { table: eid_fitr },
        HolidayRule::Tabulated { table: eid_adha },
    ]
}

fn xist_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
        chrono_tz::Europe::Istanbul,
    )
}

/// Sun-Thu weekmask used by TASE.
const TASE_WEEKMASK: [bool; 7] = [true, true, true, true, false, false, true];

fn xtae_rules() -> Vec<HolidayRule> {
    // Tel Aviv Stock Exchange: tabulated Jewish holidays (Passover, Shavuot,
    // Rosh Hashanah, Yom Kippur, Sukkot, Simchat Torah, Independence Day).
    // Tabulated 2020-2026.
    let purim: &'static [(i32, u32, u32)] = &[
        (2020, 3, 10),
        (2021, 2, 26),
        (2022, 3, 17),
        (2023, 3, 7),
        (2024, 3, 24),
        (2025, 3, 14),
        (2026, 3, 3),
    ];
    let passover_eve: &'static [(i32, u32, u32)] = &[
        (2020, 4, 8),
        (2021, 3, 27),
        (2022, 4, 15),
        (2023, 4, 5),
        (2024, 4, 22),
        (2025, 4, 12),
        (2026, 4, 1),
    ];
    let shavuot: &'static [(i32, u32, u32)] = &[
        (2020, 5, 29),
        (2021, 5, 17),
        (2022, 6, 5),
        (2023, 5, 26),
        (2024, 6, 12),
        (2025, 6, 2),
        (2026, 5, 22),
    ];
    let rosh: &'static [(i32, u32, u32)] = &[
        (2020, 9, 19),
        (2021, 9, 7),
        (2022, 9, 26),
        (2023, 9, 16),
        (2024, 10, 3),
        (2025, 9, 23),
        (2026, 9, 12),
    ];
    let yom_kippur: &'static [(i32, u32, u32)] = &[
        (2020, 9, 28),
        (2021, 9, 16),
        (2022, 10, 5),
        (2023, 9, 25),
        (2024, 10, 12),
        (2025, 10, 2),
        (2026, 9, 21),
    ];
    let sukkot: &'static [(i32, u32, u32)] = &[
        (2020, 10, 3),
        (2021, 9, 21),
        (2022, 10, 10),
        (2023, 9, 30),
        (2024, 10, 17),
        (2025, 10, 7),
        (2026, 9, 26),
    ];
    let independence: &'static [(i32, u32, u32)] = &[
        (2020, 4, 29),
        (2021, 4, 15),
        (2022, 5, 5),
        (2023, 4, 26),
        (2024, 5, 14),
        (2025, 5, 1),
        (2026, 4, 22),
    ];
    vec![
        HolidayRule::Tabulated { table: purim },
        HolidayRule::Tabulated {
            table: passover_eve,
        },
        HolidayRule::Tabulated { table: shavuot },
        HolidayRule::Tabulated {
            table: independence,
        },
        HolidayRule::Tabulated { table: rosh },
        HolidayRule::Tabulated { table: yom_kippur },
        HolidayRule::Tabulated { table: sukkot },
    ]
}

fn xtae_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 59, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 14, 0).unwrap(),
        chrono_tz::Asia::Jerusalem,
    )
}

fn xdfm_rules() -> Vec<HolidayRule> {
    // Dubai Financial Market / ADX: NY, UAE National (Dec 2-3),
    // Commemoration (Nov 30). Eid tabulated.
    let eid_fitr: &'static [(i32, u32, u32)] = &[
        (2020, 5, 24),
        (2021, 5, 13),
        (2022, 5, 2),
        (2023, 4, 21),
        (2024, 4, 10),
        (2025, 3, 30),
        (2026, 3, 20),
    ];
    let eid_adha: &'static [(i32, u32, u32)] = &[
        (2020, 7, 31),
        (2021, 7, 20),
        (2022, 7, 9),
        (2023, 6, 28),
        (2024, 6, 16),
        (2025, 6, 6),
        (2026, 5, 27),
    ];
    vec![
        fixed(1, 1, None),
        fixed(11, 30, None),
        fixed(12, 2, None),
        fixed(12, 3, None),
        HolidayRule::Tabulated { table: eid_fitr },
        HolidayRule::Tabulated { table: eid_adha },
    ]
}

fn xdfm_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
        chrono_tz::Asia::Dubai,
    )
}

// LatAm

fn bvmf_rules() -> Vec<HolidayRule> {
    // B3 / BMF Bovespa (São Paulo): NY, Carnival Mon (-48), Carnival Tue (-47),
    // Good Friday, Tiradentes (Apr 21), Labour, Corpus Christi (+60),
    // Independence (Sep 7), Our Lady of Aparecida (Oct 12), All Souls (Nov 2),
    // Republic (Nov 15), Black Awareness (Nov 20), Christmas Eve, Christmas, NYE.
    // B3 stopped observing São Paulo state/city holidays after 2021; Black
    // Awareness Day became a national holiday in 2024. In 2020 the exchange did
    // not close for the São Paulo holidays (see exceptions at construction).
    vec![
        fixed_no_roll(1, 1, None),
        fixed_between(1, 25, None, Some(2021)), // São Paulo city anniversary
        easter(-48),
        easter(-47),
        easter(-2),
        fixed_no_roll(4, 21, None),
        fixed_no_roll(5, 1, None),
        easter(60),
        fixed_between(7, 9, None, Some(2021)), // São Paulo Constitutionalist Revolution
        fixed_no_roll(9, 7, None),
        fixed_no_roll(10, 12, None),
        fixed_no_roll(11, 2, None),
        fixed_no_roll(11, 15, None),
        fixed_between(11, 20, None, Some(2021)), // Black Awareness (São Paulo era)
        fixed_no_roll(11, 20, Some(2024)),       // Black Awareness (national since 2024)
        fixed_no_roll(12, 24, None),
        fixed_no_roll(12, 25, None),
        fixed_prev_fri(12, 31), // New Year's Eve (preceding Friday if weekend)
    ]
}

fn bvmf_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 30, 0).unwrap(),
        chrono_tz::America::Sao_Paulo,
    )
}

fn xmex_rules() -> Vec<HolidayRule> {
    // BMV Mexico: NY, Constitution (1st Mon Feb), Benito Juárez (3rd Mon Mar),
    // Maundy Thu, Good Fri, Labour, Independence (Sep 16), Revolution
    // (3rd Mon Nov), Banxico holiday (Dec 12), Christmas.
    vec![
        fixed_no_roll(1, 1, None),
        nth(2, Weekday::Mon, 1),
        nth(3, Weekday::Mon, 3),
        easter(-3),
        easter(-2),
        fixed_no_roll(5, 1, None),
        fixed_no_roll(9, 16, None),
        fixed_no_roll(11, 2, None), // All Souls' Day
        nth(11, Weekday::Mon, 3),
        fixed_no_roll(12, 12, None),
        fixed_no_roll(12, 25, None),
    ]
}

fn xmex_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(8, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
        chrono_tz::America::Mexico_City,
    )
}

fn xbue_rules() -> Vec<HolidayRule> {
    // Buenos Aires (BYMA): NY, Carnival Mon/Tue, Truth & Justice (Mar 24),
    // Malvinas Day (Apr 2), Good Fri, Labour, May Revolution (May 25),
    // Flag Day (Jun 20), Independence (Jul 9), San Martín (3rd Mon Aug),
    // Diversity (Oct 12), Sovereignty (Nov 20), Immaculate, Christmas.
    vec![
        fixed(1, 1, None),
        easter(-48),
        easter(-47),
        fixed(3, 24, None),
        fixed(4, 2, None),
        easter(-2),
        fixed(5, 1, None),
        fixed(5, 25, None),
        fixed(6, 20, None),
        fixed(7, 9, None),
        nth(8, Weekday::Mon, 3),
        fixed(10, 12, None),
        fixed(11, 20, None),
        fixed(12, 8, None),
        fixed(12, 25, None),
    ]
}

fn xbue_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(11, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        chrono_tz::America::Argentina::Buenos_Aires,
    )
}

fn xsgo_rules() -> Vec<HolidayRule> {
    // Santiago Stock Exchange: NY, Good Fri, Holy Sat, Labour, Navy Day
    // (May 21), Saint Peter & Paul (Jun 29), Virgen del Carmen (Jul 16),
    // Assumption, Independence (Sep 18-19), Columbus (Oct 12), Reformation
    // (Oct 31), All Saints, Immaculate, Christmas.
    vec![
        fixed(1, 1, None),
        easter(-2),
        easter(-1),
        fixed(5, 1, None),
        fixed(5, 21, None),
        fixed(6, 29, None),
        fixed(7, 16, None),
        fixed(8, 15, None),
        fixed(9, 18, None),
        fixed(9, 19, None),
        fixed(10, 12, None),
        fixed(10, 31, None),
        fixed(11, 1, None),
        fixed(12, 8, None),
        fixed(12, 25, None),
    ]
}

fn xsgo_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        chrono_tz::America::Santiago,
    )
}

fn xlim_rules() -> Vec<HolidayRule> {
    // Lima Stock Exchange: NY, Maundy Thu, Good Fri, Labour, Saint Peter
    // & Paul, Independence (Jul 28-29), Santa Rosa (Aug 30), Battle of
    // Angamos (Oct 8), All Saints, Immaculate, Christmas.
    vec![
        fixed(1, 1, None),
        easter(-3),
        easter(-2),
        fixed(5, 1, None),
        fixed(6, 29, None),
        fixed(7, 28, None),
        fixed(7, 29, None),
        fixed(8, 30, None),
        fixed(10, 8, None),
        fixed(11, 1, None),
        fixed(12, 8, None),
        fixed(12, 25, None),
    ]
}

fn xlim_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(8, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
        chrono_tz::America::Lima,
    )
}

fn xbog_rules() -> Vec<HolidayRule> {
    // Bogotá (BVC). Many holidays follow the Emiliani law (observed the next
    // Monday). The Easter-based movable feasts are pre-shifted to their observed
    // Monday: Ascension = Easter+43, Corpus Christi = Easter+64, Sacred Heart =
    // Easter+71.
    vec![
        fixed_no_roll(1, 1, None),
        emiliani(1, 6),  // Epiphany
        emiliani(3, 19), // Saint Joseph
        easter(-3),      // Maundy Thursday
        easter(-2),      // Good Friday
        fixed_no_roll(5, 1, None), // Labour Day
        easter(43),      // Ascension (observed Monday)
        easter(64),      // Corpus Christi (observed Monday)
        easter(71),      // Sacred Heart (observed Monday)
        emiliani(6, 29), // Saint Peter & Paul
        fixed_no_roll(7, 20, None), // Independence
        fixed_no_roll(8, 7, None),  // Battle of Boyacá
        emiliani(8, 15), // Assumption
        emiliani(10, 12), // Race Day
        emiliani(11, 1),  // All Saints
        emiliani(11, 11), // Independence of Cartagena
        fixed_no_roll(12, 8, None),  // Immaculate Conception
        fixed_no_roll(12, 25, None), // Christmas
        fixed_prev_fri(12, 31), // New Year's Eve (preceding Friday if weekend)
    ]
}

fn xbog_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        chrono_tz::America::Bogota,
    )
}

// Calendar family resolver

/// Logical calendar family. Many MICs share a family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    UsEquity,
    UsOptions,
    UsBondSifma,
    UsFuturesCme,
    UsFuturesCmeEnergy,
    UsFuturesCbotGrains,
    UsFuturesCmeLivestock,
    UsFuturesCmeLumber,
    UsFuturesIce,
    UsFuturesCfe,
    Forex24x5,
    Crypto24x7,
    Lse,
    Tse,
    Hkex,
    Sse,
    Xetra,
    EuronextParis,
    EuronextAms,
    EuronextBru,
    EuronextLis,
    EuronextDub,
    Tsx,
    Asx,
    Nse,
    Xmil,
    Xmad,
    Xswx,
    Xosl,
    Xsto,
    Xhel,
    Xcse,
    Xice,
    Xwar,
    Xpra,
    Xbud,
    Xwbo,
    Xkrx,
    Xses,
    Xtai,
    Xbkk,
    Xkls,
    Xidx,
    Xphs,
    Xnze,
    Xjse,
    Xsau,
    Xist,
    Xtae,
    Xdfm,
    Bvmf,
    Xmex,
    Xbue,
    Xsgo,
    Xlim,
    Xbog,
}

fn family_for_mic(mic: &str) -> Option<Family> {
    use Family::*;
    let m = match mic {
        // US equities
        "XNYS" | "NYSD" | "XCIS" | "CISD" | "XCHI" | "ARCX" | "ARCD" | "ARCO"
        | "XASE" | "AMXO" | "XNAS" | "XNGS" | "XNCM" | "XNMS" | "NASD" | "XNDQ"
        | "XBOS" | "BOSD" | "XBXO" | "XPHL" | "XPSX" | "PSXD" | "XPHO" | "XPBT"
        | "XPOR" | "XNFI" | "EDGA" | "EDGD" | "EDGX" | "EDDP" | "EDGO" | "BATS"
        | "BZXD" | "BATO" | "BATY" | "BYXD" | "MEMX" | "MEMD" | "IEXG" | "LTSE"
        | "MIHI" | "MPRL" | "EPRL" | "EPRD" | "XMIO" | "EMLD"
        // Over-the-counter and Financial Industry Regulatory Authority venues share the NYSE holiday calendar
        | "OTCM" | "CAVE" | "OTCB" | "OTCQ" | "PINL" | "PINI" | "PINX" | "PSGM"
        | "PINC" | "FINR" | "FINN" | "FINC" | "FINY" | "XADF" | "FINO" | "OOTC"
        // Synthetic / placeholder venues
        | "XXXX" | "PYPR" | "SIMU" => UsEquity,
        // US options
        "XISE" | "GMNI" | "MCRY" | "XCBO" | "C2OX" | "MXOP" | "OPRA" => UsOptions,
        // US futures: CME group equity-index/FX/financials. We also route
        // category-level synthetic aliases from CME's Globex filter buckets
        // to this baseline template when no product-specific schedule is
        // modeled yet.
        "XCME" | "FCME" | "GLBX" | "XCBT" | "FCBT" | "XKBT"
        // Product-level aliases that use the baseline overnight template.
        | "SR3" | "ES" | "NQ" | "RTY"
        | "CME_DAIRY" | "GLOBEX_DAIRY" => UsFuturesCme,
        // Product-level aliases for livestock daytime sessions.
        "LE" | "GF" | "HE" | "CME_LIVESTOCK" | "GLOBEX_LIVESTOCK" => {
            UsFuturesCmeLivestock
        }
        // Product-level aliases for lumber daytime sessions.
        "LBR" | "LS" | "CME_LUMBER" | "GLOBEX_LUMBER" => UsFuturesCmeLumber,
        // NYMEX (energy/metals) lives under CME group too but with energy hours.
        "XNYM" | "NYMEX_ENERGY" | "COMEX_METALS"
        // Product-level energy/metals aliases.
        | "CL" | "MCL" | "QM" | "GC" | "MGC" | "QO"
        | "CME_ENERGY" | "GLOBEX_ENERGY"
        | "CME_METALS" | "GLOBEX_METALS" => UsFuturesCmeEnergy,
        // Synthetic product-group calendars for materially different CBOT
        // agricultural hours.
        "CBOT_GRAINS" | "CME_GRAINS" | "GLOBEX_GRAINS"
        // Product-level grain/oilseed aliases.
        | "ZC" | "ZW" | "ZS" | "ZL" | "ZM" | "ZO" | "KE" | "HRS"
        | "CBOT_OILSEEDS" | "CBOT_WHEAT" | "CBOT_CORN" | "CBOT_SOYBEANS" => {
            UsFuturesCbotGrains
        }
        // CFE / ICE / SIFMA / FX / Crypto generic families
        "CFE" => UsFuturesCfe,
        "ICE_US" => UsFuturesIce,
        "SIFMA_US" => UsBondSifma,
        "FOREX" => Forex24x5,
        "CRYPTO" => Crypto24x7,
        // Canada
        "XTSE" | "XDRK" | "VDRK" | "XTSX" | "XTNX" | "XATS" | "XATX" | "ADRK"
        | "XMOD" | "XMOC" | "NEOE" | "NEOD" | "NEON" | "NEOC" | "XCNQ" | "PURE"
        | "CSE2" => Tsx,
        // Major non-US equities
        "XLON" => Lse,
        "XTKS" => Tse,
        "XHKG" => Hkex,
        "XSHG" => Sse,
        "21XX" | "XEUR" | "XFRA" => Xetra,
        "XPAR" => EuronextParis,
        "XAMS" => EuronextAms,
        "XBRU" => EuronextBru,
        "XLIS" => EuronextLis,
        "XDUB" => EuronextDub,
        "XMIL" => Xmil,
        "XMAD" => Xmad,
        "XSWX" => Xswx,
        "XOSL" => Xosl,
        "XSTO" => Xsto,
        "XHEL" => Xhel,
        "XCSE" => Xcse,
        "XICE" => Xice,
        "XWAR" => Xwar,
        "XPRA" => Xpra,
        "XBUD" => Xbud,
        "XWBO" => Xwbo,
        "XASX" => Asx,
        "XBOM" | "XNSE" => Nse,
        "XKRX" => Xkrx,
        "XSES" => Xses,
        "XTAI" => Xtai,
        "XBKK" => Xbkk,
        "XKLS" => Xkls,
        "XIDX" => Xidx,
        "XPHS" => Xphs,
        "XNZE" => Xnze,
        "XJSE" => Xjse,
        "XSAU" => Xsau,
        "XIST" => Xist,
        "XTAE" => Xtae,
        "XDFM" | "XADS" => Xdfm,
        "BVMF" => Bvmf,
        "XMEX" => Xmex,
        "XBUE" => Xbue,
        "XSGO" => Xsgo,
        "XLIM" => Xlim,
        "XBOG" => Xbog,
        _ => return None,
    };
    Some(m)
}

fn build_family(name: &str, fam: Family) -> Calendar {
    use Family::*;
    match fam {
        UsEquity => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            nyse_rules(),
            Some(nyse_trading_hours()),
        )
        .with_early_closes(nyse_early_closes()),
        UsOptions => Calendar::with_type(
            name,
            market_type("Options"),
            STANDARD_WEEKMASK,
            nyse_rules(),
            Some(options_trading_hours()),
        )
        .with_early_closes(nyse_early_closes()),
        UsBondSifma => Calendar::with_type(
            name,
            market_type("FixedIncome"),
            STANDARD_WEEKMASK,
            sifma_us_rules(),
            Some(sifma_us_hours()),
        ),
        UsFuturesCme => Calendar::with_type(
            name,
            market_type("Futures"),
            STANDARD_WEEKMASK,
            cme_globex_rules(),
            Some(cme_globex_overnight_hours()),
        ),
        UsFuturesCmeEnergy => Calendar::with_type(
            name,
            market_type("Futures"),
            STANDARD_WEEKMASK,
            cme_globex_rules(),
            Some(cme_globex_energy_hours()),
        ),
        UsFuturesCbotGrains => Calendar::with_type(
            name,
            market_type("Futures"),
            STANDARD_WEEKMASK,
            cme_globex_rules(),
            Some(cbot_grain_futures_hours()),
        ),
        UsFuturesCmeLivestock => Calendar::with_type(
            name,
            market_type("Futures"),
            STANDARD_WEEKMASK,
            cme_globex_rules(),
            Some(cme_livestock_hours()),
        ),
        UsFuturesCmeLumber => Calendar::with_type(
            name,
            market_type("Futures"),
            STANDARD_WEEKMASK,
            cme_globex_rules(),
            Some(cme_lumber_hours()),
        ),
        UsFuturesIce => Calendar::with_type(
            name,
            market_type("Futures"),
            STANDARD_WEEKMASK,
            ice_us_rules(),
            Some(ice_us_hours()),
        ),
        UsFuturesCfe => Calendar::with_type(
            name,
            market_type("Futures"),
            STANDARD_WEEKMASK,
            cfe_rules(),
            Some(cfe_trading_hours()),
        ),
        Forex24x5 => Calendar::with_type(
            name,
            market_type("ForeignExchange"),
            STANDARD_WEEKMASK,
            forex_rules(),
            Some(TradingHours::forex_24x5()),
        ),
        Crypto24x7 => Calendar::with_type(
            name,
            market_type("DigitalAssets"),
            CRYPTO_WEEKMASK,
            crypto_rules(),
            Some(TradingHours::crypto_24x7()),
        ),
        Lse => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            lse_rules(),
            Some(lse_trading_hours()),
        )
        .with_exceptions(LSE_MOVED),
        Tse => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            tse_rules(),
            Some(tse_trading_hours()),
        )
        .with_schedules(tse_schedules())
        .with_exceptions(TSE_MOVED)
        .with_adjustment(HolidayAdjustment::Japanese),
        Hkex => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            hkex_rules(),
            Some(hkex_trading_hours()),
        ),
        Sse => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            sse_rules(),
            Some(sse_trading_hours()),
        ),
        Xetra => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xetra_rules(),
            Some(xetra_trading_hours()),
        ),
        EuronextParis => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            euronext_paris_rules(),
            Some(euronext_paris_trading_hours()),
        ),
        EuronextAms => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xams_rules(),
            Some(euronext_hours(chrono_tz::Europe::Amsterdam)),
        ),
        EuronextBru => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xbru_rules(),
            Some(euronext_hours(chrono_tz::Europe::Brussels)),
        ),
        EuronextLis => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xlis_rules(),
            Some(euronext_hours(chrono_tz::Europe::Lisbon)),
        ),
        EuronextDub => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xdub_rules(),
            Some(xdub_hours()),
        ),
        Tsx => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            tsx_rules(),
            Some(tsx_trading_hours()),
        ),
        Asx => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            asx_rules(),
            Some(asx_trading_hours()),
        ),
        Nse => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            nse_rules(),
            Some(nse_trading_hours()),
        ),
        Xmil => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xmil_rules(),
            Some(xmil_hours()),
        ),
        Xmad => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xmad_rules(),
            Some(xmad_hours()),
        ),
        Xswx => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xswx_rules(),
            Some(xswx_hours()),
        ),
        Xosl => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xosl_rules(),
            Some(xosl_hours()),
        ),
        Xsto => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xsto_rules(),
            Some(xsto_hours()),
        ),
        Xhel => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xhel_rules(),
            Some(xhel_hours()),
        ),
        Xcse => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xcse_rules(),
            Some(xcse_hours()),
        ),
        Xice => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xice_rules(),
            Some(xice_hours()),
        ),
        Xwar => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xwar_rules(),
            Some(xwar_hours()),
        ),
        Xpra => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xpra_rules(),
            Some(xpra_hours()),
        ),
        Xbud => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xbud_rules(),
            Some(xbud_hours()),
        ),
        Xwbo => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xwbo_rules(),
            Some(xwbo_hours()),
        ),
        Xkrx => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xkrx_rules(),
            Some(xkrx_hours()),
        ),
        Xses => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xses_rules(),
            Some(xses_hours()),
        ),
        Xtai => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xtai_rules(),
            Some(xtai_hours()),
        ),
        Xbkk => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xbkk_rules(),
            Some(xbkk_hours()),
        ),
        Xkls => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xkls_rules(),
            Some(xkls_hours()),
        ),
        Xidx => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xidx_rules(),
            Some(xidx_hours()),
        ),
        Xphs => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xphs_rules(),
            Some(xphs_hours()),
        ),
        Xnze => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xnze_rules(),
            Some(xnze_hours()),
        ),
        Xjse => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xjse_rules(),
            Some(xjse_hours()),
        ),
        Xsau => Calendar::with_type(
            name,
            market_type("Equities"),
            MIDEAST_WEEKMASK,
            xsau_rules(),
            Some(xsau_hours()),
        ),
        Xist => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xist_rules(),
            Some(xist_hours()),
        ),
        Xtae => Calendar::with_type(
            name,
            market_type("Equities"),
            TASE_WEEKMASK,
            xtae_rules(),
            Some(xtae_hours()),
        ),
        Xdfm => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xdfm_rules(),
            Some(xdfm_hours()),
        ),
        Bvmf => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            bvmf_rules(),
            Some(bvmf_hours()),
        )
        // In 2020 B3 traded on the São Paulo state/city holidays.
        .with_exceptions(&[(2020, 7, 9), (2020, 11, 20)]),
        Xmex => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xmex_rules(),
            Some(xmex_hours()),
        ),
        Xbue => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xbue_rules(),
            Some(xbue_hours()),
        ),
        Xsgo => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xsgo_rules(),
            Some(xsgo_hours()),
        ),
        Xlim => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xlim_rules(),
            Some(xlim_hours()),
        ),
        Xbog => Calendar::with_type(
            name,
            market_type("Equities"),
            STANDARD_WEEKMASK,
            xbog_rules(),
            Some(xbog_hours()),
        ),
    }
}

/// Build a calendar from its MIC code (or a generic family name like
/// `FOREX`, `CRYPTO`, `SIFMA_US`, `ICE_US`, `CFE`). Returns `None` if unknown.
pub fn calendar_for_exchange(code: &str) -> Option<Calendar> {
    let upper = code.to_ascii_uppercase();
    if let Some(fam) = family_for_mic(&upper) {
        return Some(build_family(&upper, fam));
    }

    let record = finance_enums::data::exchange_record(&upper)?;
    if let Some(mut calendar) = calendar_for_region(record.iso_country_code) {
        calendar.name = upper;
        return Some(calendar);
    }

    Some(Calendar::with_type(
        upper,
        market_type_for_exchange_record(record),
        STANDARD_WEEKMASK,
        Vec::new(),
        None,
    ))
}

fn market_type_for_exchange_record(record: &finance_enums::data::ExchangeRecord) -> &'static str {
    match record.market_category_code {
        "IDQS" | "NSPD" | "OTFS" | "SINT" => market_type("OverTheCounter"),
        _ => market_type("Equities"),
    }
}

/// Build a calendar from a country code. Returns `None` if unknown.
pub fn calendar_for_region(code: &str) -> Option<Calendar> {
    let upper = code.to_ascii_uppercase();
    if !COUNTRY_CODES.contains(&upper.as_str()) && !COUNTRY_CODES3.contains(&upper.as_str()) {
        return None;
    }
    match upper.as_str() {
        "US" | "USA" => calendar_for_exchange("XNYS"),
        "GB" | "GBR" => calendar_for_exchange("XLON"),
        "JP" | "JPN" => calendar_for_exchange("XTKS"),
        "HK" | "HKG" => calendar_for_exchange("XHKG"),
        "CN" | "CHN" => calendar_for_exchange("XSHG"),
        "DE" | "DEU" => calendar_for_exchange("XFRA"),
        "FR" | "FRA" => calendar_for_exchange("XPAR"),
        "CA" | "CAN" => calendar_for_exchange("XTSE"),
        "AU" | "AUS" => calendar_for_exchange("XASX"),
        "IN" | "IND" => calendar_for_exchange("XNSE"),
        "NL" | "NLD" => calendar_for_exchange("XAMS"),
        "BE" | "BEL" => calendar_for_exchange("XBRU"),
        "PT" | "PRT" => calendar_for_exchange("XLIS"),
        "IT" | "ITA" => calendar_for_exchange("XMIL"),
        "ES" | "ESP" => calendar_for_exchange("XMAD"),
        "CH" | "CHE" => calendar_for_exchange("XSWX"),
        "NO" | "NOR" => calendar_for_exchange("XOSL"),
        "SE" | "SWE" => calendar_for_exchange("XSTO"),
        "FI" | "FIN" => calendar_for_exchange("XHEL"),
        "DK" | "DNK" => calendar_for_exchange("XCSE"),
        "IS" | "ISL" => calendar_for_exchange("XICE"),
        "PL" | "POL" => calendar_for_exchange("XWAR"),
        "CZ" | "CZE" => calendar_for_exchange("XPRA"),
        "HU" | "HUN" => calendar_for_exchange("XBUD"),
        "AT" | "AUT" => calendar_for_exchange("XWBO"),
        "IE" | "IRL" => calendar_for_exchange("XDUB"),
        "KR" | "KOR" => calendar_for_exchange("XKRX"),
        "SG" | "SGP" => calendar_for_exchange("XSES"),
        "TW" | "TWN" => calendar_for_exchange("XTAI"),
        "TH" | "THA" => calendar_for_exchange("XBKK"),
        "MY" | "MYS" => calendar_for_exchange("XKLS"),
        "ID" | "IDN" => calendar_for_exchange("XIDX"),
        "PH" | "PHL" => calendar_for_exchange("XPHS"),
        "NZ" | "NZL" => calendar_for_exchange("XNZE"),
        "ZA" | "ZAF" => calendar_for_exchange("XJSE"),
        "SA" | "SAU" => calendar_for_exchange("XSAU"),
        "TR" | "TUR" => calendar_for_exchange("XIST"),
        "IL" | "ISR" => calendar_for_exchange("XTAE"),
        "AE" | "ARE" => calendar_for_exchange("XDFM"),
        "BR" | "BRA" => calendar_for_exchange("BVMF"),
        "MX" | "MEX" => calendar_for_exchange("XMEX"),
        "AR" | "ARG" => calendar_for_exchange("XBUE"),
        "CL" | "CHL" => calendar_for_exchange("XSGO"),
        "PE" | "PER" => calendar_for_exchange("XLIM"),
        "CO" | "COL" => calendar_for_exchange("XBOG"),
        _ => None,
    }
}

/// Build a calendar for a specific product at a given exchange.
///
/// `exchange` is an exchange MIC (or a synthetic alias such as `FOREX` or
/// `CRYPTO`); `product` is a variant name from the `finance-enums`
/// commodity/instrument sub-type enums, e.g. `"NaturalGas"`, `"Corn"`,
/// `"Gold"`, `"Cattle"`.  When the exchange+product pair matches a
/// product-specific schedule the result calendar is named
/// `"<EXCHANGE>:<product>"`.  When no product-specific match is found the
/// call falls back to `calendar_for_exchange(exchange)`.
pub fn calendar_for_product(exchange: &str, product: &str) -> Option<Calendar> {
    use Family::*;
    let exch = exchange.to_ascii_uppercase();
    let fam: Option<Family> = match (exch.as_str(), product) {
        // ── NYMEX / COMEX ──────────────────────────────────────────────
        // Energy (CL, QM, NG, HO, RB, PRP, UX, etc.)
        ("XNYM", "Crude")
        | ("XNYM", "NaturalGas")
        | ("XNYM", "HeatingOil")
        | ("XNYM", "Gasoline")
        | ("XNYM", "LiquefiedNaturalGas")
        | ("XNYM", "Propane")
        | ("XNYM", "Electricity")
        | ("XNYM", "Uranium")
        | ("XNYM", "Energy") => Some(UsFuturesCmeEnergy),
        // Metals (GC, SI, HG, PL, PA, AL…)
        ("XNYM", "Gold")
        | ("XNYM", "Silver")
        | ("XNYM", "Copper")
        | ("XNYM", "Platinum")
        | ("XNYM", "Palladium")
        | ("XNYM", "Aluminum")
        | ("XNYM", "Zinc")
        | ("XNYM", "Nickel")
        | ("XNYM", "Lead")
        | ("XNYM", "Tin")
        | ("XNYM", "Steel")
        | ("XNYM", "Cobalt")
        | ("XNYM", "Iron")
        | ("XNYM", "Metals") => Some(UsFuturesCmeEnergy),

        // ── CBOT grains / oilseeds ─────────────────────────────────────
        ("XCBT", "Corn")
        | ("XCBT", "Wheat")
        | ("XCBT", "Soybean")
        | ("XCBT", "Oats")
        | ("XCBT", "Soy")
        | ("XCBT", "Agriculture")
        | ("XCBT", "Softs") => Some(UsFuturesCbotGrains),

        // ── CME livestock ──────────────────────────────────────────────
        ("XCME", "Cattle") | ("XCME", "Feeder") | ("XCME", "Hogs") | ("XCME", "Livestock") => {
            Some(UsFuturesCmeLivestock)
        }

        // ── CME lumber ─────────────────────────────────────────────────
        ("XCME", "Lumber") => Some(UsFuturesCmeLumber),

        // ── ICE US (softs + Brent crude) ──────────────────────────────
        ("ICE_US", "Sugar")
        | ("ICE_US", "Coffee")
        | ("ICE_US", "Cocoa")
        | ("ICE_US", "Cotton")
        | ("ICE_US", "OrangeJuice")
        | ("ICE_US", "Crude")
        | ("ICE_US", "NaturalGas")
        | ("ICE_US", "Energy")
        | ("ICE_US", "Softs")
        | ("ICE_US", "Agriculture") => Some(UsFuturesIce),

        // ── no product-specific override; fall through ─────────────────
        _ => None,
    };

    if let Some(family) = fam {
        let name = format!("{}:{}", exch, product);
        Some(build_family(&name, family))
    } else {
        calendar_for_exchange(exchange)
    }
}

fn is_known_asset_label(value: &str) -> bool {
    UNDERLYING_ASSET_CLASSES.contains(&value)
        || COMMODITY_TYPES.contains(&value)
        || ENERGY_TYPES.contains(&value)
        || METALS_TYPES.contains(&value)
        || AGRICULTURE_TYPES.contains(&value)
        || matches!(value, "Feeder")
}

/// Build a calendar from an exchange and finance-enums asset vocabulary.
///
/// `asset_class` and optional `subclass` are canonical finance-enums variant
/// names, such as `UnderlyingAssetClass::Commodity` plus
/// `EnergyType::NaturalGas`, or simply `UnderlyingAssetClass::Equity` for a
/// broad exchange calendar. When a subclass is provided it chooses the product
/// schedule; otherwise `asset_class` is used directly.
pub fn calendar_for_asset(
    exchange: &str,
    asset_class: &str,
    subclass: Option<&str>,
) -> Option<Calendar> {
    if !is_known_asset_label(asset_class) {
        return None;
    }
    let product = subclass.unwrap_or(asset_class);
    if !is_known_asset_label(product) {
        return None;
    }
    calendar_for_product(exchange, product)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono::Timelike;

    #[test]
    fn nyse_2024_business_day_count() {
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
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2022, 12, 26).unwrap()));
    }

    #[test]
    fn nyse_juneteenth_first_year_2022() {
        let cal = calendar_for_exchange("XNYS").unwrap();
        // NYSE first observed Juneteenth in 2022; the market was open on
        // 2021-06-18 (federal holiday established too late to be observed).
        assert!(!cal.is_holiday(NaiveDate::from_ymd_opt(2020, 6, 19).unwrap()));
        assert!(!cal.is_holiday(NaiveDate::from_ymd_opt(2021, 6, 18).unwrap()));
        // 2022-06-19 was a Sunday, observed Monday 2022-06-20.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2022, 6, 20).unwrap()));
    }

    #[test]
    fn nyse_carter_day_of_mourning_2025() {
        let cal = calendar_for_exchange("XNYS").unwrap();
        // Jan 9, 2025 was a Thursday, closed for President Carter's funeral.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2025, 1, 9).unwrap()));
        // Adjacent trading days remain open.
        assert!(!cal.is_holiday(NaiveDate::from_ymd_opt(2025, 1, 8).unwrap()));
        assert!(!cal.is_holiday(NaiveDate::from_ymd_opt(2025, 1, 10).unwrap()));
    }

    #[test]
    fn nyse_special_closures_9_11_and_sandy() {
        let cal = calendar_for_exchange("XNYS").unwrap();
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2001, 9, 11).unwrap()));
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2012, 10, 29).unwrap()));
    }

    #[test]
    fn lse_easter_monday_2024() {
        let cal = calendar_for_exchange("XLON").unwrap();
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2024, 4, 1).unwrap()));
    }

    #[test]
    fn jpx_equinox_substitute_and_citizens() {
        let cal = calendar_for_exchange("XTKS").unwrap();
        // Computed equinoxes.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2020, 3, 20).unwrap())); // vernal
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2020, 9, 22).unwrap())); // autumnal
        // Citizens' holiday: 2015-09-22, between Respect-for-the-Aged (Mon 21)
        // and the autumnal equinox (Wed 23).
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2015, 9, 22).unwrap()));
        // Substitute holiday: Constitution Day 2020-05-03 (Sun) → Wed May 6.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2020, 5, 6).unwrap()));
        // Olympic move: Marine Day 2020 to Jul 23, not the normal 3rd Monday.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2020, 7, 23).unwrap()));
        assert!(!cal.is_holiday(NaiveDate::from_ymd_opt(2020, 7, 20).unwrap()));
    }

    #[test]
    fn tsx_victoria_day_and_substitution() {
        let cal = calendar_for_exchange("XTSE").unwrap();
        // Victoria Day = Monday on or before May 24 (not last Monday of May).
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2024, 5, 20).unwrap()));
        assert!(!cal.is_holiday(NaiveDate::from_ymd_opt(2024, 5, 27).unwrap()));
        // Canada Day 2023-07-01 Sat → substitute Mon Jul 3.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2023, 7, 3).unwrap()));
    }

    #[test]
    fn lse_bank_holiday_substitution_and_oneoffs() {
        let cal = calendar_for_exchange("XLON").unwrap();
        // New Year 2022-01-01 Sat → substitute Mon Jan 3.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2022, 1, 3).unwrap()));
        assert!(!cal.is_holiday(NaiveDate::from_ymd_opt(2022, 1, 1).unwrap()));
        // Christmas 2021: Dec 25 Sat → Mon 27, Boxing → Tue 28.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2021, 12, 27).unwrap()));
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2021, 12, 28).unwrap()));
        // One-off closures.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2022, 9, 19).unwrap())); // Queen funeral
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2023, 5, 8).unwrap())); // Coronation
        // Spring bank holiday moved in 2022: May 30 traded, Jun 2 closed.
        assert!(!cal.is_holiday(NaiveDate::from_ymd_opt(2022, 5, 30).unwrap()));
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2022, 6, 2).unwrap()));
    }

    #[test]
    fn region_us_resolves_to_xnys() {
        let cal = calendar_for_region("US").unwrap();
        assert_eq!(cal.name, "XNYS");
        assert_eq!(calendar_for_region("USA").unwrap().name, "XNYS");
        assert!(calendar_for_region("EU").is_none());
        assert!(calendar_for_region("UK").is_none());
    }

    #[test]
    fn nyse_is_open_at_market_open() {
        let cal = calendar_for_exchange("XNYS").unwrap();
        let inst = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 8, 9, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
        let inst_b = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 8, 9, 27, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(!cal.is_open(inst_b));
    }

    #[test]
    fn nyse_is_open_handles_dst() {
        let cal = calendar_for_exchange("XNYS").unwrap();
        let inst = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 3, 11, 9, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
    }

    #[test]
    fn cme_futures_open_sunday_evening() {
        // CME equity-index futures: Sun 18:00 CT should be in Mon's session.
        let cal = calendar_for_exchange("XCME").unwrap();
        assert_eq!(cal.market_type, market_type("Futures"));
        let inst = chrono_tz::America::Chicago
            .with_ymd_and_hms(2024, 1, 7, 18, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
        // Sat 03:00 CT — closed.
        let inst2 = chrono_tz::America::Chicago
            .with_ymd_and_hms(2024, 1, 13, 3, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(!cal.is_open(inst2));
    }

    #[test]
    fn nymex_energy_uses_chicago_tz() {
        let cal = calendar_for_exchange("XNYM").unwrap();
        assert_eq!(cal.market_type, market_type("Futures"));
        // Mon 09:00 CT → in session (started Sun 17:00 CT).
        let inst = chrono_tz::America::Chicago
            .with_ymd_and_hms(2024, 1, 8, 9, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
    }

    #[test]
    fn nymex_energy_daily_maintenance_break_is_closed() {
        let cal = calendar_for_exchange("XNYM").unwrap();
        let maintenance_break = chrono_tz::America::Chicago
            .with_ymd_and_hms(2024, 1, 8, 16, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        let next_trade_date_open = chrono_tz::America::Chicago
            .with_ymd_and_hms(2024, 1, 8, 17, 0, 0)
            .unwrap()
            .with_timezone(&Utc);

        assert!(!cal.is_open(maintenance_break));
        assert_eq!(cal.next_open(maintenance_break), Some(next_trade_date_open));
    }

    #[test]
    fn cbot_grain_futures_expose_overnight_and_day_sessions() {
        let cal = calendar_for_exchange("CBOT_GRAINS").unwrap();
        assert_eq!(cal.market_type, market_type("Futures"));
        let th = cal.trading_hours.as_ref().unwrap();
        let actual: Vec<_> = th
            .sessions
            .iter()
            .map(|session| {
                (
                    (
                        session.open.hour(),
                        session.open.minute(),
                        session.open_day_offset,
                    ),
                    (
                        session.close.hour(),
                        session.close.minute(),
                        session.close_day_offset,
                    ),
                )
            })
            .collect();

        assert_eq!(
            actual,
            vec![((19, 0, -1), (7, 45, 0)), ((8, 30, 0), (13, 20, 0))]
        );

        let morning_break = chrono_tz::America::Chicago
            .with_ymd_and_hms(2024, 1, 8, 8, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        let day_open = chrono_tz::America::Chicago
            .with_ymd_and_hms(2024, 1, 8, 8, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        let day_close = chrono_tz::America::Chicago
            .with_ymd_and_hms(2024, 1, 8, 13, 20, 0)
            .unwrap()
            .with_timezone(&Utc);

        assert!(!cal.is_open(morning_break));
        assert_eq!(cal.next_open(morning_break), Some(day_open));
        assert_eq!(cal.next_close(morning_break), Some(day_close));
    }

    #[test]
    fn commodity_category_aliases_resolve_to_expected_templates() {
        for code in [
            "CBOT_OILSEEDS",
            "CBOT_WHEAT",
            "CBOT_CORN",
            "CBOT_SOYBEANS",
            "GLOBEX_GRAINS",
            "ZC",
            "ZW",
            "ZS",
            "ZL",
            "ZM",
            "ZO",
            "KE",
            "HRS",
        ] {
            let cal = calendar_for_exchange(code).unwrap();
            let th = cal.trading_hours.as_ref().unwrap();
            assert_eq!(th.sessions.len(), 2, "{code}");
            assert_eq!(th.sessions[0].open.hour(), 19, "{code}");
            assert_eq!(th.sessions[0].open_day_offset, -1, "{code}");
            assert_eq!(th.sessions[1].open.hour(), 8, "{code}");
            assert_eq!(th.sessions[1].open.minute(), 30, "{code}");
        }

        for code in [
            "CME_ENERGY",
            "GLOBEX_ENERGY",
            "CME_METALS",
            "GLOBEX_METALS",
            "CL",
            "MCL",
            "QM",
            "GC",
            "MGC",
            "QO",
        ] {
            let cal = calendar_for_exchange(code).unwrap();
            let th = cal.trading_hours.as_ref().unwrap();
            assert_eq!(th.sessions.len(), 1, "{code}");
            assert_eq!(th.sessions[0].open.hour(), 17, "{code}");
            assert_eq!(th.sessions[0].open_day_offset, -1, "{code}");
            assert_eq!(th.sessions[0].close.hour(), 16, "{code}");
        }

        for code in ["CME_DAIRY", "GLOBEX_DAIRY", "SR3", "ES", "NQ", "RTY"] {
            let cal = calendar_for_exchange(code).unwrap();
            let th = cal.trading_hours.as_ref().unwrap();
            assert_eq!(th.sessions.len(), 1, "{code}");
            assert_eq!(th.sessions[0].open.hour(), 17, "{code}");
            assert_eq!(th.sessions[0].open_day_offset, -1, "{code}");
            assert_eq!(th.sessions[0].close.hour(), 16, "{code}");
        }

        for code in ["CME_LIVESTOCK", "GLOBEX_LIVESTOCK", "LE", "GF", "HE"] {
            let cal = calendar_for_exchange(code).unwrap();
            let th = cal.trading_hours.as_ref().unwrap();
            assert_eq!(th.sessions.len(), 1, "{code}");
            assert_eq!(th.sessions[0].open.hour(), 8, "{code}");
            assert_eq!(th.sessions[0].open.minute(), 30, "{code}");
            assert_eq!(th.sessions[0].open_day_offset, 0, "{code}");
            assert_eq!(th.sessions[0].close.hour(), 13, "{code}");
            assert_eq!(th.sessions[0].close.minute(), 5, "{code}");
        }

        for code in ["CME_LUMBER", "GLOBEX_LUMBER", "LBR", "LS"] {
            let cal = calendar_for_exchange(code).unwrap();
            let th = cal.trading_hours.as_ref().unwrap();
            assert_eq!(th.sessions.len(), 1, "{code}");
            assert_eq!(th.sessions[0].open.hour(), 9, "{code}");
            assert_eq!(th.sessions[0].open_day_offset, 0, "{code}");
            assert_eq!(th.sessions[0].close.hour(), 15, "{code}");
            assert_eq!(th.sessions[0].close.minute(), 5, "{code}");
        }
    }

    #[test]
    fn cfe_classifies_as_futures() {
        let cal = calendar_for_exchange("CFE").unwrap();
        assert_eq!(cal.market_type, market_type("Futures"));
        // Wed 09:00 CT — open.
        let inst = chrono_tz::America::Chicago
            .with_ymd_and_hms(2024, 1, 10, 9, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
    }

    #[test]
    fn forex_open_tuesday_3am() {
        let cal = calendar_for_exchange("FOREX").unwrap();
        assert_eq!(cal.market_type, market_type("ForeignExchange"));
        let inst = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 9, 3, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
    }

    #[test]
    fn crypto_open_saturday_3am() {
        let cal = calendar_for_exchange("CRYPTO").unwrap();
        assert_eq!(cal.market_type, market_type("DigitalAssets"));
        let inst = chrono_tz::UTC
            .with_ymd_and_hms(2024, 1, 13, 3, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
    }

    #[test]
    fn options_close_at_1615() {
        let cal = calendar_for_exchange("OPRA").unwrap();
        assert_eq!(cal.market_type, market_type("Options"));
        let inst = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 8, 16, 10, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
    }

    #[test]
    fn sifma_includes_columbus_and_veterans() {
        let cal = calendar_for_exchange("SIFMA_US").unwrap();
        assert_eq!(cal.market_type, market_type("FixedIncome"));
        // Veterans Day 2024 = Mon Nov 11.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2024, 11, 11).unwrap()));
        // Columbus Day 2024 = 2nd Mon Oct = Oct 14.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2024, 10, 14).unwrap()));
    }

    #[test]
    fn ice_us_uses_overnight_session() {
        let cal = calendar_for_exchange("ICE_US").unwrap();
        // Sun 21:00 NY should be in Mon's ICE session.
        let inst = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 7, 21, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
    }

    #[test]
    fn all_exchange_codes_resolve() {
        let mut missing = Vec::new();
        for code in EXCHANGE_CODES {
            if calendar_for_exchange(code).is_none() {
                missing.push(*code);
            }
        }
        assert!(missing.is_empty(), "unresolved MICs: {missing:?}");
    }

    #[test]
    fn exchange_codes_are_sourced_from_finance_enums() {
        assert_eq!(EXCHANGE_CODES, finance_enums::data::ExchangeCode_VARIANTS);
        assert!(std::ptr::eq(
            EXCHANGE_CODES.as_ptr(),
            finance_enums::data::ExchangeCode_VARIANTS.as_ptr()
        ));
    }

    #[test]
    fn market_type_variants_match_finance_enum_values() {
        let expected: &[&str] = &[
            "Equities",
            "FixedIncome",
            "ForeignExchange",
            "Commodities",
            "Derivatives",
            "Options",
            "Futures",
            "Funds",
            "DigitalAssets",
            "OverTheCounter",
        ];
        assert_eq!(MARKET_TYPES, expected);
        assert!(!MARKET_TYPES.contains(&"Other"));
    }

    #[test]
    fn market_type_lookup_uses_finance_enum_variant_names() {
        assert_eq!(market_type("Options"), "Options");
        assert_eq!(market_type("Futures"), "Futures");
        assert!(MARKET_TYPES.contains(&market_type("Options")));
    }

    #[test]
    fn calendar_for_asset_uses_finance_enum_asset_names() {
        let gas = calendar_for_asset("XNYM", "Commodity", Some("NaturalGas")).unwrap();
        assert_eq!(gas.market_type, market_type("Futures"));
        assert_eq!(gas.name, "XNYM:NaturalGas");

        let grains = calendar_for_asset("XCBT", "Agriculture", None).unwrap();
        assert_eq!(grains.market_type, market_type("Futures"));
        assert_eq!(grains.name, "XCBT:Agriculture");
        assert_eq!(grains.trading_hours.unwrap().sessions.len(), 2);

        let equity = calendar_for_asset("XNYS", "Equity", None).unwrap();
        assert_eq!(equity.market_type, market_type("Equities"));
        assert_eq!(equity.name, "XNYS");

        assert!(calendar_for_asset("XNYS", "NotAnAssetClass", None).is_none());
    }

    #[test]
    fn otc_inherits_nyse_holidays() {
        let cal = calendar_for_exchange("PINX").unwrap();
        assert_eq!(cal.market_type, market_type("Equities"));
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2024, 7, 4).unwrap()));
    }

    #[test]
    fn canadian_calendar_for_neoe() {
        let cal = calendar_for_exchange("NEOE").unwrap();
        // Canada Day 2024 = Mon Jul 1.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2024, 7, 1).unwrap()));
    }

    #[test]
    fn nyse_july3_2024_is_early_close() {
        // July 4 2024 was Thursday; July 3 (Wed) had a 13:00 ET early close.
        let cal = calendar_for_exchange("XNYS").unwrap();
        let day = NaiveDate::from_ymd_opt(2024, 7, 3).unwrap();
        assert_eq!(
            cal.early_close_for(day),
            Some(NaiveTime::from_hms_opt(13, 0, 0).unwrap())
        );
        // 14:00 ET that day should be CLOSED.
        let inst = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 7, 3, 14, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(!cal.is_open(inst));
        // 12:30 ET should be OPEN.
        let inst2 = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 7, 3, 12, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst2));
    }

    #[test]
    fn nyse_black_friday_2024_early_close() {
        // 2024 Thanksgiving = Thu Nov 28; Black Friday = Nov 29.
        let cal = calendar_for_exchange("XNYS").unwrap();
        assert_eq!(
            cal.early_close_for(NaiveDate::from_ymd_opt(2024, 11, 29).unwrap()),
            Some(NaiveTime::from_hms_opt(13, 0, 0).unwrap())
        );
    }

    #[test]
    fn xams_no_kingsday_closure() {
        // Euronext Amsterdam does not close for King's Day; it observes only the
        // six harmonized Euronext holidays. Apr 27 2023 was a trading day.
        let cal = calendar_for_exchange("XAMS").unwrap();
        assert!(!cal.is_holiday(NaiveDate::from_ymd_opt(2023, 4, 27).unwrap()));
    }

    #[test]
    fn xkrx_seollal_2024_multi_day() {
        // Korean Lunar NY 2024 spans Feb 9 (Fri) and Feb 12 (Mon).
        let cal = calendar_for_exchange("XKRX").unwrap();
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2024, 2, 9).unwrap()));
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2024, 2, 12).unwrap()));
    }

    #[test]
    fn xtae_uses_sun_thu_weekmask() {
        let cal = calendar_for_exchange("XTAE").unwrap();
        // Sun May 5 2024 should be a business day at TASE.
        assert!(cal.is_business_day(NaiveDate::from_ymd_opt(2024, 5, 5).unwrap()));
        // Fri May 3 2024 — weekend.
        assert!(!cal.is_business_day(NaiveDate::from_ymd_opt(2024, 5, 3).unwrap()));
    }

    #[test]
    fn xsau_uses_sun_thu_weekmask() {
        let cal = calendar_for_exchange("XSAU").unwrap();
        assert!(cal.is_business_day(NaiveDate::from_ymd_opt(2024, 5, 5).unwrap()));
        assert!(!cal.is_business_day(NaiveDate::from_ymd_opt(2024, 5, 3).unwrap()));
    }

    #[test]
    fn bvmf_carnival_2024() {
        // 2024: Easter Apr 1 → Carnival Mon Feb 12, Tue Feb 13.
        let cal = calendar_for_exchange("BVMF").unwrap();
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2024, 2, 12).unwrap()));
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2024, 2, 13).unwrap()));
    }

    #[test]
    fn region_br_resolves_to_bvmf() {
        let cal = calendar_for_region("BR").unwrap();
        assert_eq!(cal.name, "BVMF");
        assert_eq!(calendar_for_region("BRA").unwrap().name, "BVMF");
    }

    #[test]
    fn xnze_waitangi_2024() {
        let cal = calendar_for_exchange("XNZE").unwrap();
        // Feb 6 2024 = Tuesday.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2024, 2, 6).unwrap()));
    }

    #[test]
    fn apac_lunch_break_calendars_expose_split_sessions() {
        let cases = [
            ("XTKS", vec![((9, 0), (11, 30)), ((12, 30), (15, 30))]),
            ("XHKG", vec![((9, 30), (12, 0)), ((13, 0), (16, 0))]),
            ("XSHG", vec![((9, 30), (11, 30)), ((13, 0), (15, 0))]),
        ];

        for (code, expected) in cases {
            let cal = calendar_for_exchange(code).unwrap();
            let th = cal.trading_hours.as_ref().unwrap();
            let actual: Vec<_> = th
                .sessions
                .iter()
                .map(|session| {
                    (
                        (session.open.hour(), session.open.minute()),
                        (session.close.hour(), session.close.minute()),
                    )
                })
                .collect();
            assert_eq!(actual, expected, "{code} sessions");
        }
    }

    #[test]
    fn tokyo_lunch_gap_is_closed_and_boundaries_advance() {
        let cal = calendar_for_exchange("XTKS").unwrap();
        let lunch_gap = chrono_tz::Asia::Tokyo
            .with_ymd_and_hms(2026, 5, 25, 11, 45, 0)
            .unwrap()
            .with_timezone(&Utc);
        let afternoon_open = chrono_tz::Asia::Tokyo
            .with_ymd_and_hms(2026, 5, 25, 12, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        let afternoon_close = chrono_tz::Asia::Tokyo
            .with_ymd_and_hms(2026, 5, 25, 15, 30, 0)
            .unwrap()
            .with_timezone(&Utc);

        assert!(!cal.is_open(lunch_gap));
        assert_eq!(cal.next_open(lunch_gap), Some(afternoon_open));
        assert_eq!(cal.next_close(lunch_gap), Some(afternoon_close));

        let sessions = cal.sessions_between(
            NaiveDate::from_ymd_opt(2026, 5, 25).unwrap(),
            NaiveDate::from_ymd_opt(2026, 5, 25).unwrap(),
        );
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn tokyo_uses_historical_close_before_2024_schedule_change() {
        let cal = calendar_for_exchange("XTKS").unwrap();
        let before = cal.sessions_between(
            NaiveDate::from_ymd_opt(2024, 11, 1).unwrap(),
            NaiveDate::from_ymd_opt(2024, 11, 1).unwrap(),
        );
        let after = cal.sessions_between(
            NaiveDate::from_ymd_opt(2024, 11, 5).unwrap(),
            NaiveDate::from_ymd_opt(2024, 11, 5).unwrap(),
        );

        let before_close = before[1].1.with_timezone(&chrono_tz::Asia::Tokyo);
        let after_close = after[1].1.with_timezone(&chrono_tz::Asia::Tokyo);
        assert_eq!((before_close.hour(), before_close.minute()), (15, 0));
        assert_eq!((after_close.hour(), after_close.minute()), (15, 30));

        let old_late_afternoon = chrono_tz::Asia::Tokyo
            .with_ymd_and_hms(2024, 11, 1, 15, 15, 0)
            .unwrap()
            .with_timezone(&Utc);
        let current_late_afternoon = chrono_tz::Asia::Tokyo
            .with_ymd_and_hms(2024, 11, 5, 15, 15, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(!cal.is_open(old_late_afternoon));
        assert!(cal.is_open(current_late_afternoon));
    }

    #[test]
    fn session_boundaries_are_explicitly_inclusive_for_next_boundaries() {
        let cal = calendar_for_exchange("XTKS").unwrap();
        let exact_afternoon_open = chrono_tz::Asia::Tokyo
            .with_ymd_and_hms(2026, 5, 25, 12, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        let exact_morning_close = chrono_tz::Asia::Tokyo
            .with_ymd_and_hms(2026, 5, 25, 11, 30, 0)
            .unwrap()
            .with_timezone(&Utc);

        assert!(cal.is_open(exact_afternoon_open));
        assert_eq!(
            cal.next_open(exact_afternoon_open),
            Some(exact_afternoon_open)
        );
        assert!(!cal.is_open(exact_morning_close));
        assert_eq!(
            cal.next_close(exact_morning_close),
            Some(exact_morning_close)
        );
    }

    #[test]
    fn nyse_sessions_between_one_week_with_early_close() {
        let cal = calendar_for_exchange("XNYS").unwrap();
        // Mon Jul 1 — Fri Jul 5 2024. Jul 4 = holiday; Jul 3 = early close.
        let s = cal.sessions_between(
            NaiveDate::from_ymd_opt(2024, 7, 1).unwrap(),
            NaiveDate::from_ymd_opt(2024, 7, 5).unwrap(),
        );
        assert_eq!(s.len(), 4);
        // Jul 3 close (3rd entry) should be 13:00 ET = 17:00 UTC (EDT = UTC-4).
        let jul3_close_local = s[2].1.with_timezone(&chrono_tz::America::New_York);
        assert_eq!(jul3_close_local.hour(), 13);
        assert_eq!(jul3_close_local.minute(), 0);
        // Jul 5 close (4th entry) should be 16:00 ET (regular).
        let jul5_close_local = s[3].1.with_timezone(&chrono_tz::America::New_York);
        assert_eq!(jul5_close_local.hour(), 16);
    }

    #[test]
    fn nyse_extended_sessions_include_pre_open_and_after_close() {
        let cal = calendar_for_exchange("XNYS").unwrap();
        let s = cal.extended_sessions_between(
            NaiveDate::from_ymd_opt(2024, 1, 8).unwrap(),
            NaiveDate::from_ymd_opt(2024, 1, 8).unwrap(),
        );
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].0, "pre_open");
        assert_eq!(s[1].0, "after_close");

        let pre_open_local = s[0].1.with_timezone(&chrono_tz::America::New_York);
        let pre_close_local = s[0].2.with_timezone(&chrono_tz::America::New_York);
        assert_eq!((pre_open_local.hour(), pre_open_local.minute()), (4, 0));
        assert_eq!((pre_close_local.hour(), pre_close_local.minute()), (9, 30));

        let after_open_local = s[1].1.with_timezone(&chrono_tz::America::New_York);
        let after_close_local = s[1].2.with_timezone(&chrono_tz::America::New_York);
        assert_eq!(
            (after_open_local.hour(), after_open_local.minute()),
            (16, 0)
        );
        assert_eq!(
            (after_close_local.hour(), after_close_local.minute()),
            (20, 0)
        );
    }

    #[test]
    fn nyse_after_close_starts_at_early_close() {
        let cal = calendar_for_exchange("XNYS").unwrap();
        let s = cal.extended_sessions_between(
            NaiveDate::from_ymd_opt(2024, 7, 3).unwrap(),
            NaiveDate::from_ymd_opt(2024, 7, 3).unwrap(),
        );
        let after_open_local = s[1].1.with_timezone(&chrono_tz::America::New_York);
        let after_close_local = s[1].2.with_timezone(&chrono_tz::America::New_York);
        assert_eq!(
            (after_open_local.hour(), after_open_local.minute()),
            (13, 0)
        );
        assert_eq!(
            (after_close_local.hour(), after_close_local.minute()),
            (20, 0)
        );
    }

    #[test]
    fn nyse_holidays_between_q3_2024() {
        let cal = calendar_for_exchange("XNYS").unwrap();
        let h = cal.holidays_between(
            NaiveDate::from_ymd_opt(2024, 7, 1).unwrap(),
            NaiveDate::from_ymd_opt(2024, 9, 30).unwrap(),
        );
        // Jul 4 (Independence Day) and Sep 2 (Labor Day).
        assert!(h.contains(&NaiveDate::from_ymd_opt(2024, 7, 4).unwrap()));
        assert!(h.contains(&NaiveDate::from_ymd_opt(2024, 9, 2).unwrap()));
        assert_eq!(h.len(), 2);
    }
}
