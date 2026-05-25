"""Smoke tests for finance-dates."""

from __future__ import annotations

from datetime import date, datetime, timezone

import pytest
from finance_enums import AgricultureType, EnergyType, ExchangeCode, MetalsType, UnderlyingAssetClass

import finance_dates
from finance_dates import (
    COUNTRY_CODES,
    COUNTRY_CODES3,
    EXCHANGE_CODES,
    Calendar,
    date_range,
)


def test_exchange_and_region_lists() -> None:
    assert "XNYS" in EXCHANGE_CODES
    assert "XLON" in EXCHANGE_CODES
    assert "US" in COUNTRY_CODES
    assert "GB" in COUNTRY_CODES
    assert "USA" in COUNTRY_CODES3
    assert "GBR" in COUNTRY_CODES3
    assert "EU" not in COUNTRY_CODES
    assert "UK" not in COUNTRY_CODES


def test_date_range_unit_step() -> None:
    out = date_range(date(2024, 1, 1), date(2024, 1, 5))
    assert out == [
        date(2024, 1, 1),
        date(2024, 1, 2),
        date(2024, 1, 3),
        date(2024, 1, 4),
        date(2024, 1, 5),
    ]


def test_top_level_business_day_range_helper_is_removed() -> None:
    assert not hasattr(finance_dates, "business_day_range")


def test_range_calendar_business_days_excludes_weekends() -> None:
    # Mon Jan 1 2024 (holiday-blind) through Sun Jan 7 2024.
    out = Calendar.from_range(date(2024, 1, 1), date(2024, 1, 7)).business_days()
    assert out == [
        date(2024, 1, 1),
        date(2024, 1, 2),
        date(2024, 1, 3),
        date(2024, 1, 4),
        date(2024, 1, 5),
    ]


def test_range_calendar_days_and_business_days() -> None:
    cal = Calendar.from_range(date(2024, 1, 1), date(2024, 1, 7))

    assert cal.days() == [
        date(2024, 1, 1),
        date(2024, 1, 2),
        date(2024, 1, 3),
        date(2024, 1, 4),
        date(2024, 1, 5),
        date(2024, 1, 6),
        date(2024, 1, 7),
    ]
    assert cal.business_days() == [
        date(2024, 1, 1),
        date(2024, 1, 2),
        date(2024, 1, 3),
        date(2024, 1, 4),
        date(2024, 1, 5),
    ]


def test_exchange_calendar_new_api_names() -> None:
    cal = Calendar.from_exchange("XNYS")

    assert Calendar.from_region("US").name == "XNYS"
    with pytest.raises(ValueError):
        Calendar.from_region("EU")
    with pytest.raises(ValueError):
        Calendar.from_region("UK")
    assert cal.business_days(date(2024, 7, 1), date(2024, 7, 5)) == [
        date(2024, 7, 1),
        date(2024, 7, 2),
        date(2024, 7, 3),
        date(2024, 7, 5),
    ]
    assert cal.holidays(date(2024, 7, 1), date(2024, 9, 30)) == [
        date(2024, 7, 4),
        date(2024, 9, 2),
    ]
    sessions = cal.sessions(date(2024, 7, 1), date(2024, 7, 5))
    assert len(sessions) == 4
    assert sessions[2][1] == datetime(2024, 7, 3, 17, 0, tzinfo=timezone.utc)


def test_calendar_compatibility_aliases_are_removed() -> None:
    assert not hasattr(Calendar, "for_exchange")
    assert not hasattr(Calendar, "for_region")

    cal = Calendar.from_exchange("XNYS")
    assert not hasattr(cal, "business_day_range")
    assert not hasattr(cal, "holidays_between")
    assert not hasattr(cal, "sessions_between")


def test_nyse_extended_hours_are_exposed() -> None:
    cal = Calendar.from_exchange("XNYS")

    assert cal.regular_sessions == [(9, 30, 0, 16, 0, 0)]
    assert cal.extended_hours == [
        ("pre_open", 4, 0, 0, 9, 30, 0),
        ("after_close", 16, 0, 0, 20, 0, 0),
    ]

    windows = cal.extended_sessions(date(2024, 1, 8), date(2024, 1, 8))
    assert windows == [
        (
            "pre_open",
            datetime(2024, 1, 8, 9, 0, tzinfo=timezone.utc),
            datetime(2024, 1, 8, 14, 30, tzinfo=timezone.utc),
        ),
        (
            "after_close",
            datetime(2024, 1, 8, 21, 0, tzinfo=timezone.utc),
            datetime(2024, 1, 9, 1, 0, tzinfo=timezone.utc),
        ),
    ]


def test_nyse_after_close_starts_at_early_close() -> None:
    cal = Calendar.from_exchange("XNYS")

    windows = cal.extended_sessions(date(2024, 7, 3), date(2024, 7, 3))
    assert windows[1] == (
        "after_close",
        datetime(2024, 7, 3, 17, 0, tzinfo=timezone.utc),
        datetime(2024, 7, 4, 0, 0, tzinfo=timezone.utc),
    )


def test_nyse_2024_has_252_trading_days() -> None:
    cal = Calendar.from_exchange("XNYS")
    assert cal.business_days_between(date(2024, 1, 1), date(2024, 12, 31)) == 252


def test_nyse_christmas_2022_observed_monday() -> None:
    cal = Calendar.from_exchange("XNYS")
    assert cal.is_holiday(date(2022, 12, 26))
    assert not cal.is_business_day(date(2022, 12, 26))


def test_nyse_juneteenth_first_year_2021() -> None:
    cal = Calendar.from_exchange("XNYS")
    # Not a holiday before 2021.
    assert not cal.is_holiday(date(2020, 6, 19))
    # 2021 Jun 19 was Saturday → observed Friday Jun 18.
    assert cal.is_holiday(date(2021, 6, 18))


def test_lse_easter_monday_2024() -> None:
    cal = Calendar.from_exchange("XLON")
    assert cal.is_holiday(date(2024, 4, 1))


def test_region_us_resolves_to_xnys() -> None:
    assert Calendar.from_region("US").name == "XNYS"


def test_next_and_previous_business_day_skip_holidays() -> None:
    cal = Calendar.from_exchange("XNYS")
    # Day after Christmas 2022 observed → Tue Dec 27.
    assert cal.next_business_day(date(2022, 12, 23)) == date(2022, 12, 27)
    assert cal.previous_business_day(date(2022, 12, 27)) == date(2022, 12, 23)


def test_business_days_method() -> None:
    cal = Calendar.from_exchange("XNYS")
    out = cal.business_days(date(2024, 7, 1), date(2024, 7, 8))
    assert date(2024, 7, 4) not in out  # Independence Day
    assert date(2024, 7, 6) not in out  # Saturday
    assert date(2024, 7, 8) in out


def test_is_open_at_market_open() -> None:
    cal = Calendar.from_exchange("XNYS")
    # 14:30 UTC == 09:30 EST on 2024-01-08 (winter, UTC-5).
    inst = datetime(2024, 1, 8, 14, 30, tzinfo=timezone.utc)
    assert cal.is_open(inst)
    inst_b = datetime(2024, 1, 8, 14, 27, tzinfo=timezone.utc)
    assert not cal.is_open(inst_b)


def test_is_open_handles_dst() -> None:
    cal = Calendar.from_exchange("XNYS")
    # 2024-03-11 is the Mon after DST start. 13:30 UTC == 09:30 EDT.
    inst = datetime(2024, 3, 11, 13, 30, tzinfo=timezone.utc)
    assert cal.is_open(inst)


def test_is_open_closed_on_weekend() -> None:
    cal = Calendar.from_exchange("XNYS")
    inst = datetime(2024, 1, 6, 15, 0, tzinfo=timezone.utc)  # Sat
    assert not cal.is_open(inst)


def test_unknown_exchange_raises() -> None:
    with pytest.raises(ValueError):
        Calendar.from_exchange("ZZZZ")


def test_holidays_year_returns_dates() -> None:
    cal = Calendar.from_exchange("XNYS")
    hs = cal.holidays(2024)
    assert all(isinstance(d, date) for d in hs)
    assert date(2024, 12, 25) in hs


def test_market_types_are_classified() -> None:
    assert Calendar.from_exchange("XNYS").market_type == "Equities"
    assert Calendar.from_exchange("21XX").market_type == "Equities"
    assert Calendar.from_exchange("OPRA").market_type == "Options"
    assert Calendar.from_exchange("XCME").market_type == "Futures"
    assert Calendar.from_exchange("XNYM").market_type == "Futures"
    assert Calendar.from_exchange("CFE").market_type == "Futures"
    assert Calendar.from_exchange("ICE_US").market_type == "Futures"
    assert Calendar.from_exchange("SIFMA_US").market_type == "FixedIncome"
    assert Calendar.from_exchange("FOREX").market_type == "ForeignExchange"
    assert Calendar.from_exchange("CRYPTO").market_type == "DigitalAssets"


def test_calendar_for_product() -> None:
    # NYMEX energy product gives energy-hours calendar
    cal = Calendar.from_product("XNYM", "NaturalGas")
    assert cal.market_type == "Futures"
    assert cal.name == "XNYM:NaturalGas"
    # NYMEX metals product also uses energy hours (COMEX)
    cal_gold = Calendar.from_product("XNYM", "Gold")
    assert cal_gold.market_type == "Futures"
    assert cal_gold.name == "XNYM:Gold"
    # NYMEX liquefied natural gas product
    cal_liquefied_natural_gas = Calendar.from_product("XNYM", "LiquefiedNaturalGas")
    assert cal_liquefied_natural_gas.market_type == "Futures"
    assert cal_liquefied_natural_gas.name == "XNYM:LiquefiedNaturalGas"
    # CBOT grains
    cal_corn = Calendar.from_product("XCBT", "Corn")
    assert cal_corn.market_type == "Futures"
    assert cal_corn.name == "XCBT:Corn"
    # CME livestock
    cal_cattle = Calendar.from_product("XCME", "Cattle")
    assert cal_cattle.market_type == "Futures"
    assert cal_cattle.name == "XCME:Cattle"
    # ICE US softs
    cal_sugar = Calendar.from_product("ICE_US", "Sugar")
    assert cal_sugar.market_type == "Futures"
    assert cal_sugar.name == "ICE_US:Sugar"
    # Fallback to exchange calendar when no product match
    cal_xnys = Calendar.from_product("XNYS", "Equities")
    assert cal_xnys.market_type == "Equities"
    assert cal_xnys.name == "XNYS"


def test_calendar_from_asset_accepts_finance_enum_members() -> None:
    cal_gas = Calendar.from_asset(
        ExchangeCode.XNYM,
        UnderlyingAssetClass.Commodity,
        subclass=EnergyType.NaturalGas,
    )
    assert cal_gas.market_type == "Futures"
    assert cal_gas.name == "XNYM:NaturalGas"

    cal_gold = Calendar.from_asset(ExchangeCode.XNYM, UnderlyingAssetClass.Metals, subclass=MetalsType.Gold)
    assert cal_gold.market_type == "Futures"
    assert cal_gold.name == "XNYM:Gold"

    cal_corn = Calendar.from_asset(ExchangeCode.XCBT, UnderlyingAssetClass.Agriculture, subclass=AgricultureType.Corn)
    assert cal_corn.market_type == "Futures"
    assert cal_corn.name == "XCBT:Corn"
    assert cal_corn.regular_sessions == [
        (19, 0, -1, 7, 45, 0),
        (8, 30, 0, 13, 20, 0),
    ]

    cal_equity = Calendar.from_asset(ExchangeCode.XNYS, UnderlyingAssetClass.Equity)
    assert cal_equity.market_type == "Equities"
    assert cal_equity.name == "XNYS"

    with pytest.raises(ValueError):
        Calendar.from_asset(ExchangeCode.XNYS, "NotAnAssetClass")


def test_cme_futures_open_sunday_evening_chicago() -> None:
    cal = Calendar.from_exchange("XCME")
    # Sunday Jan 7 2024 23:00 UTC = 17:00 CT — first instant of Mon's session.
    inst = datetime(2024, 1, 7, 23, 0, tzinfo=timezone.utc)
    assert cal.is_open(inst)


def test_nymex_energy_daily_maintenance_break_is_closed() -> None:
    cal = Calendar.from_exchange("XNYM")
    maintenance_break = datetime(2024, 1, 8, 22, 30, tzinfo=timezone.utc)  # 16:30 CT

    assert cal.is_open(maintenance_break) is False
    assert cal.next_open(maintenance_break) == datetime(2024, 1, 8, 23, 0, tzinfo=timezone.utc)


def test_cbot_grain_futures_expose_split_sessions() -> None:
    cal = Calendar.from_exchange("CBOT_GRAINS")

    assert cal.market_type == "Futures"
    assert cal.timezone == "America/Chicago"
    assert cal.regular_sessions == [
        (19, 0, -1, 7, 45, 0),
        (8, 30, 0, 13, 20, 0),
    ]

    morning_break = datetime(2024, 1, 8, 14, 0, tzinfo=timezone.utc)  # 08:00 CT
    assert cal.is_open(morning_break) is False
    assert cal.next_open(morning_break) == datetime(2024, 1, 8, 14, 30, tzinfo=timezone.utc)
    assert cal.next_close(morning_break) == datetime(2024, 1, 8, 19, 20, tzinfo=timezone.utc)


def test_globex_commodity_aliases_cover_more_than_cbot_grains() -> None:
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
    ]:
        cal = Calendar.from_exchange(code)
        assert cal.market_type == "Futures"
        assert cal.timezone == "America/Chicago"
        assert cal.regular_sessions == [
            (19, 0, -1, 7, 45, 0),
            (8, 30, 0, 13, 20, 0),
        ]

    for code in [
        "CME_ENERGY",
        "GLOBEX_ENERGY",
        "CME_METALS",
        "GLOBEX_METALS",
        "CME_DAIRY",
        "GLOBEX_DAIRY",
        "CL",
        "MCL",
        "QM",
        "GC",
        "MGC",
        "QO",
        "SR3",
        "ES",
        "NQ",
        "RTY",
    ]:
        cal = Calendar.from_exchange(code)
        assert cal.market_type == "Futures"
        assert cal.timezone == "America/Chicago"
        assert cal.regular_sessions == [(17, 0, -1, 16, 0, 0)]

    for code in [
        "CME_LIVESTOCK",
        "GLOBEX_LIVESTOCK",
        "LE",
        "GF",
        "HE",
    ]:
        cal = Calendar.from_exchange(code)
        assert cal.market_type == "Futures"
        assert cal.timezone == "America/Chicago"
        assert cal.regular_sessions == [(8, 30, 0, 13, 5, 0)]

    for code in [
        "CME_LUMBER",
        "GLOBEX_LUMBER",
        "LBR",
        "LS",
    ]:
        cal = Calendar.from_exchange(code)
        assert cal.market_type == "Futures"
        assert cal.timezone == "America/Chicago"
        assert cal.regular_sessions == [(9, 0, 0, 15, 5, 0)]


def test_forex_open_continuously_during_week() -> None:
    cal = Calendar.from_exchange("FOREX")
    # Tuesday 08:00 UTC = 03:00 NY — continuous FX session.
    inst = datetime(2024, 1, 9, 8, 0, tzinfo=timezone.utc)
    assert cal.is_open(inst)


def test_crypto_open_on_saturday() -> None:
    cal = Calendar.from_exchange("CRYPTO")
    inst = datetime(2024, 1, 13, 3, 0, tzinfo=timezone.utc)
    assert cal.is_open(inst)


def test_sifma_includes_columbus_and_veterans() -> None:
    cal = Calendar.from_exchange("SIFMA_US")
    assert cal.is_holiday(date(2024, 11, 11))  # Veterans Day
    assert cal.is_holiday(date(2024, 10, 14))  # Columbus Day


def test_all_exchange_codes_resolve() -> None:
    from finance_dates import EXCHANGE_CODES

    for code in EXCHANGE_CODES:
        cal = Calendar.from_exchange(code)
        assert cal.name.upper() == code.upper()


def test_calendar_exposes_sessions_and_timezone() -> None:
    cme = Calendar.from_exchange("XCME")
    sessions = cme.regular_sessions
    assert len(sessions) == 1
    open_hh, open_mm, open_off, close_hh, close_mm, close_off = sessions[0]
    assert (open_hh, open_mm, open_off) == (17, 0, -1)
    assert (close_hh, close_mm, close_off) == (16, 0, 0)
    assert cme.timezone == "America/Chicago"


def test_apac_lunch_break_sessions_are_exposed() -> None:
    cases = {
        "XTKS": (
            [(9, 0, 0, 11, 30, 0), (12, 30, 0, 15, 30, 0)],
            [
                (datetime(2026, 5, 25, 0, 0, tzinfo=timezone.utc), datetime(2026, 5, 25, 2, 30, tzinfo=timezone.utc)),
                (datetime(2026, 5, 25, 3, 30, tzinfo=timezone.utc), datetime(2026, 5, 25, 6, 30, tzinfo=timezone.utc)),
            ],
        ),
        "XHKG": (
            [(9, 30, 0, 12, 0, 0), (13, 0, 0, 16, 0, 0)],
            [
                (datetime(2026, 5, 25, 1, 30, tzinfo=timezone.utc), datetime(2026, 5, 25, 4, 0, tzinfo=timezone.utc)),
                (datetime(2026, 5, 25, 5, 0, tzinfo=timezone.utc), datetime(2026, 5, 25, 8, 0, tzinfo=timezone.utc)),
            ],
        ),
        "XSHG": (
            [(9, 30, 0, 11, 30, 0), (13, 0, 0, 15, 0, 0)],
            [
                (datetime(2026, 5, 25, 1, 30, tzinfo=timezone.utc), datetime(2026, 5, 25, 3, 30, tzinfo=timezone.utc)),
                (datetime(2026, 5, 25, 5, 0, tzinfo=timezone.utc), datetime(2026, 5, 25, 7, 0, tzinfo=timezone.utc)),
            ],
        ),
    }

    for code, (templates, windows) in cases.items():
        cal = Calendar.from_exchange(code)
        assert cal.regular_sessions == templates
        assert cal.sessions(date(2026, 5, 25), date(2026, 5, 25)) == windows


def test_tokyo_lunch_gap_is_closed_and_boundaries_advance() -> None:
    cal = Calendar.from_exchange("XTKS")
    lunch_gap = datetime(2026, 5, 25, 2, 45, tzinfo=timezone.utc)

    assert cal.is_open(lunch_gap) is False
    assert cal.next_open(lunch_gap) == datetime(2026, 5, 25, 3, 30, tzinfo=timezone.utc)
    assert cal.next_close(lunch_gap) == datetime(2026, 5, 25, 6, 30, tzinfo=timezone.utc)


def test_tokyo_uses_historical_close_before_2024_schedule_change() -> None:
    cal = Calendar.from_exchange("XTKS")

    before = cal.sessions(date(2024, 11, 1), date(2024, 11, 1))
    after = cal.sessions(date(2024, 11, 5), date(2024, 11, 5))

    assert before[1][1] == datetime(2024, 11, 1, 6, 0, tzinfo=timezone.utc)
    assert after[1][1] == datetime(2024, 11, 5, 6, 30, tzinfo=timezone.utc)
    assert cal.is_open(datetime(2024, 11, 1, 6, 15, tzinfo=timezone.utc)) is False
    assert cal.is_open(datetime(2024, 11, 5, 6, 15, tzinfo=timezone.utc)) is True


def test_next_boundaries_are_inclusive_at_exact_session_times() -> None:
    cal = Calendar.from_exchange("XTKS")
    exact_afternoon_open = datetime(2026, 5, 25, 3, 30, tzinfo=timezone.utc)
    exact_morning_close = datetime(2026, 5, 25, 2, 30, tzinfo=timezone.utc)

    assert cal.is_open(exact_afternoon_open) is True
    assert cal.next_open(exact_afternoon_open) == exact_afternoon_open
    assert cal.is_open(exact_morning_close) is False
    assert cal.next_close(exact_morning_close) == exact_morning_close


def test_nyse_july3_2024_early_close() -> None:
    cal = Calendar.from_exchange("XNYS")
    assert cal.early_close_for(date(2024, 7, 3)) == (13, 0)
    assert cal.early_close_for(date(2024, 7, 5)) is None


def test_nyse_black_friday_early_close() -> None:
    cal = Calendar.from_exchange("XNYS")
    # 2024 Black Friday = Nov 29.
    assert cal.early_close_for(date(2024, 11, 29)) == (13, 0)


def test_nyse_july3_closed_after_early_close() -> None:
    cal = Calendar.from_exchange("XNYS")
    # 14:00 ET on July 3 2024 — should be closed (early close at 13:00).
    aware = datetime(2024, 7, 3, 18, 0, tzinfo=timezone.utc)  # 14:00 ET (EDT = UTC-4)
    assert cal.is_open(aware) is False


def test_emea_and_latam_calendars_resolve() -> None:
    for code in ("XAMS", "XMIL", "XSWX", "XJSE", "XKRX", "XSAU", "BVMF", "XMEX"):
        cal = Calendar.from_exchange(code)
        assert cal.market_type == "Equities"
        assert cal.timezone != ""


def test_tase_uses_sun_thu_weekmask() -> None:
    cal = Calendar.from_exchange("XTAE")
    # Monday → Sunday; weekmask[0..6] = [Mon..Sun].
    # Fri = idx 4 → False, Sun = idx 6 → True.
    wm = cal.weekmask
    assert wm[4] is False
    assert wm[6] is True


def test_korean_seollal_2024_multi_day_holiday() -> None:
    cal = Calendar.from_exchange("XKRX")
    assert cal.is_holiday(date(2024, 2, 9)) is True
    assert cal.is_holiday(date(2024, 2, 12)) is True


def test_region_br_and_kr_resolve() -> None:
    assert Calendar.from_region("BR").name == "BVMF"
    assert Calendar.from_region("KR").name == "XKRX"


def test_business_day_series_method() -> None:
    cal = Calendar.from_exchange("XNYS")
    days = cal.business_days(date(2024, 7, 1), date(2024, 7, 5))
    # Jul 4 (Thu) is a holiday → 4 business days.
    assert days == [date(2024, 7, 1), date(2024, 7, 2), date(2024, 7, 3), date(2024, 7, 5)]


def test_holidays_q3_2024() -> None:
    cal = Calendar.from_exchange("XNYS")
    h = cal.holidays(date(2024, 7, 1), date(2024, 9, 30))
    assert date(2024, 7, 4) in h
    assert date(2024, 9, 2) in h
    assert len(h) == 2


def test_sessions_include_early_close() -> None:
    cal = Calendar.from_exchange("XNYS")
    sess = cal.sessions(date(2024, 7, 1), date(2024, 7, 5))
    # 4 business days × 1 session each.
    assert len(sess) == 4
    # Jul 3 close (third entry) is at 13:00 ET = 17:00 UTC during EDT.
    jul3_close = sess[2][1]
    assert jul3_close == datetime(2024, 7, 3, 17, 0, tzinfo=timezone.utc)
    # Jul 5 close is regular 16:00 ET = 20:00 UTC.
    jul5_close = sess[3][1]
    assert jul5_close == datetime(2024, 7, 5, 20, 0, tzinfo=timezone.utc)
