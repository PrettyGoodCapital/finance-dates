//! Trading hours expressed in an IANA timezone with multi-session support.
//!
//! A trading day can have one or more `Session`s. Each session has an open
//! time and a close time, both expressed as local clock times, plus optional
//! day offsets that allow sessions to span midnight (e.g. CME Globex equity
//! futures open 17:00 the previous calendar day and close 16:00 the same
//! "trading day", and ICE energy futures open 18:00 the previous day and
//! close 17:00 the same trading day).
//!
//! The "trading day" is the close-side calendar day. All session times are
//! anchored to that day; offsets shift the open or close.

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// One contiguous trading session anchored to a trading day.
///
/// `open_day_offset` and `close_day_offset` are calendar-day offsets relative
/// to the trading day (the close-side day). Typical values:
/// - Regular cash equities: open=0, close=0
/// - CME Globex equity futures: open=-1, close=0
/// - 24x5 FX: open=-1, close=0
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Session {
    pub open: NaiveTime,
    pub open_day_offset: i32,
    pub close: NaiveTime,
    pub close_day_offset: i32,
}

impl Session {
    pub const fn regular(open: NaiveTime, close: NaiveTime) -> Self {
        Self { open, open_day_offset: 0, close, close_day_offset: 0 }
    }

    /// A session whose open is on the previous calendar day, close on the
    /// trading day itself (the common futures pattern).
    pub const fn overnight(open: NaiveTime, close: NaiveTime) -> Self {
        Self { open, open_day_offset: -1, close, close_day_offset: 0 }
    }

    /// Convert this session into a UTC `(open, close)` pair given a trading
    /// day in the calendar's local timezone.
    pub fn instants(
        &self,
        tz: Tz,
        trading_day: NaiveDate,
    ) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let open_local = trading_day + Duration::days(self.open_day_offset as i64);
        let close_local = trading_day + Duration::days(self.close_day_offset as i64);
        let open = tz.from_local_datetime(&open_local.and_time(self.open)).single()?;
        let close = tz.from_local_datetime(&close_local.and_time(self.close)).single()?;
        Some((open.with_timezone(&Utc), close.with_timezone(&Utc)))
    }
}

/// One or more sessions per trading day, all in the same local timezone.
#[derive(Clone, Debug)]
pub struct TradingHours {
    pub sessions: Vec<Session>,
    pub timezone: Tz,
}

impl TradingHours {
    /// Single regular session (open and close on the trading day).
    pub fn new(open: NaiveTime, close: NaiveTime, timezone: Tz) -> Self {
        Self { sessions: vec![Session::regular(open, close)], timezone }
    }

    pub fn from_sessions(sessions: Vec<Session>, timezone: Tz) -> Self {
        Self { sessions, timezone }
    }

    /// Convenience: 24-hour-a-day, 5-day-a-week (open prev 17:00, close 17:00 NY).
    pub fn forex_24x5() -> Self {
        Self::from_sessions(
            vec![Session::overnight(
                NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            )],
            chrono_tz::America::New_York,
        )
    }

    /// 24x7 always-open marker (UTC, single full-day session).
    pub fn crypto_24x7() -> Self {
        Self::from_sessions(
            vec![Session {
                open: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                open_day_offset: 0,
                close: NaiveTime::from_hms_opt(23, 59, 59).unwrap(),
                close_day_offset: 0,
            }],
            chrono_tz::UTC,
        )
    }

    /// True iff `instant` falls within at least one session. The caller must
    /// ensure the relevant trading day is itself a business day; this method
    /// scans a 3-day window so cross-midnight sessions still resolve.
    pub fn contains_local_time(&self, instant: DateTime<Utc>) -> bool {
        let local_today = instant.with_timezone(&self.timezone).date_naive();
        for delta in [-1i64, 0, 1] {
            let day = local_today + Duration::days(delta);
            for s in &self.sessions {
                if let Some((o, c)) = s.instants(self.timezone, day) {
                    if instant >= o && instant < c {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// First session open instant for the given trading day.
    pub fn open_at(&self, year: i32, month: u32, day: u32) -> Option<DateTime<Utc>> {
        let nd = NaiveDate::from_ymd_opt(year, month, day)?;
        self.sessions
            .first()
            .and_then(|s| s.instants(self.timezone, nd).map(|(o, _)| o))
    }

    /// Last session close instant for the given trading day.
    pub fn close_at(&self, year: i32, month: u32, day: u32) -> Option<DateTime<Utc>> {
        let nd = NaiveDate::from_ymd_opt(year, month, day)?;
        self.sessions
            .last()
            .and_then(|s| s.instants(self.timezone, nd).map(|(_, c)| c))
    }
}

/// Convenience: parse "HH:MM" into a NaiveTime.
pub fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M").ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono_tz::America::{Chicago, New_York};

    #[test]
    fn nyse_contains_local() {
        let th = TradingHours::new(
            NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            New_York,
        );
        let inst = New_York
            .with_ymd_and_hms(2024, 1, 8, 9, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(th.contains_local_time(inst));
        let before = inst - Duration::minutes(1);
        assert!(!th.contains_local_time(before));
    }

    #[test]
    fn cme_equity_futures_overnight_open() {
        let th = TradingHours::from_sessions(
            vec![Session::overnight(
                NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            )],
            Chicago,
        );
        // Sun Jan 7 2024 18:00 CT should be in session for Mon Jan 8 trading day.
        let inst = Chicago
            .with_ymd_and_hms(2024, 1, 7, 18, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(th.contains_local_time(inst));
        // Mon Jan 8 2024 16:30 CT — outside the 16:00 close.
        let inst2 = Chicago
            .with_ymd_and_hms(2024, 1, 8, 16, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(!th.contains_local_time(inst2));
    }

    #[test]
    fn forex_continuous_24x5() {
        let th = TradingHours::forex_24x5();
        // Tuesday 03:00 NY — should be open (between Mon 17:00 → Tue 17:00).
        let inst = New_York
            .with_ymd_and_hms(2024, 1, 9, 3, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(th.contains_local_time(inst));
        // Note: this method does not enforce the weekmask — the Calendar layer
        // does. See `Calendar::is_open` for the full Mon-Fri filter.
    }
}
