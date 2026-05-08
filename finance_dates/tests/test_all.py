"""Smoke tests for finance-dates."""

from __future__ import annotations

from datetime import date, datetime, timezone

import pytest

from finance_dates import (
    Calendar,
    EXCHANGE_CODES,
    REGION_CODES,
    business_day_range,
    date_range,
)


def test_exchange_and_region_lists() -> None:
    assert "XNYS" in EXCHANGE_CODES
    assert "XLON" in EXCHANGE_CODES
    assert "US" in REGION_CODES
    assert "JP" in REGION_CODES


def test_date_range_unit_step() -> None:
    out = date_range(date(2024, 1, 1), date(2024, 1, 5))
    assert out == [
        date(2024, 1, 1),
        date(2024, 1, 2),
        date(2024, 1, 3),
        date(2024, 1, 4),
        date(2024, 1, 5),
    ]


def test_business_day_range_excludes_weekends() -> None:
    # Mon Jan 1 2024 (holiday-blind) through Sun Jan 7 2024.
    out = business_day_range(date(2024, 1, 1), date(2024, 1, 7))
    assert out == [
        date(2024, 1, 1),
        date(2024, 1, 2),
        date(2024, 1, 3),
        date(2024, 1, 4),
        date(2024, 1, 5),
    ]


def test_nyse_2024_has_252_trading_days() -> None:
    cal = Calendar.for_exchange("XNYS")
    assert cal.business_days_between(date(2024, 1, 1), date(2024, 12, 31)) == 252


def test_nyse_christmas_2022_observed_monday() -> None:
    cal = Calendar.for_exchange("XNYS")
    assert cal.is_holiday(date(2022, 12, 26))
    assert not cal.is_business_day(date(2022, 12, 26))


def test_nyse_juneteenth_first_year_2021() -> None:
    cal = Calendar.for_exchange("XNYS")
    # Not a holiday before 2021.
    assert not cal.is_holiday(date(2020, 6, 19))
    # 2021 Jun 19 was Saturday → observed Friday Jun 18.
    assert cal.is_holiday(date(2021, 6, 18))


def test_lse_easter_monday_2024() -> None:
    cal = Calendar.for_exchange("XLON")
    assert cal.is_holiday(date(2024, 4, 1))


def test_region_us_resolves_to_xnys() -> None:
    assert Calendar.for_region("US").name == "XNYS"


def test_next_and_previous_business_day_skip_holidays() -> None:
    cal = Calendar.for_exchange("XNYS")
    # Day after Christmas 2022 observed → Tue Dec 27.
    assert cal.next_business_day(date(2022, 12, 23)) == date(2022, 12, 27)
    assert cal.previous_business_day(date(2022, 12, 27)) == date(2022, 12, 23)


def test_business_day_range_method() -> None:
    cal = Calendar.for_exchange("XNYS")
    out = cal.business_day_range(date(2024, 7, 1), date(2024, 7, 8))
    assert date(2024, 7, 4) not in out  # Independence Day
    assert date(2024, 7, 6) not in out  # Saturday
    assert date(2024, 7, 8) in out


def test_is_open_at_market_open() -> None:
    cal = Calendar.for_exchange("XNYS")
    # 14:30 UTC == 09:30 EST on 2024-01-08 (winter, UTC-5).
    inst = datetime(2024, 1, 8, 14, 30, tzinfo=timezone.utc)
    assert cal.is_open(inst)
    inst_b = datetime(2024, 1, 8, 14, 27, tzinfo=timezone.utc)
    assert not cal.is_open(inst_b)


def test_is_open_handles_dst() -> None:
    cal = Calendar.for_exchange("XNYS")
    # 2024-03-11 is the Mon after DST start. 13:30 UTC == 09:30 EDT.
    inst = datetime(2024, 3, 11, 13, 30, tzinfo=timezone.utc)
    assert cal.is_open(inst)


def test_is_open_closed_on_weekend() -> None:
    cal = Calendar.for_exchange("XNYS")
    inst = datetime(2024, 1, 6, 15, 0, tzinfo=timezone.utc)  # Sat
    assert not cal.is_open(inst)


def test_unknown_exchange_raises() -> None:
    with pytest.raises(ValueError):
        Calendar.for_exchange("ZZZZ")


def test_holidays_year_returns_dates() -> None:
    cal = Calendar.for_exchange("XNYS")
    hs = cal.holidays(2024)
    assert all(isinstance(d, date) for d in hs)
    assert date(2024, 12, 25) in hs
