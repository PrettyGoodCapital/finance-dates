# Calendars

`finance-dates` models a few related ideas:

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
tokyo = Calendar.from_exchange("XTKS")
seoul = Calendar.from_exchange("XKRX")
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

`Calendar.from_exchange()` accepts any MIC that resolves to a modeled
family, plus resolver-only calendar aliases (see below). Exchange codes
known to `finance-enums` but without a dedicated family fall back to the
calendar for their ISO country when one is modeled, and otherwise to a
plain weekmask-only calendar with the appropriate `market_type`. Codes
that are entirely unknown, and region codes outside the supported set,
raise `ValueError` in Python.

The `weekmask` is always seven booleans indexed Monday through Sunday.
Most venues use the standard Monday-Friday week. Saudi Arabia (`XSAU`)
and Tel Aviv (`XTAE`) trade Sunday through Thursday, `FOREX` trades
Sunday through Friday, and `CRYPTO` trades every day.

______________________________________________________________________

## Market families

The resolver groups related MICs into calendar families. Each family
pairs a holiday rule set with a trading-hours template and a weekmask.
US equity venues share the NYSE-style holiday set, options venues share
US options hours, CME/NYMEX futures use overnight Globex-style sessions,
and most international venues have a dedicated national holiday rule set.
A few synthetic futures product-group codes are also accepted for
contracts whose hours differ materially from the broad MIC family.

### US and generic families

| Family / code group | Codes                                                                                                                  | Notes                                                                                            |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| US equities         | `XNYS`, `XNAS`, `XASE`, `ARCX`, `BATS`, `IEXG`, `MEMX`, and other US venue and OTC MICs                                | NYSE-style holidays, 09:30-16:00 ET regular hours, 04:00-09:30 / 16:00-20:00 ET extended windows |
| US options          | `OPRA`, `XCBO`, `C2OX`, `XISE`, `GMNI`, `MCRY`, `MXOP`                                                                 | NYSE holiday set with US options market type                                                     |
| US bonds            | `SIFMA_US`                                                                                                             | SIFMA recommended closes, adding Columbus Day and Veterans Day; 07:00-17:30 ET                   |
| US futures          | `XCME`, `XCBT`, `GLBX`, `XNYM`, `ICE_US`, `CFE`, plus resolver aliases such as `CBOT_GRAINS`, `CME_ENERGY`, `CL`, `ZC` | Overnight and split futures sessions with market-specific timezone choices (see matrix below)    |
| FX                  | `FOREX`                                                                                                                | 24x5 Sunday-through-Friday session family                                                        |
| Crypto              | `CRYPTO`                                                                                                               | 24x7 session family                                                                              |

### International equity calendars

Every venue below has a dedicated national/exchange holiday rule set.
Tokyo, Hong Kong, and Shanghai use split regular sessions for their
lunch breaks; the rest use a single local continuous session. Saudi
Arabia and Tel Aviv use a Sunday-through-Thursday weekmask.

| Region         | Exchanges (MIC)                      | Holiday-system highlights                                                              |
| -------------- | ------------------------------------ | -------------------------------------------------------------------------------------- |
| Canada         | `XTSE` (and Canadian ATS/venue MICs) | Canadian statutory holidays with observed rolls                                        |
| Brazil         | `BVMF`                               | Carnival, Corpus Christi, Black Awareness transition, year-end conventions             |
| Mexico         | `XMEX`                               | Constitution Day, Benito Juárez, Banxico holiday                                       |
| Argentina      | `XBUE`                               | Carnival, movable national holidays, tabulated bridge days                             |
| Chile          | `XSGO`                               | Religious and civic holidays plus tabulated bridge days                                |
| Peru           | `XLIM`                               | National and religious holidays                                                        |
| Colombia       | `XBOG`                               | Emiliani-law Monday-moved holidays                                                     |
| United Kingdom | `XLON`                               | Bank holidays with substitute days and one-off royal closures                          |
| Germany        | `XFRA`, `XEUR` (Xetra)               | Xetra festive block; historically date-limited unity/Whit rules                        |
| France         | `XPAR`                               | Harmonized Euronext holiday set                                                        |
| Netherlands    | `XAMS`                               | Harmonized Euronext holiday set                                                        |
| Belgium        | `XBRU`                               | Harmonized Euronext holiday set                                                        |
| Portugal       | `XLIS`                               | Harmonized Euronext holiday set                                                        |
| Ireland        | `XDUB`                               | Euronext migration of Irish bank holidays                                              |
| Italy          | `XMIL`                               | Euronext base plus Assumption and year-end days                                        |
| Spain          | `XMAD`                               | Spanish national holidays                                                              |
| Switzerland    | `XSWX`                               | Swiss national holidays                                                                |
| Norway         | `XOSL`                               | Nordic holiday set                                                                     |
| Sweden         | `XSTO`                               | Nordic holiday set                                                                     |
| Finland        | `XHEL`                               | Nordic holiday set                                                                     |
| Denmark        | `XCSE`                               | Nordic holiday set (Great Prayer Day through 2023)                                     |
| Iceland        | `XICE`                               | Icelandic national holidays                                                            |
| Poland         | `XWAR`                               | Polish national and religious holidays                                                 |
| Czechia        | `XPRA`                               | Czech national holidays                                                                |
| Hungary        | `XBUD`                               | Hungarian holidays with historically date-limited rules                                |
| Austria        | `XWBO`                               | Austrian holidays with National Day                                                    |
| Japan          | `XTKS`                               | Equinox computation, substitute/citizens' days, split lunch session, 2024 close change |
| Hong Kong      | `XHKG`                               | Astronomical lunar calendar, Buddha's Birthday, split lunch session                    |
| China          | `XSHG`                               | Lunar calendar plus tabulated Golden Week and make-up days, split lunch session        |
| South Korea    | `XKRX`                               | Seollal/Chuseok lunar dates and tabulated substitute holidays                          |
| Taiwan         | `XTAI`                               | Lunar New Year, tabulated bridge/make-up/typhoon closures                              |
| Singapore      | `XSES`                               | Lunar New Year plus tabulated Islamic and festival closures                            |
| Thailand       | `XBKK`                               | Thai royal and Buddhist holidays plus tabulated closures                               |
| Malaysia       | `XKLS`                               | National holidays plus tabulated Islamic and lunar closures                            |
| Indonesia      | `XIDX`                               | National holidays plus tabulated Eid and exceptions                                    |
| Philippines    | `XPHS`                               | Philippine national and religious holidays                                             |
| India          | `XBOM`, `XNSE`                       | Indian exchange holiday set                                                            |
| Australia      | `XASX`                               | Australian holidays with substitute days                                               |
| New Zealand    | `XNZE`                               | New Zealand holidays including tabulated Matariki                                      |
| Saudi Arabia   | `XSAU`                               | Sunday-Thursday week; Eid and national days tabulated                                  |
| Turkey         | `XIST`                               | Turkish national holidays plus tabulated Eid                                           |
| Israel         | `XTAE`                               | Sunday-Thursday week; Hebrew-calendar Jewish holidays                                  |
| UAE            | `XDFM`, `XADS`                       | Emirati national holidays plus tabulated Islamic dates                                 |
| South Africa   | `XJSE`                               | South African public holidays with Sunday-to-Monday rolls                              |

### Holiday systems

The holiday engine composes several rule kinds so that each family can be
expressed declaratively:

- Fixed-date rules with optional weekend-roll observance (forward,
  nearest-weekday, or Sunday-to-Monday).
- Nth-weekday-of-month rules such as US Thanksgiving or the UK bank
  holidays.
- Easter-offset rules for Good Friday, Easter Monday, Corpus Christi, and
  similar movable feasts.
- Computed astronomical rules: Chinese, Korean, and Taiwanese lunar dates
  and the Japanese vernal and autumnal equinoxes.
- Per-market adjustments such as the Japanese substitute/citizens' days
  and the Hong Kong Sunday roll.
- Tabulated rules for events that cannot be derived from a formula:
  Golden Week bridge and make-up days, Eid al-Fitr and Eid al-Adha,
  Jewish holidays, Matariki, and one-off closures (state funerals,
  typhoons, exchange outages).

### Discovery and product-aware calendars

`EXCHANGE_CODES` is exported for discovery and is sourced from
`finance-enums`. `COUNTRY_CODES` and `COUNTRY_CODES3` list the supported
ISO country-code inputs for `Calendar.from_region()`. For product-aware futures
calendars, prefer `Calendar.from_asset(exchange, asset_class, subclass=...)`
with `finance-enums` enum members, or their string values, over synthetic
exchange names.

```python
from finance_dates import Calendar
from finance_enums import AgricultureType, EnergyType, ExchangeCode, UnderlyingAssetClass

Calendar.from_asset(ExchangeCode.XNYM, UnderlyingAssetClass.Commodity, subclass=EnergyType.NaturalGas)
Calendar.from_asset(ExchangeCode.XCBT, UnderlyingAssetClass.Agriculture, subclass=AgricultureType.Corn)
Calendar.from_asset(ExchangeCode.XNYS, UnderlyingAssetClass.Equity)
```

### Region calendars

`Calendar.from_region()` maps an ISO 3166 alpha-2 or alpha-3 country code
to a representative exchange calendar. The following countries have an
explicit mapping; any other code in `COUNTRY_CODES` / `COUNTRY_CODES3`
raises `ValueError`.

| Region                                                                                         | Resolves to                                                                  |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `US`/`USA`                                                                                     | `XNYS`                                                                       |
| `GB`/`GBR`                                                                                     | `XLON`                                                                       |
| `JP`/`JPN`                                                                                     | `XTKS`                                                                       |
| `HK`/`HKG`                                                                                     | `XHKG`                                                                       |
| `CN`/`CHN`                                                                                     | `XSHG`                                                                       |
| `DE`/`DEU`                                                                                     | `XFRA`                                                                       |
| `FR`/`FRA`                                                                                     | `XPAR`                                                                       |
| `CA`/`CAN`                                                                                     | `XTSE`                                                                       |
| `AU`/`AUS`                                                                                     | `XASX`                                                                       |
| `IN`/`IND`                                                                                     | `XNSE`                                                                       |
| `NL`/`NLD`, `BE`/`BEL`, `PT`/`PRT`, `IT`/`ITA`, `ES`/`ESP`, `CH`/`CHE`                         | Euronext / national venues (`XAMS`, `XBRU`, `XLIS`, `XMIL`, `XMAD`, `XSWX`)  |
| `NO`/`NOR`, `SE`/`SWE`, `FI`/`FIN`, `DK`/`DNK`, `IS`/`ISL`                                     | Nordic venues (`XOSL`, `XSTO`, `XHEL`, `XCSE`, `XICE`)                       |
| `PL`/`POL`, `CZ`/`CZE`, `HU`/`HUN`, `AT`/`AUT`, `IE`/`IRL`                                     | Central-European venues (`XWAR`, `XPRA`, `XBUD`, `XWBO`, `XDUB`)             |
| `KR`/`KOR`, `SG`/`SGP`, `TW`/`TWN`, `TH`/`THA`, `MY`/`MYS`, `ID`/`IDN`, `PH`/`PHL`, `NZ`/`NZL` | APAC venues (`XKRX`, `XSES`, `XTAI`, `XBKK`, `XKLS`, `XIDX`, `XPHS`, `XNZE`) |
| `ZA`/`ZAF`, `SA`/`SAU`, `TR`/`TUR`, `IL`/`ISR`, `AE`/`ARE`                                     | EMEA venues (`XJSE`, `XSAU`, `XIST`, `XTAE`, `XDFM`)                         |
| `BR`/`BRA`, `MX`/`MEX`, `AR`/`ARG`, `CL`/`CHL`, `PE`/`PER`, `CO`/`COL`                         | Latin-American venues (`BVMF`, `XMEX`, `XBUE`, `XSGO`, `XLIM`, `XBOG`)       |

### Futures product/calendar/exchange matrix

The table below is the current `Calendar.from_exchange(...)` futures mapping
for product and product-group aliases. Codes in the first column are grouped when
they share the same resolver family and session template.

| Inputs                                                                                                                                                     | Calendar template       | Local regular sessions                                | Notes                                                                                 |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- | ----------------------------------------------------- | ------------------------------------------------------------------------------------- |
| `XCME`, `FCME`, `GLBX`, `XCBT`, `FCBT`, `XKBT`, `SR3`, `ES`, `NQ`, `RTY`, `CME_DAIRY`, `GLOBEX_DAIRY`                                                      | `UsFuturesCme`          | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | Baseline CME Globex overnight template; dairy currently shares this template.         |
| `XNYM`, `NYMEX_ENERGY`, `COMEX_METALS`, `CL`, `MCL`, `QM`, `GC`, `MGC`, `QO`, `CME_ENERGY`, `GLOBEX_ENERGY`, `CME_METALS`, `GLOBEX_METALS`                 | `UsFuturesCmeEnergy`    | `[(17, 0, -1, 16, 0, 0)]` (CT)                        | NYMEX/COMEX energy and metals template with the daily 16:00-17:00 CT maintenance gap. |
| `CBOT_GRAINS`, `CME_GRAINS`, `GLOBEX_GRAINS`, `ZC`, `ZW`, `ZS`, `ZL`, `ZM`, `ZO`, `KE`, `HRS`, `CBOT_OILSEEDS`, `CBOT_WHEAT`, `CBOT_CORN`, `CBOT_SOYBEANS` | `UsFuturesCbotGrains`   | `[(19, 0, -1, 7, 45, 0), (8, 30, 0, 13, 20, 0)]` (CT) | CBOT grain/oilseed split template.                                                    |
| `LE`, `GF`, `HE`, `CME_LIVESTOCK`, `GLOBEX_LIVESTOCK`                                                                                                      | `UsFuturesCmeLivestock` | `[(8, 30, 0, 13, 5, 0)]` (CT)                         | CME livestock daytime template.                                                       |
| `LBR`, `LS`, `CME_LUMBER`, `GLOBEX_LUMBER`                                                                                                                 | `UsFuturesCmeLumber`    | `[(9, 0, 0, 15, 5, 0)]` (CT)                          | CME lumber daytime template.                                                          |
| `ICE_US`                                                                                                                                                   | `UsFuturesIce`          | `[(20, 0, -1, 18, 0, 0)]` (ET)                        | ICE US energy/softs broad exchange template.                                          |
| `CFE`                                                                                                                                                      | `UsFuturesCfe`          | `[(8, 30, 0, 15, 15, 0)]` (CT)                        | CBOE Futures Exchange daytime template with the full US holiday set.                  |

Only enum-backed exchange and generic identifiers are exported by
`EXCHANGE_CODES`; many product mnemonics and synthetic product-group names in
this matrix are resolver-only aliases accepted by `Calendar.from_exchange()`.
For product-aware code, prefer `Calendar.from_product(...)` or
`Calendar.from_asset(...)` when the exchange/product vocabulary is known.

The baseline CME Globex holiday set used by the CME/NYMEX/CBOT templates
currently captures full-closure holidays (New Year's Day, Good Friday, Memorial
Day, Independence Day, Thanksgiving, Christmas); ICE US uses a smaller set (New
Year's Day, Good Friday, Christmas) and CFE uses the full US equity set. CME
product holiday notices include many partial closes that remain out of scope
until session-specific holiday truncation is modeled.

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
`Calendar.from_asset(ExchangeCode.XNYM, UnderlyingAssetClass.Commodity, subclass=EnergyType.NaturalGas)`, `Calendar.from_product("XNYM", "NaturalGas")`,
and lower-level aliases such as `XNYM`, `NYMEX_ENERGY`, `COMEX_METALS`, `CL`,
`MCL`, `QM`, `GC`, `MGC`, and `QO`. The daily 16:00-17:00 CT maintenance
window is closed; `next_open()` from that gap returns the 17:00 CT open for the
next trade date.

For broader category-level parity with CME Globex filters, energy and metals
aliases also include `CME_ENERGY`, `GLOBEX_ENERGY`, `CME_METALS`, and
`GLOBEX_METALS`.

CBOT grain and oilseed futures have a more unusual split schedule. Use
`Calendar.from_asset(ExchangeCode.XCBT, UnderlyingAssetClass.Agriculture, subclass=AgricultureType.Corn)` when resolving from finance-enums vocabulary, or
the lower-level synthetic product-group codes `CBOT_GRAINS`, `CME_GRAINS`,
`GLOBEX_GRAINS`, or product mnemonics such as `ZC`, `ZW`, and `ZS` when that
distinction matters:

```python
grains = Calendar.from_exchange("CBOT_GRAINS")
grains.regular_sessions
# [(19, 0, -1, 7, 45, 0), (8, 30, 0, 13, 20, 0)]
```

Equivalent grain/oilseed aliases include `CBOT_OILSEEDS`, `CBOT_WHEAT`,
`CBOT_CORN`, `CBOT_SOYBEANS`, `ZL`, `ZM`, `ZO`, `KE`, and `HRS`.

`CME_LIVESTOCK` / `GLOBEX_LIVESTOCK` and the product mnemonics `LE`, `GF`, and
`HE` use the daytime livestock session (08:30-13:05 CT). `CME_LUMBER` /
`GLOBEX_LUMBER` and `LBR` / `LS` use a daytime lumber session (09:00-15:05 CT).
`CME_DAIRY` / `GLOBEX_DAIRY` use the overnight dairy template (17:00 previous
day to 16:00 trade date CT).

### Source-backed status and remaining placeholders

This is a current audit of major schedule assumptions and placeholders that are
still present in the futures layer.

| Area                                                                                 | Current behavior                                 | Status        | Needed source to remove placeholder                                                  |
| ------------------------------------------------------------------------------------ | ------------------------------------------------ | ------------- | ------------------------------------------------------------------------------------ |
| `LE` / `GF` / `HE` / `CME_LIVESTOCK` / `GLOBEX_LIVESTOCK`                            | Routed to 08:30-13:05 CT livestock session       | Source-backed | Optional chapter citation if you want external-link provenance in docs               |
| `CME_DAIRY` / `GLOBEX_DAIRY`                                                         | Routed to 17:00-16:00 CT overnight dairy session | Source-backed | Optional chapter citation if you want external-link provenance in docs               |
| `LBR` / `LS` / `CME_LUMBER` / `GLOBEX_LUMBER`                                        | Routed to 09:00-15:05 CT lumber session          | Source-backed | Optional chapter citation if you want external-link provenance in docs               |
| `ZC` / `ZW` / `ZS` / `ZL` / `ZM` / `ZO` / `KE` / `HRS` and CBOT grain bucket aliases | Aliased to shared grain split template           | Partial       | Product-level confirmation for exact overnight/day split by contract group           |
| CME futures historical transitions                                                   | Mostly static schedule templates                 | Partial       | Official effective-date change logs for each product family (not just current hours) |

If you can share authoritative links or chapter extracts for those items, they
can be promoted from placeholder/partial to source-backed templates.

______________________________________________________________________

## Scope and limitations

Calendar support is designed for deterministic application logic and
tests. It includes a broad set of exchange families, but it is not a
replacement for venue notices or regulatory calendars.

Important boundaries:

- Holiday rules are implemented per family and may use tabulated lunar,
  Islamic, or Hebrew holidays for markets where simple formulae are not
  enough.
- Tabulated holiday data has finite horizons. Lunar Golden Week and
  bridge/make-up tables and similar civic-arrangement tables extend to
  roughly 2030, and the Eid al-Fitr / Eid al-Adha tables for Saudi Arabia
  and Turkey currently run through 2026. Dates beyond a table's horizon
  are not modeled until the table is extended.
- Trading-hours templates are date-effective only where explicitly implemented,
  currently including Tokyo's 2024 close-time extension. Other historical
  schedule changes may still use current-rule approximations.
- Weekmasks are static per calendar. Saudi Arabia (`XSAU`) and Tel Aviv
  (`XTAE`) trade Sunday-Thursday; other venues use a Monday-Friday week.
- Some special closures, ad-hoc national mourning days, weather events,
  or emergency interruptions may not be modeled beyond the one-off dates
  already tabulated.
- Early closes are NYSE-shaped: a single early close time applied to the
  final regular session. Non-US half days are not generally modeled.
- `holidays(start, end)` returns holidays, not every invalid date.
- `business_days()` is inclusive of both endpoints.
- Extended-hours coverage is currently populated where the calendar has a
  known source-of-truth window; absent `extended_hours` means no extended
  template is configured, not that the venue never has one.
- Datetime inputs are interpreted as UTC when naive; prefer timezone-aware
  UTC datetimes for clarity.
