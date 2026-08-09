from __future__ import annotations

from datetime import date

import polars as pl
import pytest
from finance_enums import Frequency

import finance_dates as fd


def test_period_grid_accepts_frequency_alias_and_expr() -> None:
    df = pl.DataFrame(
        {
            "date": [date(2024, 1, 2), date(2024, 1, 31), date(2024, 2, 1)],
            "bucket": ["a", "a", "b"],
        }
    )

    out = df.select(
        fd.period_grid(pl.col("date"), Frequency.Month).alias("enum_month"),
        fd.period_grid(pl.col("date"), "monthly").alias("alias_month"),
        fd.period_grid(pl.col("date"), pl.col("bucket")).alias("expr_bucket"),
    )

    assert out["enum_month"].to_list() == [date(2024, 1, 1), date(2024, 1, 1), date(2024, 2, 1)]
    assert out["alias_month"].to_list() == out["enum_month"].to_list()
    assert out["expr_bucket"].to_list() == ["a", "a", "b"]


def test_period_grid_rejects_empty_period_string() -> None:
    with pytest.raises(ValueError, match="must not be empty"):
        pl.select(fd.period_grid(pl.lit(date(2024, 1, 1)), "   "))
