//! Fast date-range, holiday-calendar, and trading-hour utilities.

pub mod calendar;
pub mod holiday;
pub mod range;
pub mod trading_hours;

pub use calendar::{
    calendar_for_exchange, calendar_for_region, Calendar, MarketType, CRYPTO_WEEKMASK,
    EXCHANGE_CODES, FX_WEEKMASK, REGION_CODES,
};
pub use holiday::{HolidayRule, Weekday};
pub use range::{business_day_range, date_range, STANDARD_WEEKMASK};
pub use trading_hours::{Session, TradingHours};
