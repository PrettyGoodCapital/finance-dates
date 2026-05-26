# API

`finance-dates` exposes the following public Python symbols:

```python
from finance_dates import (
    COUNTRY_CODES,
    COUNTRY_CODES3,
    Calendar,
    EXCHANGE_CODES,
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
from finance_enums import EnergyType, ExchangeCode, UnderlyingAssetClass

nyse = Calendar.from_exchange("XNYS")
gas = Calendar.from_asset(ExchangeCode.XNYM, UnderlyingAssetClass.Commodity, subclass=EnergyType.NaturalGas)
ice_sugar = Calendar.from_product("ICE_US", "Sugar")

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
from finance_enums import AgricultureType, EnergyType, ExchangeCode, UnderlyingAssetClass

plain = Calendar.from_range(date(2024, 1, 1), date(2024, 1, 5))
nyse = Calendar.from_exchange("XNYS")
us = Calendar.from_region("US")
gas = Calendar.from_asset(ExchangeCode.XNYM, UnderlyingAssetClass.Commodity, subclass=EnergyType.NaturalGas)
corn = Calendar.from_asset(ExchangeCode.XCBT, UnderlyingAssetClass.Agriculture, subclass=AgricultureType.Corn)
ice_sugar = Calendar.from_product("ICE_US", "Sugar")
```

`from_range()` creates a plain date-series calendar. `from_exchange()`
and `from_region()` create exchange-aware calendars; `from_region()` accepts
ISO country codes from `COUNTRY_CODES` and `COUNTRY_CODES3`. `from_product()`
accepts an exchange code plus a finance-enums product/subtype label such as
`"NaturalGas"`, `"Corn"`, or `"Sugar"`. `from_asset()` accepts
finance-enums exchange-code and asset/subclass enum members, or their string
values; when no product-specific calendar is modeled, it falls back to the broad
exchange calendar for recognized finance-enums asset labels.

Useful attributes:

```python
nyse.name         # "XNYS"
nyse.market_type  # "Equities"
nyse.weekmask     # [True, True, True, True, True, False, False]
nyse.timezone     # "America/New_York"
nyse.regular_sessions  # local regular open/close templates
nyse.extended_hours    # named local extended-hours templates

tokyo = Calendar.from_exchange("XTKS")
tokyo.regular_sessions
# [(9, 0, 0, 11, 30, 0), (12, 30, 0, 15, 30, 0)]

grains = Calendar.from_exchange("CBOT_GRAINS")
grains.regular_sessions
# [(19, 0, -1, 7, 45, 0), (8, 30, 0, 13, 20, 0)]

energy = Calendar.from_exchange("CME_ENERGY")
energy.regular_sessions
# [(17, 0, -1, 16, 0, 0)]
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
`datetime` values for regular sessions. A business day with a lunch break or
other split schedule returns one pair per regular interval. `extended_sessions()`
returns `(name, open, close)` tuples for named extended-hours windows such as
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

### Inspect lunch-break sessions

```python
from datetime import date, datetime, timezone
from finance_dates import Calendar

tokyo = Calendar.from_exchange("XTKS")
tokyo.regular_sessions
# [(9, 0, 0, 11, 30, 0), (12, 30, 0, 15, 30, 0)]

tokyo.is_open(datetime(2026, 5, 25, 2, 45, tzinfo=timezone.utc))
# False, 11:45 local is the lunch gap.

tokyo.sessions(date(2026, 5, 25), date(2026, 5, 25))
# Two UTC open/close pairs, one for each regular session.
```

### Inspect date-effective sessions

```python
from datetime import date
from finance_dates import Calendar

tokyo = Calendar.from_exchange("XTKS")

# Before the 2024-11-05 close extension, Tokyo's afternoon session closed
# at 15:00 local. Current dates close at 15:30 local.
tokyo.sessions(date(2024, 11, 1), date(2024, 11, 1))
tokyo.sessions(date(2024, 11, 5), date(2024, 11, 5))
```

### Inspect commodity futures sessions

```python
from datetime import date, datetime, timezone
from finance_dates import Calendar
from finance_enums import AgricultureType, EnergyType, ExchangeCode, UnderlyingAssetClass

nymex = Calendar.from_asset(ExchangeCode.XNYM, UnderlyingAssetClass.Commodity, subclass=EnergyType.NaturalGas)
nymex.is_open(datetime(2024, 1, 8, 22, 30, tzinfo=timezone.utc))
# False, 16:30 Chicago time is the daily maintenance break.

grains = Calendar.from_asset(ExchangeCode.XCBT, UnderlyingAssetClass.Agriculture, subclass=AgricultureType.Corn)
grains.sessions(date(2024, 1, 8), date(2024, 1, 8))
# Evening and day-session UTC windows for the Jan 8 trade date.

for code in ["CBOT_OILSEEDS", "CBOT_WHEAT", "CBOT_CORN", "CBOT_SOYBEANS"]:
    Calendar.from_exchange(code).regular_sessions

for code in ["CME_LIVESTOCK", "CME_DAIRY", "CME_LUMBER"]:
    Calendar.from_exchange(code).regular_sessions

for code in ["LE", "CL", "GC", "ZC", "LBR"]:
    Calendar.from_exchange(code).regular_sessions
```

### Generate extended-hours windows

```python
from datetime import date
from finance_dates import Calendar

cal = Calendar.from_exchange("XNYS")
templates = cal.extended_hours
windows = cal.extended_sessions(date(2024, 1, 8), date(2024, 1, 8))
```

### Discover supported codes

```python
from finance_dates import COUNTRY_CODES, COUNTRY_CODES3, EXCHANGE_CODES

len(EXCHANGE_CODES)
"FOREX" in EXCHANGE_CODES
"CBOT_GRAINS" in EXCHANGE_CODES  # False; resolver-only alias
"BR" in COUNTRY_CODES
"BRA" in COUNTRY_CODES3
```

`EXCHANGE_CODES` contains the enum-backed exchange/MIC and generic identifiers
sourced from `finance-enums`. `Calendar.from_exchange()` accepts all of those
plus resolver-only calendar aliases such as `CBOT_GRAINS`, `CME_ENERGY`, `CL`,
and `ZC`.

______________________________________________________________________

## Versioning

The current Python package version is exposed at
`finance_dates.__version__`. The public Python API is the set of symbols
listed at the top of this page. The native module
`finance_dates.finance_dates` is an implementation detail.
