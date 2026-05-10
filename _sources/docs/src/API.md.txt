# API

`finance-dates` exposes the following public Python symbols:

```python
from finance_dates import (
    Calendar,
    EXCHANGE_CODES,
    REGION_CODES,
    date_range,
)
```

For concepts, calendar families, and trading-hours conventions, see the
[Calendars](CALENDARS.md) page.

______________________________________________________________________

## Quick start

```python
from datetime import date, datetime, timezone

from finance_dates import Calendar

nyse = Calendar.from_exchange("XNYS")

trading_days = nyse.business_days(date(2024, 7, 1), date(2024, 7, 5))
holidays = nyse.holidays(date(2024, 7, 1), date(2024, 9, 30))
sessions = nyse.sessions(date(2024, 7, 1), date(2024, 7, 5))
extended = nyse.extended_sessions(date(2024, 7, 1), date(2024, 7, 5))
is_open = nyse.is_open(datetime(2024, 7, 3, 16, 30, tzinfo=timezone.utc))
```

______________________________________________________________________

## Reference

```{eval-rst}
.. currentmodule:: finance_dates

.. autofunction:: date_range

.. autoclass:: Calendar
   :members:
   :undoc-members:
   :show-inheritance:
```

______________________________________________________________________

## Top-level helpers

### `date_range(start, end, *, step_days=1)`

Returns an inclusive list of `datetime.date` values. `step_days` must be
a positive integer.

```python
from datetime import date
from finance_dates import date_range

date_range(date(2024, 1, 1), date(2024, 1, 5), step_days=2)
```

Use `Calendar.from_range(...).business_days()` for generic Monday-Friday
business days, and `Calendar.from_exchange(...).business_days()` when
exchange holidays matter.

______________________________________________________________________

## `Calendar`

Construct calendars with class methods:

```python
from datetime import date
from finance_dates import Calendar

plain = Calendar.from_range(date(2024, 1, 1), date(2024, 1, 5))
nyse = Calendar.from_exchange("XNYS")
us = Calendar.from_region("US")
```

`from_range()` creates a plain date-series calendar. `from_exchange()`
and `from_region()` create exchange-aware calendars.

Useful attributes:

```python
nyse.name         # "XNYS"
nyse.market_type  # "equity"
nyse.weekmask     # [True, True, True, True, True, False, False]
nyse.timezone     # "America/New_York"
nyse.regular_sessions  # local regular open/close templates
nyse.extended_hours    # named local extended-hours templates
```

Date methods:

```python
plain.days()
plain.business_days()
nyse.is_business_day(date(2024, 7, 3))
nyse.is_holiday(date(2024, 7, 4))
nyse.next_business_day(date(2024, 7, 3))
nyse.previous_business_day(date(2024, 7, 5))
nyse.business_days_between(date(2024, 1, 1), date(2024, 12, 31))
nyse.business_days(date(2024, 7, 1), date(2024, 7, 5))
nyse.holidays(2024)
nyse.holidays(date(2024, 7, 1), date(2024, 9, 30))
```

Datetime/session methods:

```python
from datetime import datetime, timezone

instant = datetime(2024, 3, 11, 13, 30, tzinfo=timezone.utc)

nyse.is_open(instant)
nyse.next_open(instant)
nyse.next_close(instant)
nyse.sessions(date(2024, 7, 1), date(2024, 7, 5))
nyse.extended_sessions(date(2024, 7, 1), date(2024, 7, 5))
nyse.early_close_for(date(2024, 7, 3))
```

`sessions()` returns `(open, close)` pairs as timezone-aware UTC
`datetime` values for regular sessions. `extended_sessions()` returns
`(name, open, close)` tuples for named extended-hours windows such as
`pre_open` and `after_close`.

______________________________________________________________________

## Recipes

### Generate valid trading dates

```python
from datetime import date
from finance_dates import Calendar

cal = Calendar.from_exchange("XNAS")
valid = cal.business_days(date(2024, 1, 1), date(2024, 1, 31))
```

### Generate invalid holiday dates

```python
from datetime import date
from finance_dates import Calendar

cal = Calendar.from_exchange("XNYS")
holidays = cal.holidays(date(2024, 1, 1), date(2024, 12, 31))
```

### Generate UTC session windows

```python
from datetime import date
from finance_dates import Calendar

cal = Calendar.from_exchange("XCME")
windows = cal.sessions(date(2024, 1, 8), date(2024, 1, 12))
```

### Generate extended-hours windows

```python
from datetime import date
from finance_dates import Calendar

cal = Calendar.from_exchange("XNYS")
templates = cal.extended_hours
windows = cal.extended_sessions(date(2024, 1, 8), date(2024, 1, 8))
```

### Discover supported exchanges

```python
from finance_dates import EXCHANGE_CODES, REGION_CODES

len(EXCHANGE_CODES)
"FOREX" in EXCHANGE_CODES
"BR" in REGION_CODES
```

______________________________________________________________________

## Versioning

The current Python package version is exposed at
`finance_dates.__version__`. The public Python API is the set of symbols
listed at the top of this page. The native module
`finance_dates.finance_dates` is an implementation detail.
