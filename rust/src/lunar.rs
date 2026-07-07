//! Chinese lunisolar calendar computations.
//!
//! Implements the astronomical new-moon (Meeus, *Astronomical Algorithms*
//! ch. 49) and apparent solar-longitude (ch. 25) series, then builds the
//! Chinese calendar month structure (winter-solstice anchored, with leap months
//! placed on the first month lacking a major solar term). Dates are resolved in
//! China Standard Time (UTC+8), which governs the official calendar used by the
//! mainland, Hong Kong, Taiwan and Korean exchanges.
//!
//! Accuracy is well within a day for 1900-2100, sufficient to reproduce the
//! published exchange holiday dates.

use chrono::{Datelike, NaiveDate};

const PI: f64 = std::f64::consts::PI;
const SYNODIC_MONTH: f64 = 29.530588861;
const CST_OFFSET_DAYS: f64 = 8.0 / 24.0;

fn rad(deg: f64) -> f64 {
    deg * PI / 180.0
}

/// Convert a Julian Day (in the given day fraction) to a civil date.
fn jd_to_date(jd: f64) -> NaiveDate {
    // Meeus ch. 7, reverse of the Julian Day formula.
    let z = (jd + 0.5).floor();
    let f = jd + 0.5 - z;
    let a = if z < 2299161.0 {
        z
    } else {
        let alpha = ((z - 1867216.25) / 36524.25).floor();
        z + 1.0 + alpha - (alpha / 4.0).floor()
    };
    let b = a + 1524.0;
    let c = ((b - 122.1) / 365.25).floor();
    let d = (365.25 * c).floor();
    let e = ((b - d) / 30.6001).floor();
    let day = b - d - (30.6001 * e).floor() + f;
    let month = if e < 14.0 { e - 1.0 } else { e - 13.0 };
    let year = if month > 2.0 { c - 4716.0 } else { c - 4715.0 };
    NaiveDate::from_ymd_opt(year as i32, month as u32, day.floor() as u32).unwrap()
}

/// Approximate ΔT (dynamical minus universal time) in days, adequate 1900-2100.
fn delta_t_days(year: f64) -> f64 {
    let t = (year - 2000.0) / 100.0;
    // Polynomial fit (Espenak & Meeus) for 2005-2050 range, good enough here.
    let secs = 62.92 + 32.217 * t + 55.89 * t * t;
    secs / 86400.0
}

/// JDE of the `k`-th new moon after the epoch (Meeus ch. 49), in Dynamical Time.
fn new_moon_jde(k: f64) -> f64 {
    let t = k / 1236.85;
    let t2 = t * t;
    let t3 = t2 * t;
    let t4 = t3 * t;
    let mut jde = 2451550.09766 + SYNODIC_MONTH * k + 0.00015437 * t2 - 0.000000150 * t3
        + 0.00000000073 * t4;
    let e = 1.0 - 0.002516 * t - 0.0000074 * t2;
    let m = rad(2.5534 + 29.1053567 * k - 0.0000014 * t2 - 0.00000011 * t3);
    let mp = rad(201.5643 + 385.81693528 * k + 0.0107582 * t2 + 0.00001238 * t3
        - 0.000000058 * t4);
    let f = rad(160.7108 + 390.67050284 * k - 0.0016118 * t2 - 0.00000227 * t3
        + 0.000000011 * t4);
    let omega = rad(124.7746 - 1.56375588 * k + 0.0020672 * t2 + 0.00000215 * t3);
    let corr = -0.40720 * mp.sin()
        + 0.17241 * e * m.sin()
        + 0.01608 * (2.0 * mp).sin()
        + 0.01039 * (2.0 * f).sin()
        + 0.00739 * e * (mp - m).sin()
        - 0.00514 * e * (mp + m).sin()
        + 0.00208 * e * e * (2.0 * m).sin()
        - 0.00111 * (mp - 2.0 * f).sin()
        - 0.00057 * (mp + 2.0 * f).sin()
        + 0.00056 * e * (2.0 * mp + m).sin()
        - 0.00042 * (3.0 * mp).sin()
        + 0.00042 * e * (m + 2.0 * f).sin()
        + 0.00038 * e * (m - 2.0 * f).sin()
        - 0.00024 * e * (2.0 * mp - m).sin()
        - 0.00017 * omega.sin()
        - 0.00007 * (mp + 2.0 * m).sin()
        + 0.00004 * (2.0 * mp - 2.0 * f).sin()
        + 0.00004 * (3.0 * m).sin()
        + 0.00003 * (mp + m - 2.0 * f).sin()
        + 0.00003 * (2.0 * mp + 2.0 * f).sin()
        - 0.00003 * (mp + m + 2.0 * f).sin()
        + 0.00003 * (mp - m + 2.0 * f).sin()
        - 0.00002 * (mp - m - 2.0 * f).sin()
        - 0.00002 * (3.0 * mp + m).sin()
        + 0.00002 * (4.0 * mp).sin();
    jde += corr;
    // Additional planetary corrections (largest terms).
    let a = [
        (299.77 + 0.107408 * k - 0.009173 * t2, 0.000325),
        (251.88 + 0.016321 * k, 0.000165),
        (251.83 + 26.651886 * k, 0.000164),
        (349.42 + 36.412478 * k, 0.000126),
        (84.66 + 18.206239 * k, 0.000110),
        (141.74 + 53.303771 * k, 0.000062),
        (207.14 + 2.453732 * k, 0.000060),
        (154.84 + 7.30686 * k, 0.000056),
        (34.52 + 27.261239 * k, 0.000047),
        (207.19 + 0.121824 * k, 0.000042),
        (291.34 + 1.844379 * k, 0.000040),
        (161.72 + 24.198154 * k, 0.000037),
        (239.56 + 25.513099 * k, 0.000035),
        (331.55 + 3.592518 * k, 0.000023),
    ];
    for (angle, amp) in a {
        jde += amp * rad(angle).sin();
    }
    jde
}

/// Date (CST) of the new moon whose instant is nearest to `jd_guess`.
fn new_moon_date_near(jd_guess: f64) -> NaiveDate {
    let k = ((jd_guess - 2451550.09766) / SYNODIC_MONTH).round();
    let jde = new_moon_jde(k);
    let ut = jde - delta_t_days(2000.0 + (jde - 2451545.0) / 365.25);
    jd_to_date(ut + CST_OFFSET_DAYS)
}

/// Apparent solar longitude (degrees, 0-360) at Julian Day `jd` (UT).
fn solar_longitude(jd: f64) -> f64 {
    let t = (jd - 2451545.0) / 36525.0;
    let l0 = 280.46646 + 36000.76983 * t + 0.0003032 * t * t;
    let m = rad(357.52911 + 35999.05029 * t - 0.0001537 * t * t);
    let c = (1.914602 - 0.004817 * t - 0.000014 * t * t) * m.sin()
        + (0.019993 - 0.000101 * t) * (2.0 * m).sin()
        + 0.000289 * (3.0 * m).sin();
    let true_long = l0 + c;
    let omega = rad(125.04 - 1934.136 * t);
    let apparent = true_long - 0.00569 - 0.00478 * omega.sin();
    apparent.rem_euclid(360.0)
}

/// Julian Day (UT) at which the apparent solar longitude equals `angle` degrees,
/// searching near `jd_guess`.
fn solar_term_jd(jd_guess: f64, angle: f64) -> f64 {
    let mut jd = jd_guess;
    for _ in 0..10 {
        let diff = ((angle - solar_longitude(jd) + 180.0).rem_euclid(360.0)) - 180.0;
        jd += diff * 365.25 / 360.0;
    }
    jd
}

/// Gregorian date of the winter solstice (solar longitude 270°) in `year`.
fn winter_solstice(year: i32) -> NaiveDate {
    let guess = 2451545.0 + (year - 2000) as f64 * 365.25 + 355.0;
    let jd = solar_term_jd(guess, 270.0);
    jd_to_date(jd + CST_OFFSET_DAYS)
}

fn to_jd(date: NaiveDate) -> f64 {
    // Noon of the given date.
    let y = date.year();
    let m = date.month() as i32;
    let d = date.day() as f64;
    let (y, m) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let a = (y as f64 / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();
    (365.25 * (y as f64 + 4716.0)).floor() + (30.6001 * (m as f64 + 1.0)).floor() + d + b - 1524.5
        + 0.5
}

/// One Chinese calendar month: its first civil date, month number (1-12) and
/// whether it is a leap month.
#[derive(Clone, Copy, Debug)]
struct LunarMonth {
    start: NaiveDate,
    number: u32,
    leap: bool,
}

/// Build the sequence of Chinese months covering `year`, numbered from the
/// month-11 that contains the winter solstice of the previous year.
fn chinese_months(year: i32) -> Vec<LunarMonth> {
    let ws_prev = winter_solstice(year - 1);
    let ws_curr = winter_solstice(year);

    // Generate a run of consecutive new-moon start dates spanning both month-11s
    // (from ~2 months before the previous winter solstice).
    let k0 = ((to_jd(ws_prev) - 2451550.09766) / SYNODIC_MONTH).floor() - 2.0;
    let mut starts: Vec<NaiveDate> = Vec::new();
    let mut k = k0;
    loop {
        let jde = new_moon_jde(k);
        let ut = jde - delta_t_days(2000.0 + (jde - 2451545.0) / 365.25);
        starts.push(jd_to_date(ut + CST_OFFSET_DAYS));
        if *starts.last().unwrap() > ws_curr + chrono::Duration::days(40) {
            break;
        }
        k += 1.0;
    }

    // Month 11 = the month whose start is the latest new moon on or before each
    // winter solstice.
    let a = starts.iter().rposition(|s| *s <= ws_prev).unwrap();
    let b = starts.iter().rposition(|s| *s <= ws_curr).unwrap();
    // 13 months between the two month-11s means the sui contains a leap month.
    let has_leap = (b - a) == 13;

    let mut result = Vec::new();
    let mut number = 11u32;
    let mut last_number = 11u32;
    let mut leap_assigned = false;
    for i in a..b {
        let start = starts[i];
        let next = starts[i + 1];
        let is_leap =
            has_leap && !leap_assigned && i > a && !contains_zhongqi(start, next);
        if is_leap {
            leap_assigned = true;
            // A leap month repeats the preceding month's number.
            result.push(LunarMonth {
                start,
                number: last_number,
                leap: true,
            });
        } else {
            result.push(LunarMonth {
                start,
                number,
                leap: false,
            });
            last_number = number;
            number = if number == 12 { 1 } else { number + 1 };
        }
    }
    result
}

/// Whether the month [start, next) contains a major solar term (a zhongqi: solar
/// longitude at a multiple of 30°). Comparisons use China Standard Time civil
/// dates so the month boundary and the term fall on the same clock.
fn contains_zhongqi(start: NaiveDate, next: NaiveDate) -> bool {
    let s = to_jd(start);
    let l_start = solar_longitude(s);
    // The next zhongqi angle at or after the longitude at `start`.
    let angle = ((l_start / 30.0).ceil() * 30.0).rem_euclid(360.0);
    let jd_term = solar_term_jd(s + 5.0, angle);
    let term_date = jd_to_date(jd_term + CST_OFFSET_DAYS);
    term_date >= start && term_date < next
}

/// Gregorian date of the `day`-th day of lunar `month` (1-12) in `year`.
/// `leap` selects the leap month of that number when present.
pub fn lunar_to_gregorian(year: i32, month: u32, day: u32, leap: bool) -> Option<NaiveDate> {
    let months = chinese_months(year);
    let m = months
        .iter()
        .find(|m| m.number == month && m.leap == leap)?;
    Some(m.start + chrono::Duration::days(day as i64 - 1))
}

/// Gregorian date of Chinese New Year (lunar month 1, day 1) in `year`.
pub fn chinese_new_year(year: i32) -> Option<NaiveDate> {
    lunar_to_gregorian(year, 1, 1, false)
}

/// Qingming / Ching Ming festival (solar term at 15° solar longitude), in `year`.
pub fn qingming(year: i32) -> NaiveDate {
    let guess = 2451545.0 + (year - 2000) as f64 * 365.25 + 94.0;
    let jd = solar_term_jd(guess, 15.0);
    jd_to_date(jd + CST_OFFSET_DAYS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_chinese_new_years() {
        let expect = [
            (2020, 1, 25),
            (2021, 2, 12),
            (2022, 2, 1),
            (2023, 1, 22),
            (2024, 2, 10),
            (2025, 1, 29),
            (2026, 2, 17),
        ];
        for (y, m, d) in expect {
            assert_eq!(
                chinese_new_year(y).unwrap(),
                NaiveDate::from_ymd_opt(y, m, d).unwrap(),
                "CNY {y}"
            );
        }
    }

    #[test]
    fn known_qingming() {
        assert_eq!(qingming(2024), NaiveDate::from_ymd_opt(2024, 4, 4).unwrap());
        assert_eq!(qingming(2025), NaiveDate::from_ymd_opt(2025, 4, 4).unwrap());
        assert_eq!(qingming(2023), NaiveDate::from_ymd_opt(2023, 4, 5).unwrap());
    }

    #[test]
    fn mid_autumn_dragon_boat() {
        // Dragon Boat = lunar 5/5, Mid-Autumn = lunar 8/15.
        assert_eq!(
            lunar_to_gregorian(2024, 5, 5, false).unwrap(),
            NaiveDate::from_ymd_opt(2024, 6, 10).unwrap()
        );
        assert_eq!(
            lunar_to_gregorian(2024, 8, 15, false).unwrap(),
            NaiveDate::from_ymd_opt(2024, 9, 17).unwrap()
        );
    }

    #[test]
    fn leap_month_years() {
        // Years with a leap month exercise the month-numbering logic:
        // 2020 (leap 4), 2023 (leap 2), 2025 (leap 6).
        assert_eq!(
            lunar_to_gregorian(2020, 5, 5, false).unwrap(),
            NaiveDate::from_ymd_opt(2020, 6, 25).unwrap()
        );
        assert_eq!(
            lunar_to_gregorian(2020, 8, 15, false).unwrap(),
            NaiveDate::from_ymd_opt(2020, 10, 1).unwrap()
        );
        assert_eq!(
            lunar_to_gregorian(2023, 8, 15, false).unwrap(),
            NaiveDate::from_ymd_opt(2023, 9, 29).unwrap()
        );
        assert_eq!(
            lunar_to_gregorian(2025, 8, 15, false).unwrap(),
            NaiveDate::from_ymd_opt(2025, 10, 6).unwrap()
        );
    }
}
