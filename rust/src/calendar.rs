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
    // Synthetic / placeholder
    "XXXX", "PYPR", "SIMU",
    // Generic market families
    "FOREX", "CRYPTO", "SIFMA_US", "ICE_US", "CFE",
];

/// ISO region codes recognised by `calendar_for_region`.
pub const REGION_CODES: &[&str] = &[
    "US", "UK", "GB", "EU", "JP", "HK", "CN", "CA", "AU", "IN", "DE", "FR",
];

/// A holiday calendar with optional trading hours and a market classification.
pub struct Calendar {
    pub name: String,
    pub market_type: MarketType,
    pub weekmask: [bool; 7],
    pub rules: Vec<HolidayRule>,
    pub trading_hours: Option<TradingHours>,
    cache: HolidayCache,
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
            cache: HolidayCache::default(),
        }
    }

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
    /// Sun-evening CME open correctly maps to Mon's trading day.
    pub fn is_open(&self, when: DateTime<Utc>) -> bool {
        let Some(th) = &self.trading_hours else { return false };
        let local_today = when.with_timezone(&th.timezone).date_naive();
        for delta in [0i64, 1] {
            let trading_day = local_today + Duration::days(delta);
            if !self.is_business_day(trading_day) {
                continue;
            }
            for s in &th.sessions {
                if let Some((o, c)) = s.instants(th.timezone, trading_day) {
                    if when >= o && when < c {
                        return true;
                    }
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
            for s in &th.sessions {
                if let Some((_, c)) = s.instants(th.timezone, trading_day) {
                    if c >= when {
                        return Some(c);
                    }
                }
            }
        }
        None
    }
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
    Tsx,
    Asx,
    Nse,
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
        "XASX" => Asx,
        "XBOM" | "XNSE" => Nse,
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
        ),
        UsOptions => Calendar::with_type(
            name, MarketType::Options, STANDARD_WEEKMASK, nyse_rules(),
            Some(options_trading_hours()),
        ),
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
}
