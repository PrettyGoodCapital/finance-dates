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
use crate::range::{
    business_day_range, business_days_between, next_business_day, previous_business_day,
    STANDARD_WEEKMASK,
};
use crate::trading_hours::{Session, TradingHours};

/// The class of instrument a calendar represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketType {
    /// Cash equities and ETFs.
    Equity,
    /// Listed options.
    Options,
    /// Listed futures and futures options.
    Futures,
    /// Spot / margin FX.
    Fx,
    /// Fixed-income (SIFMA-style).
    Bond,
    /// 24x7 crypto.
    Crypto,
    /// Other / unknown.
    Other,
}

impl MarketType {
    pub fn as_str(self) -> &'static str {
        match self {
            MarketType::Equity => "equity",
            MarketType::Options => "options",
            MarketType::Futures => "futures",
            MarketType::Fx => "fx",
            MarketType::Bond => "bond",
            MarketType::Crypto => "crypto",
            MarketType::Other => "other",
        }
    }
}

/// Mon-Sun all-true weekmask (used by 24x7 crypto venues).
pub const CRYPTO_WEEKMASK: [bool; 7] = [true, true, true, true, true, true, true];

/// Sun-Fri weekmask used by 24x5 FX. Monday=index 0; Sunday=index 6.
pub const FX_WEEKMASK: [bool; 7] = [true, true, true, true, true, false, true];

/// All MIC codes recognised by `calendar_for_exchange`.
pub const EXCHANGE_CODES: &[&str] = &[
    // US equities (NYSE family)
    "XNYS", "NYSD", "XCIS", "CISD", "XCHI", "ARCX", "ARCD", "ARCO", "XASE", "AMXO",
    "XNAS", "XNGS", "XNCM", "XNMS", "NASD", "XNDQ",
    "XBOS", "BOSD", "XBXO", "XPHL", "XPSX", "PSXD", "XPHO", "XPBT", "XPOR", "XNFI",
    "EDGA", "EDGD", "EDGX", "EDDP", "EDGO",
    "BATS", "BZXD", "BATO", "BATY", "BYXD",
    "MEMX", "MEMD", "IEXG", "LTSE",
    "MIHI", "MPRL", "EPRL", "EPRD", "XMIO", "EMLD",
    // US options
    "XISE", "GMNI", "MCRY", "XCBO", "C2OX", "MXOP", "OPRA",
    // OTC / FINRA
    "OTCM", "CAVE", "OTCB", "OTCQ", "PINL", "PINI", "PINX", "PSGM", "PINC",
    "FINR", "FINN", "FINC", "FINY", "XADF", "FINO", "OOTC",
    // US futures (CME group)
    "XCME", "FCME", "GLBX", "XCBT", "FCBT", "XKBT", "XNYM",
    // Canada
    "XTSE", "XDRK", "VDRK", "XTSX", "XTNX", "XATS", "XATX", "ADRK", "XMOD", "XMOC",
    "NEOE", "NEOD", "NEON", "NEOC", "XCNQ", "PURE", "CSE2",
    // Major non-US equities
    "XLON", "XTKS", "XHKG", "XSHG", "XEUR", "XPAR", "XFRA", "XASX", "XBOM", "XNSE",
    // Europe (additional)
    "XAMS", "XBRU", "XLIS", "XMIL", "XMAD", "XSWX", "XOSL", "XSTO", "XHEL",
    "XCSE", "XICE", "XWAR", "XPRA", "XBUD", "XWBO", "XDUB",
    // Asia / Pacific
    "XKRX", "XSES", "XTAI", "XBKK", "XKLS", "XIDX", "XPHS", "XNZE",
    // EMEA
    "XJSE", "XSAU", "XIST", "XTAE", "XDFM", "XADS",
    // LatAm
    "BVMF", "XMEX", "XBUE", "XSGO", "XLIM", "XBOG",
    // Synthetic / placeholder
    "XXXX", "PYPR", "SIMU",
    // Generic market families
    "FOREX", "CRYPTO", "SIFMA_US", "ICE_US", "CFE",
];

/// ISO region codes recognised by `calendar_for_region`.
pub const REGION_CODES: &[&str] = &[
    "US", "UK", "GB", "EU", "JP", "HK", "CN", "CA", "AU", "IN", "DE", "FR",
    "NL", "BE", "PT", "IT", "ES", "CH", "NO", "SE", "FI", "DK", "IS",
    "PL", "CZ", "HU", "AT", "IE",
    "KR", "SG", "TW", "TH", "MY", "ID", "PH", "NZ",
    "ZA", "SA", "TR", "IL", "AE",
    "BR", "MX", "AR", "CL", "PE", "CO",
];

/// A holiday calendar with optional trading hours and a market classification.
pub struct Calendar {
    pub name: String,
    pub market_type: MarketType,
    pub weekmask: [bool; 7],
    pub rules: Vec<HolidayRule>,
    pub trading_hours: Option<TradingHours>,
    /// Days when the venue closes earlier than usual. Each rule resolves to
    /// at most one date per year, paired with a local close time that
    /// replaces the normal session close on that date.
    pub early_closes: Vec<EarlyCloseRule>,
    cache: HolidayCache,
    early_cache: EarlyCloseCache,
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
        Self::with_type(name, MarketType::Equity, weekmask, rules, trading_hours)
    }

    pub fn with_type(
        name: impl Into<String>,
        market_type: MarketType,
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
            early_closes: Vec::new(),
            cache: HolidayCache::default(),
            early_cache: EarlyCloseCache::default(),
        }
    }

    /// Builder: attach early-close rules.
    pub fn with_early_closes(mut self, ec: Vec<EarlyCloseRule>) -> Self {
        self.early_closes = ec;
        self
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
                if !self.weekmask[i] {
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
        for r in &self.rules {
            for d in r.dates_in(year) {
                set.insert(d);
            }
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
        self.weekmask[i] && !self.is_holiday(d)
    }

    pub fn next_business_day(&self, d: NaiveDate) -> NaiveDate {
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

    /// True iff the venue is currently in any trading session.
    ///
    /// For sessions that span midnight, the trading-day check considers both
    /// the local calendar day of `when` and the next local calendar day, so a
    /// Sun-evening CME open correctly maps to Mon's trading day. If an
    /// early-close is in effect for that trading day, the last session's
    /// close is shortened.
    pub fn is_open(&self, when: DateTime<Utc>) -> bool {
        let Some(th) = &self.trading_hours else { return false };
        let local_today = when.with_timezone(&th.timezone).date_naive();
        for delta in [0i64, 1] {
            let trading_day = local_today + Duration::days(delta);
            if !self.is_business_day(trading_day) {
                continue;
            }
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

// ---------- Holiday rule constructors ----------

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

// ---------- Built-in calendars ----------

fn nyse_rules() -> Vec<HolidayRule> {
    vec![
        fixed(1, 1, None),
        nth(1, Weekday::Mon, 3),
        nth(2, Weekday::Mon, 3),
        easter(-2),
        nth(5, Weekday::Mon, -1),
        fixed(6, 19, Some(2021)),
        fixed(7, 4, None),
        nth(9, Weekday::Mon, 1),
        nth(11, Weekday::Thu, 4),
        fixed(12, 25, None),
    ]
}

fn nyse_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        chrono_tz::America::New_York,
    )
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
    vec![
        fixed(1, 1, None),
        easter(-2),
        fixed(12, 25, None),
    ]
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

/// CME Globex energy & metals: 17:00 prev — 16:00 today CT (with one-hour
/// daily break that this layer treats as a single contiguous session).
fn cme_globex_energy_hours() -> TradingHours {
    TradingHours::from_sessions(
        vec![Session::overnight(
            NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        )],
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

fn lse_rules() -> Vec<HolidayRule> {
    vec![
        fixed(1, 1, None),
        easter(-2),
        easter(1),
        nth(5, Weekday::Mon, 1),
        nth(5, Weekday::Mon, -1),
        nth(8, Weekday::Mon, -1),
        fixed(12, 25, None),
        fixed(12, 26, None),
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
        fixed_no_roll(1, 1, None),
        fixed_no_roll(1, 2, None),
        fixed_no_roll(1, 3, None),
        nth(1, Weekday::Mon, 2),
        fixed_no_roll(2, 11, None),
        fixed_no_roll(2, 23, Some(2020)),
        fixed_no_roll(4, 29, None),
        fixed_no_roll(5, 3, None),
        fixed_no_roll(5, 4, None),
        fixed_no_roll(5, 5, None),
        nth(7, Weekday::Mon, 3),
        fixed_no_roll(8, 11, None),
        nth(9, Weekday::Mon, 3),
        nth(10, Weekday::Mon, 2),
        fixed_no_roll(11, 3, None),
        fixed_no_roll(11, 23, None),
        fixed_no_roll(12, 31, None),
    ]
}

fn tse_trading_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
        chrono_tz::Asia::Tokyo,
    )
}

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
        fixed(7, 1, None),
        fixed(10, 1, None),
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
        fixed(10, 3, None),
        fixed(12, 24, None),
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
        nth(2, Weekday::Mon, 3),
        easter(-2),
        nth(5, Weekday::Mon, -1),
        fixed(7, 1, None),
        nth(8, Weekday::Mon, 1),
        nth(9, Weekday::Mon, 1),
        nth(10, Weekday::Mon, 2),
        fixed(12, 25, None),
        fixed(12, 26, None),
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
        fixed(1, 26, None),
        easter(-2),
        easter(1),
        fixed(4, 25, None),
        nth(6, Weekday::Mon, 2),
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

// ---------- Early-close rule helpers ----------

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
        (2020, 11, 27), (2021, 11, 26), (2022, 11, 25), (2023, 11, 24),
        (2024, 11, 29), (2025, 11, 28), (2026, 11, 27), (2027, 11, 26),
        (2028, 11, 24), (2029, 11, 23), (2030, 11, 29), (2031, 11, 28),
        (2032, 11, 26), (2033, 11, 25), (2034, 11, 24), (2035, 11, 23),
    ];
    vec![
        ec(HolidayRule::Tabulated { table: BLACK_FRIDAY }, 13, 0),
        ec(fixed_no_roll(12, 24, None), 13, 0),
        ec(fixed_no_roll(7, 3, None), 13, 0),
    ]
}

// ---------- Additional non-US equity calendars ----------

/// Generic European Christian-calendar holidays: NY, Good Friday,
/// Easter Monday, May Day, Christmas, Boxing Day. Used as a baseline.
fn euro_basic_rules() -> Vec<HolidayRule> {
    vec![
        fixed(1, 1, None),
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
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

fn xams_rules() -> Vec<HolidayRule> {
    // Amsterdam: NY, Good Friday, Easter Mon, King's Day (Apr 27, since 2014),
    // Ascension (+39), Whit Monday (+50), Christmas, Boxing Day.
    vec![
        fixed(1, 1, None),
        easter(-2),
        easter(1),
        fixed_no_roll(4, 27, Some(2014)),
        easter(39),
        easter(50),
        fixed(12, 25, None),
        fixed(12, 26, None),
    ]
}

fn xbru_rules() -> Vec<HolidayRule> {
    // Brussels: NY, Good Friday, Easter Mon, Labour, Ascension, Whit Mon,
    // Christmas, Boxing Day.
    vec![
        fixed(1, 1, None),
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        easter(39),
        easter(50),
        fixed(12, 25, None),
        fixed(12, 26, None),
    ]
}

fn xlis_rules() -> Vec<HolidayRule> {
    // Lisbon: subset of euro_basic + Carnival (Easter -47).
    let mut r = euro_basic_rules();
    r.push(easter(-47));
    r
}

fn xmil_rules() -> Vec<HolidayRule> {
    // Borsa Italiana (Milan): NY, Epiphany (Jan 6), Easter Mon, Liberation
    // Day (Apr 25), Labour, Republic Day (Jun 2), Assumption (Aug 15),
    // All Saints (Nov 1), Immaculate Conception (Dec 8), Christmas, Boxing.
    vec![
        fixed(1, 1, None),
        fixed_no_roll(1, 6, None),
        easter(-2),
        easter(1),
        fixed_no_roll(4, 25, None),
        fixed(5, 1, None),
        fixed_no_roll(6, 2, None),
        fixed_no_roll(8, 15, None),
        fixed_no_roll(11, 1, None),
        fixed_no_roll(12, 8, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
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
    // BME Madrid: NY, Epiphany, Good Friday, Easter Mon, Labour,
    // Assumption, National Day (Oct 12), All Saints, Constitution (Dec 6),
    // Immaculate (Dec 8), Christmas, Boxing.
    vec![
        fixed(1, 1, None),
        fixed_no_roll(1, 6, None),
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        fixed_no_roll(8, 15, None),
        fixed_no_roll(10, 12, None),
        fixed_no_roll(11, 1, None),
        fixed_no_roll(12, 6, None),
        fixed_no_roll(12, 8, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
    ]
}

fn xmad_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 30, 0).unwrap(),
        chrono_tz::Europe::Madrid,
    )
}

fn xswx_rules() -> Vec<HolidayRule> {
    // SIX Swiss: NY, Berchtold (Jan 2), Good Friday, Easter Mon, Labour,
    // Ascension, Whit Mon, Swiss National (Aug 1), Christmas, Boxing.
    vec![
        fixed(1, 1, None),
        fixed_no_roll(1, 2, None),
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        easter(39),
        easter(50),
        fixed_no_roll(8, 1, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
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
    // Oslo Børs: NY, Maundy Thu (-3), Good Friday, Easter Mon, Labour,
    // Constitution (May 17), Ascension, Whit Mon, Christmas Eve (half),
    // Christmas, Boxing, NYE (half).
    vec![
        fixed(1, 1, None),
        easter(-3),
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        fixed_no_roll(5, 17, None),
        easter(39),
        easter(50),
        fixed(12, 25, None),
        fixed(12, 26, None),
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
    // Stockholm OMX: NY, Epiphany, Good Friday, Easter Mon, Labour,
    // Ascension, National Day (Jun 6), Midsummer Eve (Fri before Jun 20-26),
    // Christmas Eve, Christmas, Boxing, NYE.
    vec![
        fixed(1, 1, None),
        fixed_no_roll(1, 6, None),
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        easter(39),
        fixed_no_roll(6, 6, None),
        fixed_no_roll(12, 24, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
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
    // Helsinki: NY, Epiphany, Good Friday, Easter Mon, Labour,
    // Ascension, Midsummer Eve (skip), Independence Day (Dec 6),
    // Christmas Eve, Christmas, Boxing.
    vec![
        fixed(1, 1, None),
        fixed_no_roll(1, 6, None),
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        easter(39),
        fixed_no_roll(12, 6, None),
        fixed_no_roll(12, 24, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
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
    // (was Easter+26, abolished 2024), Ascension, Constitution (Jun 5),
    // Christmas Eve, Christmas, Boxing.
    vec![
        fixed(1, 1, None),
        easter(-3),
        easter(-2),
        easter(1),
        easter(39),
        fixed_no_roll(6, 5, None),
        fixed_no_roll(12, 24, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
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
    // Iceland: NY, Maundy Thu, Good Fri, Easter Mon, First Day of Summer
    // (skip), Labour, Ascension, Whit Mon, National Day (Jun 17),
    // Commerce Day (skip), Christmas Eve, Christmas, Boxing.
    vec![
        fixed(1, 1, None),
        easter(-3),
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        easter(39),
        easter(50),
        fixed_no_roll(6, 17, None),
        fixed_no_roll(12, 24, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
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
    // Warsaw: NY, Epiphany, Easter Mon, Labour, Constitution (May 3),
    // Corpus Christi (+60), Assumption, All Saints, Independence (Nov 11),
    // Christmas, Boxing.
    vec![
        fixed(1, 1, None),
        fixed_no_roll(1, 6, None),
        easter(1),
        fixed(5, 1, None),
        fixed_no_roll(5, 3, None),
        easter(60),
        fixed_no_roll(8, 15, None),
        fixed_no_roll(11, 1, None),
        fixed_no_roll(11, 11, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
    ]
}

fn xwar_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        chrono_tz::Europe::Warsaw,
    )
}

fn xpra_rules() -> Vec<HolidayRule> {
    // Prague: NY, Good Friday, Easter Mon, Labour, Liberation (May 8),
    // Ss Cyril & Methodius (Jul 5), Jan Hus (Jul 6), Statehood (Sep 28),
    // Independence (Oct 28), Freedom (Nov 17), Christmas Eve, Christmas, Boxing.
    vec![
        fixed(1, 1, None),
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        fixed_no_roll(5, 8, None),
        fixed_no_roll(7, 5, None),
        fixed_no_roll(7, 6, None),
        fixed_no_roll(9, 28, None),
        fixed_no_roll(10, 28, None),
        fixed_no_roll(11, 17, None),
        fixed_no_roll(12, 24, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
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
    // Budapest: NY, 1848 Revolution (Mar 15), Good Friday, Easter Mon,
    // Labour, Whit Mon, State Foundation (Aug 20), 1956 Revolution (Oct 23),
    // All Saints, Christmas, Boxing.
    vec![
        fixed(1, 1, None),
        fixed_no_roll(3, 15, None),
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        easter(50),
        fixed_no_roll(8, 20, None),
        fixed_no_roll(10, 23, None),
        fixed_no_roll(11, 1, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
    ]
}

fn xbud_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        chrono_tz::Europe::Budapest,
    )
}

fn xwbo_rules() -> Vec<HolidayRule> {
    // Vienna (Wiener Börse): NY, Good Friday, Easter Mon, Labour,
    // Ascension, Whit Mon, Corpus Christi, Assumption, National (Oct 26),
    // All Saints, Immaculate (Dec 8), Christmas Eve, Christmas, Boxing.
    vec![
        fixed(1, 1, None),
        easter(-2),
        easter(1),
        fixed(5, 1, None),
        easter(39),
        easter(50),
        easter(60),
        fixed_no_roll(8, 15, None),
        fixed_no_roll(10, 26, None),
        fixed_no_roll(11, 1, None),
        fixed_no_roll(12, 8, None),
        fixed_no_roll(12, 24, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
    ]
}

fn xwbo_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(17, 30, 0).unwrap(),
        chrono_tz::Europe::Vienna,
    )
}

fn xdub_rules() -> Vec<HolidayRule> {
    // Euronext Dublin: NY, Saint Patrick (Mar 17), Good Friday, Easter Mon,
    // May Day (1st Mon), June Bank (1st Mon), August Bank (1st Mon),
    // October Bank (last Mon), Christmas, Boxing.
    vec![
        fixed(1, 1, None),
        fixed(3, 17, None),
        easter(-2),
        easter(1),
        nth(5, Weekday::Mon, 1),
        nth(6, Weekday::Mon, 1),
        nth(8, Weekday::Mon, 1),
        nth(10, Weekday::Mon, -1),
        fixed(12, 25, None),
        fixed(12, 26, None),
    ]
}

fn xdub_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 28, 0).unwrap(),
        chrono_tz::Europe::Dublin,
    )
}

// ---------- Asia / Pacific ----------

fn xkrx_rules() -> Vec<HolidayRule> {
    // Korea Exchange: tabulated lunar holidays (Seollal, Chuseok). For
    // accuracy these are baked in as lookup tables 2020-2030.
    let seollal: &'static [(i32, u32, u32)] = &[
        (2020, 1, 24), (2020, 1, 27),
        (2021, 2, 11), (2021, 2, 12),
        (2022, 1, 31), (2022, 2, 1), (2022, 2, 2),
        (2023, 1, 23), (2023, 1, 24),
        (2024, 2, 9), (2024, 2, 12),
        (2025, 1, 28), (2025, 1, 29), (2025, 1, 30),
        (2026, 2, 16), (2026, 2, 17), (2026, 2, 18),
    ];
    let chuseok: &'static [(i32, u32, u32)] = &[
        (2020, 9, 30), (2020, 10, 1), (2020, 10, 2),
        (2021, 9, 20), (2021, 9, 21), (2021, 9, 22),
        (2022, 9, 9), (2022, 9, 12),
        (2023, 9, 28), (2023, 9, 29),
        (2024, 9, 16), (2024, 9, 17), (2024, 9, 18),
        (2025, 10, 6), (2025, 10, 7), (2025, 10, 8),
        (2026, 9, 24), (2026, 9, 25),
    ];
    vec![
        fixed(1, 1, None),
        HolidayRule::Tabulated { table: seollal },
        fixed_no_roll(3, 1, None),       // Independence Movement
        fixed_no_roll(5, 5, None),       // Children's Day
        fixed_no_roll(6, 6, None),       // Memorial Day
        fixed_no_roll(8, 15, None),      // Liberation Day
        HolidayRule::Tabulated { table: chuseok },
        fixed_no_roll(10, 3, None),      // National Foundation
        fixed_no_roll(10, 9, None),      // Hangul Day
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
        (2020, 1, 24), (2021, 2, 12), (2022, 2, 1), (2023, 1, 23),
        (2024, 2, 12), (2025, 1, 29), (2026, 2, 17),
    ];
    let lny2: &'static [(i32, u32, u32)] = &[
        (2020, 1, 27), (2021, 2, 15), (2022, 2, 2), (2023, 1, 24),
        (2024, 2, 13), (2025, 1, 30), (2026, 2, 18),
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
        (2020, 1, 23), (2021, 2, 8), (2022, 1, 27), (2023, 1, 19),
        (2024, 2, 5), (2025, 1, 23), (2026, 2, 13),
    ];
    vec![
        fixed(1, 1, None),
        HolidayRule::Tabulated { table: lny },
        fixed_no_roll(2, 28, None),      // Peace Memorial
        fixed_no_roll(4, 4, None),       // Children's
        fixed_no_roll(4, 5, None),       // Tomb Sweeping
        fixed(5, 1, None),
        fixed_no_roll(10, 10, None),     // ROC National
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
        (2020, 1, 27), (2021, 2, 12), (2022, 2, 1), (2023, 1, 23),
        (2024, 2, 12), (2025, 1, 29), (2026, 2, 17),
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
        (2020, 1, 27), (2021, 2, 12), (2022, 2, 1), (2023, 1, 23),
        (2024, 2, 8), (2025, 1, 29), (2026, 2, 17),
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

fn xnze_rules() -> Vec<HolidayRule> {
    // NZX (New Zealand): NY (Jan 1, Jan 2 observed), Waitangi (Feb 6),
    // Good Fri, Easter Mon, ANZAC (Apr 25), King's Birthday (1st Mon Jun),
    // Matariki (variable, skipped here), Labour Day (4th Mon Oct),
    // Christmas, Boxing Day.
    vec![
        fixed(1, 1, None),
        fixed(1, 2, None),
        fixed(2, 6, None),
        easter(-2),
        easter(1),
        fixed(4, 25, None),
        nth(6, Weekday::Mon, 1),
        nth(10, Weekday::Mon, 4),
        fixed(12, 25, None),
        fixed(12, 26, None),
    ]
}

fn xnze_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 45, 0).unwrap(),
        chrono_tz::Pacific::Auckland,
    )
}

// ---------- EMEA ----------

fn xjse_rules() -> Vec<HolidayRule> {
    // Johannesburg: NY, Human Rights (Mar 21), Good Fri, Family Day (Easter
    // Mon), Freedom (Apr 27), Workers (May 1), Youth (Jun 16), National
    // Women's (Aug 9), Heritage (Sep 24), Day of Reconciliation (Dec 16),
    // Christmas, Day of Goodwill (Dec 26).
    vec![
        fixed(1, 1, None),
        fixed(3, 21, None),
        easter(-2),
        easter(1),
        fixed(4, 27, None),
        fixed(5, 1, None),
        fixed(6, 16, None),
        fixed(8, 9, None),
        fixed(9, 24, None),
        fixed(12, 16, None),
        fixed(12, 25, None),
        fixed(12, 26, None),
    ]
}

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
        (2020, 5, 24), (2021, 5, 13), (2022, 5, 2), (2023, 4, 21),
        (2024, 4, 10), (2025, 3, 30), (2026, 3, 20),
    ];
    let eid_adha: &'static [(i32, u32, u32)] = &[
        (2020, 7, 31), (2021, 7, 20), (2022, 7, 9), (2023, 6, 28),
        (2024, 6, 16), (2025, 6, 6), (2026, 5, 27),
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
        (2020, 5, 24), (2021, 5, 13), (2022, 5, 2), (2023, 4, 21),
        (2024, 4, 10), (2025, 3, 30), (2026, 3, 20),
    ];
    let eid_adha: &'static [(i32, u32, u32)] = &[
        (2020, 7, 31), (2021, 7, 20), (2022, 7, 9), (2023, 6, 28),
        (2024, 6, 16), (2025, 6, 6), (2026, 5, 27),
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
        (2020, 3, 10), (2021, 2, 26), (2022, 3, 17), (2023, 3, 7),
        (2024, 3, 24), (2025, 3, 14), (2026, 3, 3),
    ];
    let passover_eve: &'static [(i32, u32, u32)] = &[
        (2020, 4, 8), (2021, 3, 27), (2022, 4, 15), (2023, 4, 5),
        (2024, 4, 22), (2025, 4, 12), (2026, 4, 1),
    ];
    let shavuot: &'static [(i32, u32, u32)] = &[
        (2020, 5, 29), (2021, 5, 17), (2022, 6, 5), (2023, 5, 26),
        (2024, 6, 12), (2025, 6, 2), (2026, 5, 22),
    ];
    let rosh: &'static [(i32, u32, u32)] = &[
        (2020, 9, 19), (2021, 9, 7), (2022, 9, 26), (2023, 9, 16),
        (2024, 10, 3), (2025, 9, 23), (2026, 9, 12),
    ];
    let yom_kippur: &'static [(i32, u32, u32)] = &[
        (2020, 9, 28), (2021, 9, 16), (2022, 10, 5), (2023, 9, 25),
        (2024, 10, 12), (2025, 10, 2), (2026, 9, 21),
    ];
    let sukkot: &'static [(i32, u32, u32)] = &[
        (2020, 10, 3), (2021, 9, 21), (2022, 10, 10), (2023, 9, 30),
        (2024, 10, 17), (2025, 10, 7), (2026, 9, 26),
    ];
    let independence: &'static [(i32, u32, u32)] = &[
        (2020, 4, 29), (2021, 4, 15), (2022, 5, 5), (2023, 4, 26),
        (2024, 5, 14), (2025, 5, 1), (2026, 4, 22),
    ];
    vec![
        HolidayRule::Tabulated { table: purim },
        HolidayRule::Tabulated { table: passover_eve },
        HolidayRule::Tabulated { table: shavuot },
        HolidayRule::Tabulated { table: independence },
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
        (2020, 5, 24), (2021, 5, 13), (2022, 5, 2), (2023, 4, 21),
        (2024, 4, 10), (2025, 3, 30), (2026, 3, 20),
    ];
    let eid_adha: &'static [(i32, u32, u32)] = &[
        (2020, 7, 31), (2021, 7, 20), (2022, 7, 9), (2023, 6, 28),
        (2024, 6, 16), (2025, 6, 6), (2026, 5, 27),
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

// ---------- LatAm ----------

fn bvmf_rules() -> Vec<HolidayRule> {
    // B3 / BMF Bovespa (São Paulo): NY, Carnival Mon (-48), Carnival Tue (-47),
    // Good Friday, Tiradentes (Apr 21), Labour, Corpus Christi (+60),
    // Independence (Sep 7), Our Lady of Aparecida (Oct 12), All Souls (Nov 2),
    // Republic (Nov 15), Black Awareness (Nov 20), Christmas Eve, Christmas, NYE.
    vec![
        fixed(1, 1, None),
        easter(-48),
        easter(-47),
        easter(-2),
        fixed(4, 21, None),
        fixed(5, 1, None),
        easter(60),
        fixed(9, 7, None),
        fixed(10, 12, None),
        fixed(11, 2, None),
        fixed(11, 15, None),
        fixed(11, 20, Some(2024)),
        fixed_no_roll(12, 24, None),
        fixed(12, 25, None),
        fixed_no_roll(12, 31, None),
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
    // (3rd Mon Nov), Christmas.
    vec![
        fixed(1, 1, None),
        nth(2, Weekday::Mon, 1),
        nth(3, Weekday::Mon, 3),
        easter(-3),
        easter(-2),
        fixed(5, 1, None),
        fixed(9, 16, None),
        nth(11, Weekday::Mon, 3),
        fixed(12, 25, None),
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
    // Bogotá Stock Exchange (BVC): NY, Epiphany, Saint Joseph, Maundy Thu,
    // Good Fri, Labour, Ascension, Corpus Christi, Sacred Heart, Saint
    // Peter & Paul, Independence (Jul 20), Battle of Boyacá (Aug 7),
    // Assumption, Race Day (Oct 12), All Saints, Independence of Cartagena
    // (Nov 11), Immaculate, Christmas.
    vec![
        fixed(1, 1, None),
        fixed(1, 6, None),
        fixed(3, 19, None),
        easter(-3),
        easter(-2),
        fixed(5, 1, None),
        easter(39),
        easter(60),
        easter(68),
        fixed(7, 20, None),
        fixed(8, 7, None),
        fixed(8, 15, None),
        fixed(10, 12, None),
        fixed(11, 1, None),
        fixed(11, 11, None),
        fixed(12, 8, None),
        fixed(12, 25, None),
    ]
}

fn xbog_hours() -> TradingHours {
    TradingHours::new(
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        chrono_tz::America::Bogota,
    )
}

// ---------- Calendar family resolver ----------

/// Logical calendar family. Many MICs share a family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    UsEquity,
    UsOptions,
    UsBondSifma,
    UsFuturesCme,
    UsFuturesCmeEnergy,
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
        // OTC / FINRA — share the NYSE holiday calendar
        | "OTCM" | "CAVE" | "OTCB" | "OTCQ" | "PINL" | "PINI" | "PINX" | "PSGM"
        | "PINC" | "FINR" | "FINN" | "FINC" | "FINY" | "XADF" | "FINO" | "OOTC"
        // Synthetic / placeholder venues
        | "XXXX" | "PYPR" | "SIMU" => UsEquity,
        // US options
        "XISE" | "GMNI" | "MCRY" | "XCBO" | "C2OX" | "MXOP" | "OPRA" => UsOptions,
        // US futures: CME group equity-index/FX/financials
        "XCME" | "FCME" | "GLBX" | "XCBT" | "FCBT" | "XKBT" => UsFuturesCme,
        // NYMEX (energy/metals) lives under CME group too but with energy hours
        "XNYM" => UsFuturesCmeEnergy,
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
        "XEUR" | "XFRA" => Xetra,
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
            name, MarketType::Equity, STANDARD_WEEKMASK, nyse_rules(),
            Some(nyse_trading_hours()),
        )
        .with_early_closes(nyse_early_closes()),
        UsOptions => Calendar::with_type(
            name, MarketType::Options, STANDARD_WEEKMASK, nyse_rules(),
            Some(options_trading_hours()),
        )
        .with_early_closes(nyse_early_closes()),
        UsBondSifma => Calendar::with_type(
            name, MarketType::Bond, STANDARD_WEEKMASK, sifma_us_rules(),
            Some(sifma_us_hours()),
        ),
        UsFuturesCme => Calendar::with_type(
            name, MarketType::Futures, STANDARD_WEEKMASK, cme_globex_rules(),
            Some(cme_globex_overnight_hours()),
        ),
        UsFuturesCmeEnergy => Calendar::with_type(
            name, MarketType::Futures, STANDARD_WEEKMASK, cme_globex_rules(),
            Some(cme_globex_energy_hours()),
        ),
        UsFuturesIce => Calendar::with_type(
            name, MarketType::Futures, STANDARD_WEEKMASK, ice_us_rules(),
            Some(ice_us_hours()),
        ),
        UsFuturesCfe => Calendar::with_type(
            name, MarketType::Futures, STANDARD_WEEKMASK, cfe_rules(),
            Some(cfe_trading_hours()),
        ),
        Forex24x5 => Calendar::with_type(
            name, MarketType::Fx, STANDARD_WEEKMASK, forex_rules(),
            Some(TradingHours::forex_24x5()),
        ),
        Crypto24x7 => Calendar::with_type(
            name, MarketType::Crypto, CRYPTO_WEEKMASK, crypto_rules(),
            Some(TradingHours::crypto_24x7()),
        ),
        Lse => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, lse_rules(),
            Some(lse_trading_hours()),
        ),
        Tse => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, tse_rules(),
            Some(tse_trading_hours()),
        ),
        Hkex => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, hkex_rules(),
            Some(hkex_trading_hours()),
        ),
        Sse => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, sse_rules(),
            Some(sse_trading_hours()),
        ),
        Xetra => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xetra_rules(),
            Some(xetra_trading_hours()),
        ),
        EuronextParis => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, euronext_paris_rules(),
            Some(euronext_paris_trading_hours()),
        ),
        EuronextAms => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xams_rules(),
            Some(euronext_hours(chrono_tz::Europe::Amsterdam)),
        ),
        EuronextBru => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xbru_rules(),
            Some(euronext_hours(chrono_tz::Europe::Brussels)),
        ),
        EuronextLis => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xlis_rules(),
            Some(euronext_hours(chrono_tz::Europe::Lisbon)),
        ),
        EuronextDub => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xdub_rules(),
            Some(xdub_hours()),
        ),
        Tsx => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, tsx_rules(),
            Some(tsx_trading_hours()),
        ),
        Asx => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, asx_rules(),
            Some(asx_trading_hours()),
        ),
        Nse => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, nse_rules(),
            Some(nse_trading_hours()),
        ),
        Xmil => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xmil_rules(), Some(xmil_hours()),
        ),
        Xmad => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xmad_rules(), Some(xmad_hours()),
        ),
        Xswx => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xswx_rules(), Some(xswx_hours()),
        ),
        Xosl => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xosl_rules(), Some(xosl_hours()),
        ),
        Xsto => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xsto_rules(), Some(xsto_hours()),
        ),
        Xhel => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xhel_rules(), Some(xhel_hours()),
        ),
        Xcse => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xcse_rules(), Some(xcse_hours()),
        ),
        Xice => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xice_rules(), Some(xice_hours()),
        ),
        Xwar => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xwar_rules(), Some(xwar_hours()),
        ),
        Xpra => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xpra_rules(), Some(xpra_hours()),
        ),
        Xbud => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xbud_rules(), Some(xbud_hours()),
        ),
        Xwbo => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xwbo_rules(), Some(xwbo_hours()),
        ),
        Xkrx => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xkrx_rules(), Some(xkrx_hours()),
        ),
        Xses => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xses_rules(), Some(xses_hours()),
        ),
        Xtai => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xtai_rules(), Some(xtai_hours()),
        ),
        Xbkk => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xbkk_rules(), Some(xbkk_hours()),
        ),
        Xkls => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xkls_rules(), Some(xkls_hours()),
        ),
        Xidx => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xidx_rules(), Some(xidx_hours()),
        ),
        Xphs => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xphs_rules(), Some(xphs_hours()),
        ),
        Xnze => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xnze_rules(), Some(xnze_hours()),
        ),
        Xjse => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xjse_rules(), Some(xjse_hours()),
        ),
        Xsau => Calendar::with_type(
            name, MarketType::Equity, MIDEAST_WEEKMASK, xsau_rules(), Some(xsau_hours()),
        ),
        Xist => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xist_rules(), Some(xist_hours()),
        ),
        Xtae => Calendar::with_type(
            name, MarketType::Equity, TASE_WEEKMASK, xtae_rules(), Some(xtae_hours()),
        ),
        Xdfm => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xdfm_rules(), Some(xdfm_hours()),
        ),
        Bvmf => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, bvmf_rules(), Some(bvmf_hours()),
        ),
        Xmex => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xmex_rules(), Some(xmex_hours()),
        ),
        Xbue => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xbue_rules(), Some(xbue_hours()),
        ),
        Xsgo => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xsgo_rules(), Some(xsgo_hours()),
        ),
        Xlim => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xlim_rules(), Some(xlim_hours()),
        ),
        Xbog => Calendar::with_type(
            name, MarketType::Equity, STANDARD_WEEKMASK, xbog_rules(), Some(xbog_hours()),
        ),
    }
}

/// Build a calendar from its MIC code (or a generic family name like
/// `FOREX`, `CRYPTO`, `SIFMA_US`, `ICE_US`, `CFE`). Returns `None` if unknown.
pub fn calendar_for_exchange(code: &str) -> Option<Calendar> {
    let upper = code.to_ascii_uppercase();
    let fam = family_for_mic(&upper)?;
    Some(build_family(&upper, fam))
}

/// Build a calendar from a region code. Returns `None` if unknown.
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
        "NL" => calendar_for_exchange("XAMS"),
        "BE" => calendar_for_exchange("XBRU"),
        "PT" => calendar_for_exchange("XLIS"),
        "IT" => calendar_for_exchange("XMIL"),
        "ES" => calendar_for_exchange("XMAD"),
        "CH" => calendar_for_exchange("XSWX"),
        "NO" => calendar_for_exchange("XOSL"),
        "SE" => calendar_for_exchange("XSTO"),
        "FI" => calendar_for_exchange("XHEL"),
        "DK" => calendar_for_exchange("XCSE"),
        "IS" => calendar_for_exchange("XICE"),
        "PL" => calendar_for_exchange("XWAR"),
        "CZ" => calendar_for_exchange("XPRA"),
        "HU" => calendar_for_exchange("XBUD"),
        "AT" => calendar_for_exchange("XWBO"),
        "IE" => calendar_for_exchange("XDUB"),
        "KR" => calendar_for_exchange("XKRX"),
        "SG" => calendar_for_exchange("XSES"),
        "TW" => calendar_for_exchange("XTAI"),
        "TH" => calendar_for_exchange("XBKK"),
        "MY" => calendar_for_exchange("XKLS"),
        "ID" => calendar_for_exchange("XIDX"),
        "PH" => calendar_for_exchange("XPHS"),
        "NZ" => calendar_for_exchange("XNZE"),
        "ZA" => calendar_for_exchange("XJSE"),
        "SA" => calendar_for_exchange("XSAU"),
        "TR" => calendar_for_exchange("XIST"),
        "IL" => calendar_for_exchange("XTAE"),
        "AE" => calendar_for_exchange("XDFM"),
        "BR" => calendar_for_exchange("BVMF"),
        "MX" => calendar_for_exchange("XMEX"),
        "AR" => calendar_for_exchange("XBUE"),
        "CL" => calendar_for_exchange("XSGO"),
        "PE" => calendar_for_exchange("XLIM"),
        "CO" => calendar_for_exchange("XBOG"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

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
    fn nyse_juneteenth_first_year_2021() {
        let cal = calendar_for_exchange("XNYS").unwrap();
        assert!(!cal.is_holiday(NaiveDate::from_ymd_opt(2020, 6, 19).unwrap()));
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
        assert_eq!(cal.market_type, MarketType::Futures);
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
        assert_eq!(cal.market_type, MarketType::Futures);
        // Mon 09:00 CT → in session (started Sun 17:00 CT).
        let inst = chrono_tz::America::Chicago
            .with_ymd_and_hms(2024, 1, 8, 9, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
    }

    #[test]
    fn cfe_classifies_as_futures() {
        let cal = calendar_for_exchange("CFE").unwrap();
        assert_eq!(cal.market_type, MarketType::Futures);
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
        assert_eq!(cal.market_type, MarketType::Fx);
        let inst = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 9, 3, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
    }

    #[test]
    fn crypto_open_saturday_3am() {
        let cal = calendar_for_exchange("CRYPTO").unwrap();
        assert_eq!(cal.market_type, MarketType::Crypto);
        let inst = chrono_tz::UTC
            .with_ymd_and_hms(2024, 1, 13, 3, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
    }

    #[test]
    fn options_close_at_1615() {
        let cal = calendar_for_exchange("OPRA").unwrap();
        assert_eq!(cal.market_type, MarketType::Options);
        let inst = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 8, 16, 10, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(cal.is_open(inst));
    }

    #[test]
    fn sifma_includes_columbus_and_veterans() {
        let cal = calendar_for_exchange("SIFMA_US").unwrap();
        assert_eq!(cal.market_type, MarketType::Bond);
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
        for code in EXCHANGE_CODES {
            assert!(
                calendar_for_exchange(code).is_some(),
                "MIC {} did not resolve",
                code
            );
        }
    }

    #[test]
    fn otc_inherits_nyse_holidays() {
        let cal = calendar_for_exchange("PINX").unwrap();
        assert_eq!(cal.market_type, MarketType::Equity);
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
    fn xams_kingsday_2024() {
        // Apr 27 2024 falls on a Saturday; King's Day skipped (no roll).
        // Test 2023 instead: Apr 27 2023 = Thursday, holiday.
        let cal = calendar_for_exchange("XAMS").unwrap();
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2023, 4, 27).unwrap()));
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
    }

    #[test]
    fn xnze_waitangi_2024() {
        let cal = calendar_for_exchange("XNZE").unwrap();
        // Feb 6 2024 = Tuesday.
        assert!(cal.is_holiday(NaiveDate::from_ymd_opt(2024, 2, 6).unwrap()));
    }
}
