"""Polars ``.fdates`` namespace helpers for finance date operations."""

from __future__ import annotations

from datetime import date
from functools import lru_cache

import polars as pl

from .finance_dates import Calendar

__all__ = []


@lru_cache(maxsize=64)
def _calendar(exchange: str) -> Calendar:
    return Calendar.from_exchange(exchange)


def _shift_business_day_value(value: date | None, n: int, exchange: str) -> date | None:
    if value is None:
        return None
    cal = _calendar(exchange)
    out = value
    step = cal.next_business_day if n >= 0 else cal.previous_business_day
    for _ in range(abs(n)):
        out = step(out)
    return out


def _align_business_day_value(value: date | None, convention: str, exchange: str) -> date | None:
    if value is None:
        return None
    cal = _calendar(exchange)
    if cal.is_business_day(value):
        return value
    normalized = convention.strip().lower().replace("-", "_")
    if normalized == "following":
        return cal.next_business_day(value)
    if normalized == "preceding":
        return cal.previous_business_day(value)
    if normalized == "modified_following":
        following = cal.next_business_day(value)
        return following if following.month == value.month else cal.previous_business_day(value)
    if normalized == "modified_preceding":
        preceding = cal.previous_business_day(value)
        return preceding if preceding.month == value.month else cal.next_business_day(value)
    raise ValueError(f"unsupported business-day convention: {convention!r}")


def _day_count_fraction_value(start: date | None, end: date | None, convention: str) -> float | None:
    if start is None or end is None:
        return None
    normalized = convention.strip().lower().replace("-", "_")
    days = (end - start).days
    if normalized in {"act_360", "actual_360"}:
        return days / 360.0
    if normalized in {"act_365", "act_365f", "actual_365", "actual_365f"}:
        return days / 365.0
    if normalized in {"act_252", "actual_252"}:
        return days / 252.0
    if normalized in {"30_360", "thirty_360", "bond_30_360"}:
        d1 = min(start.day, 30)
        d2 = min(end.day, 30) if d1 == 30 else end.day
        return ((end.year - start.year) * 360 + (end.month - start.month) * 30 + (d2 - d1)) / 360.0
    raise ValueError(f"unsupported day-count convention: {convention!r}")


@pl.api.register_expr_namespace("fdates")
class ExprFinanceDates:
    def __init__(self, expr: pl.Expr) -> None:
        self._expr = expr

    def shift_business_days(self, n: int, *, exchange: str = "XNYS") -> pl.Expr:
        return self._expr.map_elements(lambda value: _shift_business_day_value(value, n, exchange), return_dtype=pl.Date)

    def align_to_business_day(self, convention: str = "following", *, exchange: str = "XNYS") -> pl.Expr:
        return self._expr.map_elements(lambda value: _align_business_day_value(value, convention, exchange), return_dtype=pl.Date)

    def day_count_fraction(self, end: pl.Expr, *, convention: str = "act_360") -> pl.Expr:
        return pl.struct([self._expr.alias("start"), end.alias("end")]).map_elements(
            lambda row: _day_count_fraction_value(row["start"], row["end"], convention),
            return_dtype=pl.Float64,
        )


@pl.api.register_series_namespace("fdates")
class SeriesFinanceDates:
    def __init__(self, series: pl.Series) -> None:
        self._series = series

    def shift_business_days(self, n: int, *, exchange: str = "XNYS") -> pl.Series:
        return self._series.map_elements(lambda value: _shift_business_day_value(value, n, exchange), return_dtype=pl.Date)

    def align_to_business_day(self, convention: str = "following", *, exchange: str = "XNYS") -> pl.Series:
        return self._series.map_elements(lambda value: _align_business_day_value(value, convention, exchange), return_dtype=pl.Date)

    def day_count_fraction(self, end: pl.Series, *, convention: str = "act_360") -> pl.Series:
        if len(self._series) != len(end):
            raise ValueError("start and end series must have the same length")
        values: list[float | None] = []
        for start_value, end_value in zip(self._series, end):
            values.append(_day_count_fraction_value(start_value, end_value, convention))
        return pl.Series(self._series.name, values, dtype=pl.Float64)
