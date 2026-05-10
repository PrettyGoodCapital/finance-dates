"""Fast date-range generation, holiday calendars, and trading hours.

The Rust core handles holiday-rule expansion, weekend-roll observance, and
DST-aware trading hours. This Python module is a thin re-export layer.
"""

from __future__ import annotations

try:  # native extension is unavailable in some doc builds
    from .finance_dates import (
        EXCHANGE_CODES,
        REGION_CODES,
        Calendar,
        business_day_range,
        date_range,
    )
except ImportError:  # pragma: no cover - sphinx fallback
    Calendar = None  # type: ignore[assignment]
    EXCHANGE_CODES = ()  # type: ignore[assignment]
    REGION_CODES = ()  # type: ignore[assignment]

    def business_day_range(*_args, **_kwargs):  # type: ignore[no-redef]
        raise ImportError("finance_dates native extension is not available")

    def date_range(*_args, **_kwargs):  # type: ignore[no-redef]
        raise ImportError("finance_dates native extension is not available")


__version__ = "0.1.0"

__all__ = [
    "Calendar",
    "EXCHANGE_CODES",
    "REGION_CODES",
    "business_day_range",
    "date_range",
]
