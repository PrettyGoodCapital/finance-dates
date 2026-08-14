import datetime as dt
import importlib.util
from pathlib import Path

_SCRIPT = Path(__file__).parents[2] / "scripts" / "check_calendar_drift.py"
_SPEC = importlib.util.spec_from_file_location("check_calendar_drift", _SCRIPT)
assert _SPEC is not None and _SPEC.loader is not None
drift = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(drift)


class ClosedCalendar:
    @classmethod
    def from_exchange(cls, code: str):
        return cls()

    def is_business_day(self, date: dt.date) -> bool:
        return False


def test_future_divergences_ignore_reference_disagreement(monkeypatch) -> None:
    monday = dt.date(2031, 1, 6)
    tuesday = dt.date(2031, 1, 7)
    monkeypatch.setattr(drift, "Calendar", ClosedCalendar)
    monkeypatch.setattr(drift, "_pmc_sessions", lambda *_: {monday})
    monkeypatch.setattr(drift, "_ec_sessions", lambda *_: {tuesday})

    divergences = drift.future_divergences("XBUD", monday, tuesday)

    assert divergences == []


def test_future_divergences_report_reference_consensus(monkeypatch) -> None:
    monday = dt.date(2031, 1, 6)
    tuesday = dt.date(2031, 1, 7)
    monkeypatch.setattr(drift, "Calendar", ClosedCalendar)
    monkeypatch.setattr(drift, "_pmc_sessions", lambda *_: {monday})
    monkeypatch.setattr(drift, "_ec_sessions", lambda *_: {monday, tuesday})

    divergences = drift.future_divergences("XBUD", monday, tuesday)

    assert divergences == [monday]
