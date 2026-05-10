# Calendars

`finance-dates` models three related ideas:

| Concept           | Purpose                                                          |
| ----------------- | ---------------------------------------------------------------- |
| Date ranges       | Inclusive `date` series with optional fixed day steps            |
| Business days     | Dates accepted by a weekmask and not present in a holiday set    |
| Trading sessions  | Local open/close times converted to timezone-aware UTC datetimes |
| Extended sessions | Named pre-open and after-close windows where available           |

The Python API is intentionally small. Use top-level helpers for plain
date series, then use `Calendar` when venue-specific holidays or trading
hours matter.

______________________________________________________________________

## Plain date series

`date_range(start, end, step_days=1)` returns every date in an
inclusive range.

```python
from datetime import date
from finance_dates import date_range

date_range(date(2024, 1, 1), date(2024, 1, 5))
```

`Calendar.from_range(...).business_days()` applies the standard
Monday-Friday weekmask and does not apply any holiday calendar. It is
useful for quick tests and generic weekday series.

```python
from datetime import date
from finance_dates import Calendar

Calendar.from_range(date(2024, 1, 1), date(2024, 1, 7)).business_days()
```

Use `Calendar.business_days()` when exchange holidays matter.

______________________________________________________________________

## Exchange calendars

Create calendars from exchange codes or representative region codes:

```python
from finance_dates import Calendar

nyse = Calendar.from_exchange("XNYS")
london = Calendar.from_exchange("XLON")
us = Calendar.from_region("US")
```

Calendar objects expose:

| Attribute / method                             | Meaning                                                                 |
| ---------------------------------------------- | ----------------------------------------------------------------------- |
| `name`                                         | Calendar code used by the resolver                                      |
| `market_type`                                  | One of `equity`, `options`, `futures`, `fx`, `bond`, `crypto`, `other`  |
| `weekmask`                                     | Seven booleans, indexed Monday through Sunday                           |
| `timezone`                                     | IANA timezone for trading hours, or `""` if no hours are configured     |
| `regular_sessions`                             | Local regular session templates as open/close hour/minute/offset tuples |
| `extended_hours`                               | Named local extended-hours templates where available                    |
| `is_business_day(date)`                        | True when the date passes weekmask and holiday checks                   |
| `is_holiday(date)`                             | True when the date is an observed holiday                               |
| `holidays(year)`                               | Observed holidays for a year                                            |
| `holidays(start, end)`                         | Holidays in an inclusive range                                          |
| `business_days_between(start, end)`            | Count of valid business days in an inclusive range                      |
| `business_days(start, end)`                    | Exchange-aware valid dates                                              |
| `sessions(start, end)`                         | UTC open/close datetimes for regular sessions                           |
| `extended_sessions(start, end)`                | Named UTC extended-hours windows                                        |
| `is_open(datetime)`                            | True when a UTC-aware timestamp is inside a regular session             |
| `next_open(datetime)` / `next_close(datetime)` | Next regular-session boundary if configured                             |
| `early_close_for(date)`                        | Local `(hour, minute)` early close or `None`                            |

Unknown exchange or region codes raise `ValueError` in Python.

______________________________________________________________________

## Market families

The resolver groups related MICs into calendar families. For example,
US equity venues share the NYSE-style holiday set, options venues share
US options hours, and CME/NYMEX futures use overnight Globex-style
sessions.

Representative families include:

| Family         | Examples                                               | Notes                                                                                          |
| -------------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------- |
| US equities    | `XNYS`, `XNAS`, `BATS`, `IEXG`                         | NYSE-style holidays, 09:30-16:00 regular hours, and 04:00-09:30 / 16:00-20:00 extended windows |
| US options     | `OPRA`, `XCBO`, `XPHL`                                 | US options classification and close conventions                                                |
| US bonds       | `SIFMA_US`                                             | Includes bond-market holidays such as Columbus Day and Veterans Day                            |
| US futures     | `XCME`, `XCBT`, `XNYM`, `ICE_US`, `CFE`                | Overnight futures sessions with market-specific timezone choices                               |
| Major equities | `XLON`, `XTKS`, `XHKG`, `XEUR`, `XPAR`, `XASX`, `BVMF` | Venue-specific holiday rules where implemented                                                 |
| FX             | `FOREX`                                                | 24x5, Sunday through Friday session family                                                     |
| Crypto         | `CRYPTO`                                               | 24x7 session family                                                                            |

`EXCHANGE_CODES` is exported for discovery and is sourced from
`finance-enums`. `REGION_CODES` lists the supported region aliases.

______________________________________________________________________

## Valid and invalid date series

Common workflows:

```python
from datetime import date
from finance_dates import Calendar

nyse = Calendar.from_exchange("XNYS")

valid_dates = nyse.business_days(date(2024, 7, 1), date(2024, 7, 5))
invalid_holidays = nyse.holidays(date(2024, 7, 1), date(2024, 9, 30))
sessions = nyse.sessions(date(2024, 7, 1), date(2024, 7, 5))
```

`business_days()` returns valid trading dates. `holidays(start, end)`
returns the observed holidays that explain many invalid weekdays.
Weekend invalid dates are not included in `holidays(start, end)`; derive
those separately if you need all invalid calendar dates.

______________________________________________________________________

## Trading hours and early closes

Trading hours are stored in local exchange time and converted to UTC for
timestamp operations. This makes DST transitions explicit at the output
boundary:

```python
from datetime import date
from finance_dates import Calendar

nyse = Calendar.from_exchange("XNYS")
nyse.sessions(date(2024, 3, 8), date(2024, 3, 12))
```

Early closes shorten the final session for the affected trading day.
For example, NYSE July 3, 2024 closes at 13:00 New York time:

```python
nyse.early_close_for(date(2024, 7, 3))  # (13, 0)
```

US equity calendars also expose informational extended-hours windows:

```python
nyse.regular_sessions
# [(9, 30, 0, 16, 0, 0)]

nyse.extended_hours
# [("pre_open", 4, 0, 0, 9, 30, 0), ("after_close", 16, 0, 0, 20, 0, 0)]

nyse.extended_sessions(date(2024, 7, 3), date(2024, 7, 3))
```

`extended_sessions()` returns `(name, open, close)` tuples with UTC
datetimes. On NYSE early-close days, `after_close` begins at the early
close rather than the regular 16:00 close.

Futures sessions can cross midnight. In `regular_sessions`, day offsets
describe the local trading-day relationship:

```python
cme = Calendar.from_exchange("XCME")
cme.regular_sessions
# [(17, 0, -1, 16, 0, 0)]
```

That tuple means the session opens at 17:00 on the previous local day
and closes at 16:00 on the trading day.

______________________________________________________________________

## Scope and limitations

Calendar support is designed for deterministic application logic and
tests. It includes a broad set of exchange families, but it is not a
replacement for venue notices or regulatory calendars.

Important boundaries:

- Holiday rules are implemented per family and may use tabulated lunar
  holidays for markets where simple formulae are not enough.
- Some special closures, ad-hoc national mourning days, weather events,
  or emergency interruptions may not be modeled.
- `holidays(start, end)` returns holidays, not every invalid date.
- `business_days()` is inclusive of both endpoints.
- Extended-hours coverage is currently populated where the calendar has a
  known source-of-truth window; absent `extended_hours` means no extended
  template is configured, not that the venue never has one.
- Datetime inputs are interpreted as UTC when naive; prefer timezone-aware
  UTC datetimes for clarity.
