#!/usr/bin/env python3
"""Report future-year drift of finance-dates calendars vs reference libraries.

The in-repo test (``finance_dates/tests/test_reference_calendars.py``) validates
2015-2030, the window our curated (tabulated) holiday tables cover. This script
runs the same reference-consensus comparison over a *future* window (2031+) so we
learn when the reference calendars gain a year we have not yet curated, or when a
computable calendar drifts.

It is intended for a scheduled (non-blocking) CI job that opens/updates an issue,
not for the PR test suite. It prints a Markdown report and, when run under GitHub
Actions, writes ``drift=true|false`` to ``$GITHUB_OUTPUT`` and the report body to
``drift_report.md``.

Usage:
    python scripts/check_calendar_drift.py [--start-year 2031] [--end-year 2040]
"""

from __future__ import annotations

import argparse
import datetime as dt
import os

import exchange_calendars as xcals
import pandas_market_calendars as mcal

from finance_dates import Calendar

# our exchange code -> (pandas-market-calendars name, exchange-calendars name)
REFERENCE_CALENDARS: dict[str, tuple[str, str | None]] = {
    "XNYS": ("NYSE", "XNYS"),
    "XNAS": ("NASDAQ", "XNAS"),
    "XLON": ("LSE", "XLON"),
    "XPAR": ("XPAR", "XPAR"),
    "XAMS": ("XAMS", "XAMS"),
    "XBRU": ("XBRU", "XBRU"),
    "XLIS": ("XLIS", "XLIS"),
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


def _pmc_sessions(name: str, start: dt.date, end: dt.date) -> set[dt.date] | None:
    try:
        cal = mcal.get_calendar(name)
        return {t.date() for t in cal.valid_days(start.isoformat(), end.isoformat())}
    except Exception:
        return None


def _ec_sessions(name: str | None, start: dt.date, end: dt.date) -> set[dt.date] | None:
    if name is None or name not in set(xcals.get_calendar_names()):
        return None
    try:
        cal = xcals.get_calendar(name, start=start.isoformat(), end=end.isoformat())
        return {t.date() for t in cal.sessions_in_range(start.isoformat(), end.isoformat())}
    except Exception:
        return None


def _weekdays(start: dt.date, end: dt.date):
    d = start
    while d <= end:
        yield d
        d += dt.timedelta(days=1)


def future_divergences(code: str, start: dt.date, end: dt.date) -> tuple[list[dt.date], int]:
    """Dates where our calendar differs from pandas-market-calendars over the
    future window, plus how many are corroborated by exchange-calendars.

    pandas-market-calendars projects future lunar/Islamic/Hebrew dates and rules
    to ~2045, so it is the forward oracle. exchange-calendars usually cannot be
    projected past its recorded bound (especially for tabulated markets), so it
    only corroborates where available.
    """
    pmc_name, ec_name = REFERENCE_CALENDARS[code]
    pmc = _pmc_sessions(pmc_name, start, end)
    if pmc is None or not pmc:
        return [], 0
    hi = min(end, max(pmc))
    ec = _ec_sessions(ec_name, start, hi)
    cal = Calendar.from_exchange(code)
    diffs, corroborated = [], 0
    for d in _weekdays(start, hi):
        po = d in pmc
        if cal.is_business_day(d) != po:
            diffs.append(d)
            if ec is not None and d <= max(ec) and (d in ec) == po:
                corroborated += 1
    return diffs, corroborated


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--start-year", type=int, default=2031)
    ap.add_argument("--end-year", type=int, default=2040)
    args = ap.parse_args()
    start = dt.date(args.start_year, 1, 1)
    end = dt.date(args.end_year, 12, 31)

    findings: dict[str, tuple[list[dt.date], int]] = {}
    for code in sorted(REFERENCE_CALENDARS):
        diffs, corrob = future_divergences(code, start, end)
        if diffs:
            findings[code] = (diffs, corrob)

    lines = [
        f"# Calendar reference drift {args.start_year}-{args.end_year}",
        "",
        "Dates where `finance-dates` differs from `pandas-market-calendars` "
        "(which projects future dates/rules) over the future window — typically a "
        "curated holiday table that needs extending past 2030, or a "
        "computable-calendar regression. `exchange-calendars` corroborates where "
        "it can be projected. These are review signals, not ground truth: the "
        "references' own future projections predate the relevant government "
        "decrees.",
        "",
    ]
    if not findings:
        lines.append("No drift: all markets match the reference over the window. ✅")
    else:
        lines.append("| Exchange | Diffs | Corroborated | First year | Sample dates |")
        lines.append("|----------|------:|-------------:|-----------:|--------------|")
        for code, (diffs, corrob) in findings.items():
            sample = ", ".join(d.isoformat() for d in diffs[:5])
            lines.append(f"| {code} | {len(diffs)} | {corrob} | {diffs[0].year} | {sample} |")
    report = "\n".join(lines)
    print(report)

    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as fh:
            fh.write(f"drift={'true' if findings else 'false'}\n")
        with open("drift_report.md", "w") as fh:
            fh.write(report + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
