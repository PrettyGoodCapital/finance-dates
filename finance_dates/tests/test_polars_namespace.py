from __future__ import annotations

from datetime import date

import polars as pl
import pytest

import finance_dates  # noqa: F401


def test_polars_fdates_namespace_business_day_helpers() -> None:
    df = pl.DataFrame(
        {
            "trade_date": [date(2024, 7, 3), date(2024, 7, 5)],
            "settle_date": [date(2024, 7, 8), date(2024, 7, 10)],
        }
    )

    out = df.with_columns(
        pl.col("trade_date").fdates.shift_business_days(1, exchange="XNYS").alias("next_trade"),
        pl.col("trade_date").fdates.align_to_business_day("following", exchange="XNYS").alias("aligned"),
        pl.col("trade_date").fdates.day_count_fraction(pl.col("settle_date"), convention="act_360").alias("dcf"),
    )

    assert out["next_trade"].to_list() == [date(2024, 7, 5), date(2024, 7, 8)]
    assert out["aligned"].to_list() == [date(2024, 7, 3), date(2024, 7, 5)]
    assert out["dcf"].to_list() == pytest.approx([5 / 360, 5 / 360])


def test_polars_fdates_namespace_modified_following() -> None:
    df = pl.DataFrame({"d": [date(2024, 8, 31)]})

    out = df.with_columns(pl.col("d").fdates.align_to_business_day("modified_following", exchange="XNYS").alias("rolled"))

    assert out["rolled"].to_list() == [date(2024, 8, 30)]
