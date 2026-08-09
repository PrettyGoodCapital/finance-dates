"""Period-grid helpers shared across finance packages."""

from __future__ import annotations

import polars as pl
from finance_enums import Frequency, to_frequency

__all__ = ["period_grid"]


def _period_rule(period: Frequency | str) -> str:
    if isinstance(period, Frequency):
        return period.polars_truncate
    value = period.strip()
    if not value:
        raise ValueError("period must not be empty")
    try:
        return to_frequency(value).polars_truncate
    except ValueError:
        return value


def period_grid(date: pl.Expr, period: Frequency | str | pl.Expr) -> pl.Expr:
    """Return a date-bucket expression for period-aware calculations.

    ``period`` accepts a ``finance_enums.Frequency`` value, any alias
    accepted by ``finance_enums.to_frequency()``, a Polars duration
    string accepted by ``dt.truncate()``, or a precomputed bucket
    expression.
    """
    if isinstance(period, pl.Expr):
        return period
    return date.dt.truncate(_period_rule(period))
