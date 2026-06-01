"""Fast date-range generation, holiday calendars, and trading hours.

The Rust core handles holiday-rule expansion, weekend-roll observance,
early closes, and DST-aware regular and extended trading hours. This
Python module is a thin re-export layer.
"""

from __future__ import annotations

from .periods import period_grid

try:  # native extension is unavailable in some doc builds
    from .finance_dates import (
        COUNTRY_CODES,
        COUNTRY_CODES3,
        EXCHANGE_CODES,
        Calendar,
        date_range,
    )
except ImportError:  # pragma: no cover - sphinx fallback
    Calendar = None  # type: ignore[assignment]
    COUNTRY_CODES = ()  # type: ignore[assignment]
    COUNTRY_CODES3 = ()  # type: ignore[assignment]
    EXCHANGE_CODES = ()  # type: ignore[assignment]

    def date_range(*_args, **_kwargs):  # type: ignore[no-redef]
        raise ImportError("finance_dates native extension is not available")


__version__ = "0.3.0"

try:  # optional runtime dependency for expression namespace registration
    from . import _namespace as _namespace  # noqa: F401
except ImportError:  # pragma: no cover - polars is optional outside expression use
    pass

__all__ = [
    "Calendar",
    "COUNTRY_CODES",
    "COUNTRY_CODES3",
    "EXCHANGE_CODES",
    "date_range",
    "period_grid",
]
