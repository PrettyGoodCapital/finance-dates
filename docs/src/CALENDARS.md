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

Create calendars from exchange codes or ISO country codes:

```python
from finance_dates import Calendar

nyse = Calendar.from_exchange("XNYS")
london = Calendar.from_exchange("XLON")
us = Calendar.from_region("US")
```

Calendar objects expose:

| Attribute / method                             | Meaning                                                                             |
| ---------------------------------------------- | ----------------------------------------------------------------------------------- |
| `name`                                         | Calendar code used by the resolver                                                  |
| `market_type`                                  | `finance-enums` `MarketType` label, e.g. `Equities`, `Options`, `Futures`           |
| `weekmask`                                     | Seven booleans, indexed Monday through Sunday                                       |
| `timezone`                                     | IANA timezone for trading hours, or `""` if no hours are configured                 |
| `regular_sessions`                             | One or more local regular session templates as open/close hour/minute/offset tuples |
| `extended_hours`                               | Named local extended-hours templates where available                                |
| `is_business_day(date)`                        | True when the date passes weekmask and holiday checks                               |
| `is_holiday(date)`                             | True when the date is an observed holiday                                           |
| `holidays(year)`                               | Observed holidays for a year                                                        |
| `holidays(start, end)`                         | Holidays in an inclusive range                                                      |
| `business_days_between(start, end)`            | Count of valid business days in an inclusive range                                  |
| `business_days(start, end)`                    | Exchange-aware valid dates                                                          |
| `sessions(start, end)`                         | UTC open/close datetimes for regular sessions                                       |
| `extended_sessions(start, end)`                | Named UTC extended-hours windows                                                    |
| `is_open(datetime)`                            | True when a UTC-aware timestamp is inside a regular session                         |
| `next_open(datetime)` / `next_close(datetime)` | Next regular-session boundary if configured                                         |
| `early_close_for(date)`                        | Local `(hour, minute)` early close or `None`                                        |

Unknown exchange or region codes raise `ValueError` in Python.

______________________________________________________________________

## Market families

The resolver groups related MICs into calendar families. For example,
US equity venues share the NYSE-style holiday set, options venues share
US options hours, and CME/NYMEX futures use overnight Globex-style
sessions. A few synthetic futures product-group codes are also accepted for
contracts whose hours differ materially from the broad MIC family.

Representative families include:

| Family         | Examples                                                                                                                       | Notes                                                                                            |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| US equities    | `XNYS`, `XNAS`, `BATS`, `IEXG`                                                                                                 | NYSE-style holidays, 09:30-16:00 regular hours, and 04:00-09:30 / 16:00-20:00 extended windows   |
| US options     | `OPRA`, `XCBO`, `XPHL`                                                                                                         | US options classification and close conventions                                                  |
| US bonds       | `SIFMA_US`                                                                                                                     | Includes bond-market holidays such as Columbus Day and Veterans Day                              |
| US futures     | `XCME`, `XCBT`, `XNYM`, `ICE_US`, `CFE`, `CBOT_GRAINS`, `CME_ENERGY`, `CME_METALS`, `CME_LIVESTOCK`, `CME_DAIRY`, `CME_LUMBER` | Overnight and split futures sessions with market-specific timezone choices                       |
| Major equities | `XLON`, `XTKS`, `XHKG`, `XSHG`, `XEUR`, `XPAR`, `XASX`, `BVMF`                                                                 | Venue-specific holiday rules where implemented; selected APAC venues expose split lunch sessions |
| FX             | `FOREX`                                                                                                                        | 24x5, Sunday through Friday session family                                                       |
| Crypto         | `CRYPTO`                                                                                                                       | 24x7 session family                                                                              |

`EXCHANGE_CODES` is exported for discovery and is sourced from
`finance-enums`. `COUNTRY_CODES` and `COUNTRY_CODES3` list the supported
ISO country-code inputs for `Calendar.from_region()`. For product-aware futures
calendars, prefer `Calendar.from_asset(exchange, asset_class, subclass=...)`
with `finance-enums` enum members over synthetic exchange names.

```python
from finance_dates import Calendar
from finance_enums import AgricultureType, EnergyType, ExchangeCode, UnderlyingAssetClass

Calendar.from_asset(ExchangeCode.XNYM, UnderlyingAssetClass.Commodity, subclass=EnergyType.NaturalGas)
Calendar.from_asset(ExchangeCode.XCBT, UnderlyingAssetClass.Agriculture, subclass=AgricultureType.Corn)
Calendar.from_asset(ExchangeCode.XNYS, UnderlyingAssetClass.Equity)
```

### Commodity product/calendar/exchange matrix

The table below is the full currently supported commodity mapping used by
`Calendar.from_exchange(...)` for futures product groups and aliases.

| Product or exchange code | Exchange family              | Calendar template       | Local regular sessions                                | Source status                                                                   |
| ------------------------ | ---------------------------- | ----------------------- | ----------------------------------------------------- | ------------------------------------------------------------------------------- |
| `XCME`                   | CME Globex (CME)             | `UsFuturesCme`          | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Verified from CME Globex schedule text                                          |
| `XCBT`                   | CME Globex (CBOT)            | `UsFuturesCme`          | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Verified from CME Globex schedule text                                          |
| `XKBT`                   | CME Globex (CBOT)            | `UsFuturesCme`          | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Verified from CME Globex schedule text                                          |
| `GLBX`                   | CME Globex synthetic         | `UsFuturesCme`          | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Verified from CME Globex schedule text                                          |
| `XNYM`                   | NYMEX (CME Globex)           | `UsFuturesCmeEnergy`    | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Verified from CME Globex schedule text                                          |
| `NYMEX_ENERGY`           | NYMEX synthetic              | `UsFuturesCmeEnergy`    | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Verified from CME Globex schedule text                                          |
| `COMEX_METALS`           | COMEX synthetic              | `UsFuturesCmeEnergy`    | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Verified from CME Globex schedule text                                          |
| `CME_ENERGY`             | CME category alias           | `UsFuturesCmeEnergy`    | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Verified open in CME Globex filters; close window inferred from active template |
| `GLOBEX_ENERGY`          | Globex category alias        | `UsFuturesCmeEnergy`    | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Verified open in CME Globex filters; close window inferred from active template |
| `CME_METALS`             | CME category alias           | `UsFuturesCmeEnergy`    | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Verified open in CME Globex filters; close window inferred from active template |
| `GLOBEX_METALS`          | Globex category alias        | `UsFuturesCmeEnergy`    | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Verified open in CME Globex filters; close window inferred from active template |
| `CBOT_GRAINS`            | CBOT product-group synthetic | `UsFuturesCbotGrains`   | `[(19, 0, -1, 7, 45, 0), (8, 30, 0, 13, 20, 0)]` (CT) | Verified from CME grain specs and Globex category open                          |
| `CME_GRAINS`             | CME category alias           | `UsFuturesCbotGrains`   | `[(19, 0, -1, 7, 45, 0), (8, 30, 0, 13, 20, 0)]` (CT) | Verified from CME grain specs and Globex category open                          |
| `GLOBEX_GRAINS`          | Globex category alias        | `UsFuturesCbotGrains`   | `[(19, 0, -1, 7, 45, 0), (8, 30, 0, 13, 20, 0)]` (CT) | Verified from CME grain specs and Globex category open                          |
| `CBOT_OILSEEDS`          | CBOT product-group alias     | `UsFuturesCbotGrains`   | `[(19, 0, -1, 7, 45, 0), (8, 30, 0, 13, 20, 0)]` (CT) | Inherited from grain template; awaiting product-level source table              |
| `CBOT_WHEAT`             | CBOT product-group alias     | `UsFuturesCbotGrains`   | `[(19, 0, -1, 7, 45, 0), (8, 30, 0, 13, 20, 0)]` (CT) | Inherited from grain template; awaiting product-level source table              |
| `CBOT_CORN`              | CBOT product-group alias     | `UsFuturesCbotGrains`   | `[(19, 0, -1, 7, 45, 0), (8, 30, 0, 13, 20, 0)]` (CT) | Inherited from grain template; awaiting product-level source table              |
| `CBOT_SOYBEANS`          | CBOT product-group alias     | `UsFuturesCbotGrains`   | `[(19, 0, -1, 7, 45, 0), (8, 30, 0, 13, 20, 0)]` (CT) | Inherited from grain template; awaiting product-level source table              |
| `CME_LIVESTOCK`          | CME category alias           | `UsFuturesCmeLivestock` | `[(8, 30, 0, 13, 5, 0)]` (CT)                         | Source-backed from `data/Trading Hours Export.xlsx` (LE/GF/HE rows)             |
| `GLOBEX_LIVESTOCK`       | Globex category alias        | `UsFuturesCmeLivestock` | `[(8, 30, 0, 13, 5, 0)]` (CT)                         | Source-backed from `data/Trading Hours Export.xlsx` (LE/GF/HE rows)             |
| `CME_DAIRY`              | CME category alias           | `UsFuturesCme`          | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Source-backed from `data/Trading Hours Export.xlsx` (DC/GDK/GNF rows)           |
| `GLOBEX_DAIRY`           | Globex category alias        | `UsFuturesCme`          | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Source-backed from `data/Trading Hours Export.xlsx` (DC/GDK/GNF rows)           |
| `CME_LUMBER`             | CME category alias           | `UsFuturesCmeLumber`    | `[(9, 0, 0, 15, 5, 0)]` (CT)                          | Source-backed from `data/Trading Hours Export.xlsx` (LBR row)                   |
| `GLOBEX_LUMBER`          | Globex category alias        | `UsFuturesCmeLumber`    | `[(9, 0, 0, 15, 5, 0)]` (CT)                          | Source-backed from `data/Trading Hours Export.xlsx` (LBR row)                   |

The baseline CME holiday set used by these futures templates currently captures
full-closure holidays (New Year's Day, Good Friday, Christmas). CME product
holiday notices include many partial closes that remain out of scope until
session-specific holiday truncation is modeled.

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

Regular trading days may contain more than one session. Venues with scheduled
lunch breaks expose one `regular_sessions` tuple per interval, and
`sessions(start, end)` returns one UTC open/close pair per regular session per
business day:

```python
tokyo = Calendar.from_exchange("XTKS")
tokyo.regular_sessions
# [(9, 0, 0, 11, 30, 0), (12, 30, 0, 15, 30, 0)]

hong_kong = Calendar.from_exchange("XHKG")
hong_kong.regular_sessions
# [(9, 30, 0, 12, 0, 0), (13, 0, 0, 16, 0, 0)]

shanghai = Calendar.from_exchange("XSHG")
shanghai.regular_sessions
# [(9, 30, 0, 11, 30, 0), (13, 0, 0, 15, 0, 0)]
```

The Shanghai afternoon interval includes the closing call auction through
15:00. Tokyo uses a date-effective schedule around the 2024-11-05 trading-hours
extension: dates before the change close at 15:00 local, and dates on or after
the change use the current 15:30 close. Most other built-in calendars still use
static current-rule templates unless otherwise documented.

`next_open()` and `next_close()` return the next matching boundary inclusively.
At an exact session open, `is_open()` is true and `next_open()` returns that same
instant. At an exact session close, `is_open()` is false and `next_close()`
returns that same close instant.

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

NYMEX energy futures use the same Chicago-time Globex template through
`Calendar.from_asset(ExchangeCode.XNYM, UnderlyingAssetClass.Commodity,
subclass=EnergyType.NaturalGas)` and the lower-level `XNYM`, `NYMEX_ENERGY`,
and `COMEX_METALS` exchange aliases. The daily 16:00-17:00 CT maintenance window
is closed; `next_open()` from that gap returns the 17:00 CT open for the next
trade date.

For broader category-level parity with CME Globex filters, energy and metals
aliases also include `CME_ENERGY`, `GLOBEX_ENERGY`, `CME_METALS`, and
`GLOBEX_METALS`.

CBOT grain and oilseed futures have a more unusual split schedule. Use
`Calendar.from_asset(ExchangeCode.XCBT, UnderlyingAssetClass.Agriculture,
subclass=AgricultureType.Corn)` when resolving from finance-enums vocabulary, or
the lower-level synthetic product-group code `CBOT_GRAINS` / `CME_GRAINS` when
that distinction matters:

```python
grains = Calendar.from_exchange("CBOT_GRAINS")
grains.regular_sessions
# [(19, 0, -1, 7, 45, 0), (8, 30, 0, 13, 20, 0)]
```

Equivalent grain/oilseed aliases include `GLOBEX_GRAINS`, `CBOT_OILSEEDS`,
`CBOT_WHEAT`, `CBOT_CORN`, and `CBOT_SOYBEANS`.

`CME_LIVESTOCK` / `GLOBEX_LIVESTOCK` use the daytime livestock session
(08:30-13:05 CT). `CME_LUMBER` / `GLOBEX_LUMBER` use a daytime lumber session
(09:00-15:05 CT). `CME_DAIRY` / `GLOBEX_DAIRY` use the overnight dairy template
(17:00 previous day to 16:00 trade date CT).

Synthetic product-group codes are accepted by `Calendar.from_exchange()`, but
they are not part of the `finance-enums` MIC list exported as `EXCHANGE_CODES`.

### Source-backed status and remaining placeholders

This is a current audit of major schedule assumptions and placeholders that are
still present in the futures layer.

| Area                                                           | Current behavior                                 | Status        | Needed source to remove placeholder                                                  |
| -------------------------------------------------------------- | ------------------------------------------------ | ------------- | ------------------------------------------------------------------------------------ |
| `CME_LIVESTOCK` / `GLOBEX_LIVESTOCK`                           | Routed to 08:30-13:05 CT livestock session       | Source-backed | Optional chapter citation if you want external-link provenance in docs               |
| `CME_DAIRY` / `GLOBEX_DAIRY`                                   | Routed to 17:00-16:00 CT overnight dairy session | Source-backed | Optional chapter citation if you want external-link provenance in docs               |
| `CME_LUMBER` / `GLOBEX_LUMBER`                                 | Routed to 09:00-15:05 CT lumber session          | Source-backed | Optional chapter citation if you want external-link provenance in docs               |
| `CBOT_OILSEEDS` / `CBOT_WHEAT` / `CBOT_CORN` / `CBOT_SOYBEANS` | Aliased to shared grain split template           | Partial       | Product-level confirmation for exact overnight/day split by contract group           |
| CME futures historical transitions                             | Mostly static schedule templates                 | Partial       | Official effective-date change logs for each product family (not just current hours) |

If you can share authoritative links or chapter extracts for those items, they
can be promoted from placeholder/partial to source-backed templates.

______________________________________________________________________

## Scope and limitations

Calendar support is designed for deterministic application logic and
tests. It includes a broad set of exchange families, but it is not a
replacement for venue notices or regulatory calendars.

Important boundaries:

- Holiday rules are implemented per family and may use tabulated lunar
  holidays for markets where simple formulae are not enough.
- Trading-hours templates are date-effective only where explicitly implemented,
  currently including Tokyo's 2024 close-time extension. Other historical
  schedule changes may still use current-rule approximations.
- Some special closures, ad-hoc national mourning days, weather events,
  or emergency interruptions may not be modeled.
- `holidays(start, end)` returns holidays, not every invalid date.
- `business_days()` is inclusive of both endpoints.
- Extended-hours coverage is currently populated where the calendar has a
  known source-of-truth window; absent `extended_hours` means no extended
  template is configured, not that the venue never has one.
- Datetime inputs are interpreted as UTC when naive; prefer timezone-aware
  UTC datetimes for clarity.
