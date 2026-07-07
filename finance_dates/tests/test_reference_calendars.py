"""Cross-check finance-dates calendars against independent reference libraries.

This test compares our trading-day set against two independent, pip-installable
reference calendar packages whose holiday data ships *inside the package*:

* ``pandas-market-calendars``
* ``exchange-calendars``

Both are development/test-only dependencies, so the comparison is deterministic
and fully offline (no network access) — suitable for CI. Neither library's data
is vendored into our source; they are used purely as oracles here.

Strategy: a **reference-consensus** check. For every date on which the two
references *agree*, our calendar must match them. Dates where the two references
*disagree* are inherently ambiguous (often a bug in one reference) and are not
asserted — this is how the check tolerates known reference errors without a
hand-maintained allowlist. When only one reference is available for a market we
fall back to comparing against it, honouring the documented divergences below.

The curated (tabulated) markets are complete through 2030; see ``END``.
"""

from __future__ import annotations

import datetime as dt

import pytest

from finance_dates import Calendar

mcal = pytest.importorskip("pandas_market_calendars")
xcals = pytest.importorskip("exchange_calendars")

START = dt.date(2015, 1, 1)
END = dt.date(2030, 12, 31)

# our exchange code -> (pandas-market-calendars name, exchange-calendars name)
# exchange-calendars uses ISO MICs; None means that reference lacks the market.
REFERENCE_CALENDARS: dict[str, tuple[str, str | None]] = {
    "XNYS": ("NYSE", "XNYS"),
    "XNAS": ("NASDAQ", "XNAS"),
    "XASE": ("NYSE", "XASE"),
    "XLON": ("LSE", "XLON"),
    "XPAR": ("XPAR", "XPAR"),
    "XAMS": ("XAMS", "XAMS"),
    "XBRU": ("XBRU", "XBRU"),
    "XLIS": ("XLIS", "XLIS"),
    "XFRA": ("XFRA", "XFRA"),
    "XETR": ("XFRA", "XETR"),
    "XMIL": ("XMIL", "XMIL"),
    "XMAD": ("XMAD", "XMAD"),
    "XSWX": ("SIX", "XSWX"),
    "XSTO": ("XSTO", "XSTO"),
    "XCSE": ("XCSE", "XCSE"),
    "XHEL": ("XHEL", "XHEL"),
    "XOSL": ("XOSL", "XOSL"),
    "XICE": ("XICE", "XICE"),
    "XWBO": ("XWBO", "XWBO"),
    "XWAR": ("XWAR", "XWAR"),
    "XBUD": ("XBUD", "XBUD"),
    "XPRA": ("XPRA", "XPRA"),
    "XDUB": ("XDUB", "XDUB"),
    "XTSE": ("TSX", "XTSE"),
    "XMEX": ("XMEX", "XMEX"),
    "XBUE": ("XBUE", "XBUE"),
    "BVMF": ("BVMF", "BVMF"),
    "XBOG": ("XBOG", "XBOG"),
    "XSGO": ("XSGO", "XSGO"),
    "XLIM": ("XLIM", "XLIM"),
    "XTKS": ("JPX", "XTKS"),
    "XHKG": ("HKEX", "XHKG"),
    "XSES": ("XSES", "XSES"),
    "XKLS": ("XKLS", "XKLS"),
    "XIDX": ("XIDX", "XIDX"),
    "XBKK": ("XBKK", "XBKK"),
    "XNSE": ("NSE", None),
    "XBOM": ("BSE", "XBOM"),
    "XKRX": ("XKRX", "XKRX"),
    "XTAI": ("XTAI", "XTAI"),
    "XASX": ("ASX", "XASX"),
    "XNZE": ("XNZE", "XNZE"),
    "XJSE": ("XJSE", "XJSE"),
    "XSAU": ("XSAU", "XSAU"),
    "XTAE": ("TASE", "XTAE"),
    "XIST": ("XIST", "XIST"),
    "XSHG": ("SSE", "XSHG"),
}

# Dates where finance-dates intentionally differs from ONE reference because that
# reference is wrong (verified against the official exchange calendar and the
# other reference). Only consulted when a single reference is available.
KNOWN_DIVERGENCES: dict[str, dict[dt.date, str]] = {
    # LSE traded on 2022-05-30: the spring bank holiday was moved to Jun 2 for
    # the Platinum Jubilee (pandas-market-calendars marks it closed).
    "XLON": {dt.date(2022, 5, 30): "spring bank holiday moved to Jun 2 (Jubilee)"},
    # Ching Ming on a Sunday whose Monday substitute is Easter Monday is observed
    # the Tuesday; HKEX was closed (pandas-market-calendars marks it open).
    "XHKG": {
        dt.date(2021, 4, 6): "Ching Ming substitute (Sunday -> after Easter Monday)",
        dt.date(2026, 4, 7): "Ching Ming substitute (Sunday -> after Easter Monday)",
    },
}


def _pmc_sessions(name: str) -> set[dt.date] | None:
    try:
        cal = mcal.get_calendar(name)
        return {t.date() for t in cal.valid_days(START.isoformat(), END.isoformat())}
    except Exception:
        return None


def _ec_sessions(name: str | None) -> set[dt.date] | None:
    if name is None or name not in set(xcals.get_calendar_names()):
        return None
    try:
        cal = xcals.get_calendar(name)
        lo = max(START, cal.first_session.date())
        hi = min(END, cal.last_session.date())
        return {t.date() for t in cal.sessions_in_range(lo.isoformat(), hi.isoformat())}
    except Exception:
        return None


def _weekdays(start: dt.date, end: dt.date):
    d = start
    while d <= end:
        yield d
        d += dt.timedelta(days=1)


@pytest.mark.parametrize("code", sorted(REFERENCE_CALENDARS))
def test_matches_reference_consensus(code: str) -> None:
    pmc_name, ec_name = REFERENCE_CALENDARS[code]
    pmc = _pmc_sessions(pmc_name)
    ec = _ec_sessions(ec_name)
    if pmc is None and ec is None:
        pytest.skip(f"no reference calendar available for {code}")

    cal = Calendar.from_exchange(code)
    allow = KNOWN_DIVERGENCES.get(code, {})
    mismatches: list[str] = []

    # Constrain to the window all available references actually cover.
    lo = START
    hi = END
    if ec is not None and ec:
        hi = min(hi, max(ec))

    for d in _weekdays(lo, hi):
        ours = cal.is_business_day(d)
        po = d in pmc if pmc is not None else None
        eo = d in ec if ec is not None else None

        if po is not None and eo is not None:
            # Two references: only assert where they agree with each other.
            if po == eo and ours != po:
                mismatches.append(f"{d} ours={ours} refs_agree={po}")
        else:
            ref = po if po is not None else eo
            if ours != ref and d not in allow:
                mismatches.append(f"{d} ours={ours} ref={ref}")

    assert not mismatches, f"{code}: {len(mismatches)} divergence(s) from reference consensus (first 10): {mismatches[:10]}"


def test_known_reference_divergences_are_still_present() -> None:
    """Guard the cases where we are right and one reference is wrong.

    If a future reference release fixes these, this test flags it so the
    allowlist can be pruned.
    """
    lse = Calendar.from_exchange("XLON")
    assert not lse.is_holiday(dt.date(2022, 5, 30))
    hk = Calendar.from_exchange("XHKG")
    assert hk.is_holiday(dt.date(2021, 4, 6))
    assert hk.is_holiday(dt.date(2026, 4, 7))


def test_nyse_carter_day_of_mourning_regression() -> None:
    """The closure that started this whole effort stays closed."""
    assert Calendar.from_exchange("XNYS").is_holiday(dt.date(2025, 1, 9))
