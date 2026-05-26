# finance dates

Fast date ranges, holiday calendars, and trading hours for financial markets

[![Build Status](https://github.com/prettygoodcapital/finance-dates/actions/workflows/build.yaml/badge.svg?branch=main&event=push)](https://github.com/prettygoodcapital/finance-dates/actions/workflows/build.yaml)
[![codecov](https://codecov.io/gh/prettygoodcapital/finance-dates/branch/main/graph/badge.svg)](https://codecov.io/gh/prettygoodcapital/finance-dates)
[![License](https://img.shields.io/github/license/prettygoodcapital/finance-dates)](https://github.com/prettygoodcapital/finance-dates)
[![PyPI](https://img.shields.io/pypi/v/finance-dates.svg)](https://pypi.python.org/pypi/finance-dates)

## Overview

`finance-dates` provides calendar-aware date utilities for the
`finance-*` stack. The Rust core handles holiday-rule expansion,
weekend observance, early closes, and DST-aware regular and extended
trading sessions; the Python package exposes a compact API for date
series, exchange calendars, and open/close timestamps.

The library is useful when you need to answer questions like:

- What are the valid trading dates for NYSE between two dates?
- Which dates in a range are holidays or otherwise invalid for a venue?
- Is a UTC timestamp inside a market session after DST and early closes?
- What are the UTC open/close datetimes for regular and extended
  trading windows?

### Quick start

```python
from datetime import date, datetime, timezone

from finance_dates import Calendar
from finance_enums import EnergyType, ExchangeCode, UnderlyingAssetClass

c = Calendar.from_range(start=date(2024, 1, 1), end=date(2024, 1, 5))

# Inclusive plain calendar dates.
c.days()

# Mon-Fri only, holiday-blind.
c.business_days()

# Exchange-aware calendar with holidays, sessions, and early closes.
nyse = Calendar.from_exchange("XNYS")
nyse.business_days(date(2024, 7, 1), date(2024, 7, 5))
nyse.holidays(date(2024, 7, 1), date(2024, 9, 30))
nyse.sessions(date(2024, 7, 1), date(2024, 7, 5))
nyse.extended_sessions(date(2024, 7, 1), date(2024, 7, 5))
nyse.is_open(datetime(2024, 3, 11, 13, 30, tzinfo=timezone.utc))

gas = Calendar.from_asset(
  ExchangeCode.XNYM,
  UnderlyingAssetClass.Commodity,
  subclass=EnergyType.NaturalGas,
)
gas.regular_sessions
```

For US equity calendars, `regular_sessions` contains the standard
09:30-16:00 New York session template, while `extended_hours` includes
`pre_open` and `after_close` templates. On early-close days, the
after-close window begins at the early close.

Calendars with lunch breaks expose multiple regular session templates. For
example, Tokyo (`XTKS`) currently returns separate 09:00-11:30 and
12:30-15:30 local sessions, and `sessions()` returns one UTC open/close pair
per regular interval. Tokyo is date-effective around the 2024-11-05 close-time
extension, so historical dates before that change close at 15:00 local while
current dates close at 15:30.

Commodity futures can also use split sessions. Prefer `Calendar.from_asset()`
with `finance-enums` exchange and asset/subclass enum members, or their string
values, when you know the instrument vocabulary; for example
`ExchangeCode.XNYM` plus `EnergyType.NaturalGas` resolves to the NYMEX energy
template without requiring synthetic names like `NYMEX_ENERGY`. Synthetic
product-group codes such as `CBOT_GRAINS` and product mnemonics such as `CL` or
`ZC` remain accepted by `from_exchange()` for lower-level calendar inspection
and compatibility.

### Exchange and country calendars

Calendars can be resolved by exchange/MIC code or by ISO country code:

```python
from finance_dates import Calendar
from finance_enums import EnergyType, ExchangeCode, UnderlyingAssetClass

Calendar.from_exchange("XLON")   # London Stock Exchange
Calendar.from_exchange("XTKS")   # Tokyo Stock Exchange, split lunch sessions
Calendar.from_exchange("XCME")   # CME futures-style overnight sessions
Calendar.from_exchange("CBOT_GRAINS")  # CBOT grain/oilseed futures sessions
Calendar.from_exchange("CME_ENERGY")  # Globex energy-category alias
Calendar.from_asset(ExchangeCode.XNYM, UnderlyingAssetClass.Commodity, subclass=EnergyType.NaturalGas)
Calendar.from_product("ICE_US", "Sugar")  # ICE US product-specific template
Calendar.from_exchange("FOREX")  # 24x5 FX family
Calendar.from_region("US")       # representative US equity calendar
```

`Calendar.from_exchange()` accepts additional resolver-only
calendar aliases such as `CBOT_GRAINS`, `CME_ENERGY`, `CL`, and `ZC`; those are
documented in the Calendars page.

### Documentation

See the [Calendars](docs/src/CALENDARS.md) page for supported concepts,
market families, date-series patterns, and trading-hours conventions.
See the [API](docs/src/API.md) page for the public Python API and
recipes.

### Rust crate

The Rust library crate is published as `finance-dates` and imported as
`finance_dates` in Rust code:

```toml
[dependencies]
finance-dates = "0.1.0"
```

```rust
use finance_dates::{calendar_for_exchange, date_range};
```

> [!NOTE]
> This library was generated using [copier](https://copier.readthedocs.io/en/stable/) from the [Base Python Project Template repository](https://github.com/python-project-templates/base).
