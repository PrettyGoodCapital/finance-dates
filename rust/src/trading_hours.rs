//! Trading hours expressed in an IANA timezone.

use chrono::{DateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// A regular trading session (no half-day handling yet).
#[derive(Clone, Debug)]
pub struct TradingHours {
    pub open: NaiveTime,
    pub close: NaiveTime,
    pub timezone: Tz,
}

impl TradingHours {
    pub fn new(open: NaiveTime, close: NaiveTime, timezone: Tz) -> Self {
        Self { open, close, timezone }
    }

    /// True iff `instant` falls within `[open, close)` on a business day in
    /// the calendar's local timezone. Business-day check is done by the
    /// caller.
    pub fn contains_local_time(&self, instant: DateTime<Utc>) -> bool {
        let local = instant.with_timezone(&self.timezone).time();
        local >= self.open && local < self.close
    }

    /// Local-time naive open/close on the given local-day.
    pub fn open_at(&self, year: i32, month: u32, day: u32) -> Option<DateTime<Utc>> {
        let nd = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        let ndt = nd.and_time(self.open);
        let local = self.timezone.from_local_datetime(&ndt).single()?;
        Some(local.with_timezone(&Utc))
    }

    pub fn close_at(&self, year: i32, month: u32, day: u32) -> Option<DateTime<Utc>> {
        let nd = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        let ndt = nd.and_time(self.close);
        let local = self.timezone.from_local_datetime(&ndt).single()?;
        Some(local.with_timezone(&Utc))
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
    use chrono_tz::America::New_York;

    #[test]
    fn nyse_contains_local() {
        let th = TradingHours::new(
            NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            New_York,
        );
        // 14:30 UTC on a winter day = 09:30 EST. Inclusive on open.
        let inst = New_York.with_ymd_and_hms(2024, 1, 8, 9, 30, 0).unwrap().with_timezone(&Utc);
        assert!(th.contains_local_time(inst));
        // One minute before open.
        let before = inst - Duration::minutes(1);
        assert!(!th.contains_local_time(before));
    }
}
