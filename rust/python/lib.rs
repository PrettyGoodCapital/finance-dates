//! pyo3 bindings for finance-dates.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDate, PyDateTime, PyTzInfo};

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};

use ::finance_dates::{
    business_day_range as core_business_day_range, calendar_for_exchange, calendar_for_region,
    date_range as core_date_range, EXCHANGE_CODES, REGION_CODES, STANDARD_WEEKMASK,
};

fn pydate_to_naive(d: &Bound<'_, PyDate>) -> PyResult<NaiveDate> {
    let y: i32 = d.getattr("year")?.extract()?;
    let m: u32 = d.getattr("month")?.extract()?;
    let day: u32 = d.getattr("day")?.extract()?;
    NaiveDate::from_ymd_opt(y, m, day)
        .ok_or_else(|| PyValueError::new_err("invalid date components"))
}

fn naive_to_pydate<'py>(py: Python<'py>, d: NaiveDate) -> PyResult<Bound<'py, PyDate>> {
    PyDate::new(py, d.year(), d.month() as u8, d.day() as u8)
}

fn pydatetime_to_utc(dt: &Bound<'_, PyDateTime>) -> PyResult<DateTime<Utc>> {
    let y: i32 = dt.getattr("year")?.extract()?;
    let mo: u32 = dt.getattr("month")?.extract()?;
    let d: u32 = dt.getattr("day")?.extract()?;
    let h: u32 = dt.getattr("hour")?.extract()?;
    let mi: u32 = dt.getattr("minute")?.extract()?;
    let s: u32 = dt.getattr("second")?.extract()?;
    let us: u32 = dt.getattr("microsecond")?.extract()?;

    let nd =
        NaiveDate::from_ymd_opt(y, mo, d).ok_or_else(|| PyValueError::new_err("invalid date"))?;
    let nt = NaiveTime::from_hms_micro_opt(h, mi, s, us)
        .ok_or_else(|| PyValueError::new_err("invalid time"))?;
    let ndt = nd.and_time(nt);

    // If tzinfo is set, subtract its UTC offset; else assume already UTC.
    let utc_offset = dt.call_method0("utcoffset")?;
    if utc_offset.is_none() {
        return Ok(Utc.from_utc_datetime(&ndt));
    }
    let total_seconds: f64 = utc_offset.call_method0("total_seconds")?.extract()?;
    Ok(Utc.from_utc_datetime(&ndt) - chrono::Duration::seconds(total_seconds as i64))
}

fn utc_to_pydatetime<'py>(
    py: Python<'py>,
    when: DateTime<Utc>,
) -> PyResult<Bound<'py, PyDateTime>> {
    let tz = PyTzInfo::utc(py)?;
    PyDateTime::new(
        py,
        when.year(),
        when.month() as u8,
        when.day() as u8,
        when.hour() as u8,
        when.minute() as u8,
        when.second() as u8,
        when.timestamp_subsec_micros(),
        Some(&tz),
    )
}

/// Inclusive calendar-day range with a fixed step in days.
#[pyfunction]
#[pyo3(signature = (start, end, *, step_days = 1))]
fn date_range<'py>(
    py: Python<'py>,
    start: &Bound<'py, PyDate>,
    end: &Bound<'py, PyDate>,
    step_days: u32,
) -> PyResult<Vec<Bound<'py, PyDate>>> {
    let s = pydate_to_naive(start)?;
    let e = pydate_to_naive(end)?;
    core_date_range(s, e, step_days)
        .into_iter()
        .map(|d| naive_to_pydate(py, d))
        .collect()
}

/// Holiday calendar for an exchange or region.
#[pyclass(module = "finance_dates", name = "Calendar")]
struct PyCalendar {
    inner: Option<::finance_dates::Calendar>,
    range_start: Option<NaiveDate>,
    range_end: Option<NaiveDate>,
    step_days: u32,
}

impl PyCalendar {
    fn from_inner(inner: ::finance_dates::Calendar) -> Self {
        Self {
            inner: Some(inner),
            range_start: None,
            range_end: None,
            step_days: 1,
        }
    }

    fn inner(&self) -> PyResult<&::finance_dates::Calendar> {
        self.inner
            .as_ref()
            .ok_or_else(|| PyValueError::new_err("calendar was created from a plain date range"))
    }

    fn resolve_range(
        &self,
        start: Option<&Bound<'_, PyDate>>,
        end: Option<&Bound<'_, PyDate>>,
    ) -> PyResult<(NaiveDate, NaiveDate)> {
        match (start, end) {
            (Some(s), Some(e)) => Ok((pydate_to_naive(s)?, pydate_to_naive(e)?)),
            (None, None) => match (self.range_start, self.range_end) {
                (Some(s), Some(e)) => Ok((s, e)),
                _ => Err(PyValueError::new_err(
                    "start and end are required for this calendar",
                )),
            },
            _ => Err(PyValueError::new_err(
                "start and end must be provided together",
            )),
        }
    }
}

#[pymethods]
impl PyCalendar {
    #[classmethod]
    #[pyo3(signature = (start, end, *, step_days = 1))]
    fn from_range(
        _cls: &Bound<'_, pyo3::types::PyType>,
        start: &Bound<'_, PyDate>,
        end: &Bound<'_, PyDate>,
        step_days: u32,
    ) -> PyResult<Self> {
        Ok(Self {
            inner: None,
            range_start: Some(pydate_to_naive(start)?),
            range_end: Some(pydate_to_naive(end)?),
            step_days,
        })
    }

    #[classmethod]
    fn from_exchange(_cls: &Bound<'_, pyo3::types::PyType>, code: &str) -> PyResult<Self> {
        calendar_for_exchange(code)
            .map(Self::from_inner)
            .ok_or_else(|| PyValueError::new_err(format!("unknown exchange code: {code}")))
    }

    #[classmethod]
    fn from_region(_cls: &Bound<'_, pyo3::types::PyType>, code: &str) -> PyResult<Self> {
        calendar_for_region(code)
            .map(Self::from_inner)
            .ok_or_else(|| PyValueError::new_err(format!("unknown region code: {code}")))
    }

    #[getter]
    fn name(&self) -> PyResult<String> {
        match &self.inner {
            Some(inner) => Ok(inner.name.clone()),
            None => Ok("range".to_string()),
        }
    }

    #[getter]
    fn market_type(&self) -> PyResult<String> {
        match &self.inner {
            Some(inner) => Ok(inner.market_type.as_str().to_string()),
            None => Ok("range".to_string()),
        }
    }

    #[getter]
    fn weekmask(&self) -> Vec<bool> {
        match &self.inner {
            Some(inner) => inner.weekmask.to_vec(),
            None => STANDARD_WEEKMASK.to_vec(),
        }
    }

    /// Trading sessions as `[(open_hh, open_mm, open_offset, close_hh, close_mm, close_offset)]`.
    #[getter]
    fn regular_sessions(&self) -> Vec<(u32, u32, i32, u32, u32, i32)> {
        let Some(inner) = &self.inner else {
            return vec![];
        };
        match &inner.trading_hours {
            None => vec![],
            Some(th) => th
                .sessions
                .iter()
                .map(|s| {
                    (
                        s.open.hour(),
                        s.open.minute(),
                        s.open_day_offset,
                        s.close.hour(),
                        s.close.minute(),
                        s.close_day_offset,
                    )
                })
                .collect(),
        }
    }

    /// Extended trading windows as `[(name, open_hh, open_mm, open_offset, close_hh, close_mm, close_offset)]`.
    #[getter]
    fn extended_hours(&self) -> Vec<(String, u32, u32, i32, u32, u32, i32)> {
        let Some(inner) = &self.inner else {
            return vec![];
        };
        match &inner.trading_hours {
            None => vec![],
            Some(th) => th
                .extended_sessions
                .iter()
                .map(|s| {
                    (
                        s.name.to_string(),
                        s.session.open.hour(),
                        s.session.open.minute(),
                        s.session.open_day_offset,
                        s.session.close.hour(),
                        s.session.close.minute(),
                        s.session.close_day_offset,
                    )
                })
                .collect(),
        }
    }

    /// IANA timezone name for trading hours, or "" if no hours configured.
    #[getter]
    fn timezone(&self) -> String {
        let Some(inner) = &self.inner else {
            return String::new();
        };
        match &inner.trading_hours {
            Some(th) => th.timezone.name().to_string(),
            None => String::new(),
        }
    }

    fn is_business_day(&self, d: &Bound<'_, PyDate>) -> PyResult<bool> {
        Ok(self.inner()?.is_business_day(pydate_to_naive(d)?))
    }

    fn is_holiday(&self, d: &Bound<'_, PyDate>) -> PyResult<bool> {
        Ok(self.inner()?.is_holiday(pydate_to_naive(d)?))
    }

    fn next_business_day<'py>(
        &self,
        py: Python<'py>,
        d: &Bound<'py, PyDate>,
    ) -> PyResult<Bound<'py, PyDate>> {
        naive_to_pydate(py, self.inner()?.next_business_day(pydate_to_naive(d)?))
    }

    fn previous_business_day<'py>(
        &self,
        py: Python<'py>,
        d: &Bound<'py, PyDate>,
    ) -> PyResult<Bound<'py, PyDate>> {
        naive_to_pydate(py, self.inner()?.previous_business_day(pydate_to_naive(d)?))
    }

    fn business_days_between(
        &self,
        start: &Bound<'_, PyDate>,
        end: &Bound<'_, PyDate>,
    ) -> PyResult<i64> {
        Ok(self
            .inner()?
            .business_days_between(pydate_to_naive(start)?, pydate_to_naive(end)?))
    }

    #[pyo3(signature = (start = None, end = None, *, step_days = None))]
    fn days<'py>(
        &self,
        py: Python<'py>,
        start: Option<&Bound<'py, PyDate>>,
        end: Option<&Bound<'py, PyDate>>,
        step_days: Option<u32>,
    ) -> PyResult<Vec<Bound<'py, PyDate>>> {
        let (s, e) = self.resolve_range(start, end)?;
        let step = step_days.unwrap_or(self.step_days);
        core_date_range(s, e, step)
            .into_iter()
            .map(|d| naive_to_pydate(py, d))
            .collect()
    }

    #[pyo3(signature = (start = None, end = None))]
    fn business_days<'py>(
        &self,
        py: Python<'py>,
        start: Option<&Bound<'py, PyDate>>,
        end: Option<&Bound<'py, PyDate>>,
    ) -> PyResult<Vec<Bound<'py, PyDate>>> {
        let (s, e) = self.resolve_range(start, end)?;
        match &self.inner {
            Some(inner) => inner
                .business_day_range(s, e)
                .into_iter()
                .map(|d| naive_to_pydate(py, d))
                .collect(),
            None => core_business_day_range(s, e, &STANDARD_WEEKMASK, &Default::default())
                .into_iter()
                .map(|d| naive_to_pydate(py, d))
                .collect(),
        }
    }

    #[pyo3(signature = (start, end = None))]
    fn holidays<'py>(
        &self,
        py: Python<'py>,
        start: &Bound<'py, PyAny>,
        end: Option<&Bound<'py, PyDate>>,
    ) -> PyResult<Vec<Bound<'py, PyDate>>> {
        let inner = self.inner()?;
        if let Ok(year) = start.extract::<i32>() {
            if end.is_some() {
                return Err(PyValueError::new_err(
                    "end must be omitted when holidays() is called with a year",
                ));
            }
            return inner
                .holidays(year)
                .iter()
                .map(|d| naive_to_pydate(py, *d))
                .collect();
        }
        let start_date = start.cast::<PyDate>()?;
        let Some(end_date) = end else {
            return Err(PyValueError::new_err(
                "end is required when holidays() is called with a start date",
            ));
        };
        let s = pydate_to_naive(start_date)?;
        let e = pydate_to_naive(end_date)?;
        inner
            .holidays_between(s, e)
            .iter()
            .map(|d| naive_to_pydate(py, *d))
            .collect()
    }

    /// `(open, close)` UTC datetimes for every business day in
    /// `[start, end]` (inclusive), with early-close adjustments applied.
    /// Each entry corresponds to one regular trading session. Returns an
    /// empty list if no trading hours are configured.
    fn sessions<'py>(
        &self,
        py: Python<'py>,
        start: &Bound<'py, PyDate>,
        end: &Bound<'py, PyDate>,
    ) -> PyResult<Vec<(Bound<'py, PyDateTime>, Bound<'py, PyDateTime>)>> {
        let s = pydate_to_naive(start)?;
        let e = pydate_to_naive(end)?;
        self.inner()?
            .sessions_between(s, e)
            .into_iter()
            .map(|(o, c)| Ok((utc_to_pydatetime(py, o)?, utc_to_pydatetime(py, c)?)))
            .collect()
    }

    fn extended_sessions<'py>(
        &self,
        py: Python<'py>,
        start: &Bound<'py, PyDate>,
        end: &Bound<'py, PyDate>,
    ) -> PyResult<Vec<(String, Bound<'py, PyDateTime>, Bound<'py, PyDateTime>)>> {
        let s = pydate_to_naive(start)?;
        let e = pydate_to_naive(end)?;
        self.inner()?
            .extended_sessions_between(s, e)
            .into_iter()
            .map(|(name, o, c)| {
                Ok((
                    name.to_string(),
                    utc_to_pydatetime(py, o)?,
                    utc_to_pydatetime(py, c)?,
                ))
            })
            .collect()
    }

    /// True when `when` is inside a regular trading session.
    fn is_open(&self, when: &Bound<'_, PyDateTime>) -> PyResult<bool> {
        Ok(self.inner()?.is_open(pydatetime_to_utc(when)?))
    }

    fn next_open<'py>(
        &self,
        py: Python<'py>,
        when: &Bound<'py, PyDateTime>,
    ) -> PyResult<Option<Bound<'py, PyDateTime>>> {
        let w = pydatetime_to_utc(when)?;
        match self.inner()?.next_open(w) {
            Some(t) => Ok(Some(utc_to_pydatetime(py, t)?)),
            None => Ok(None),
        }
    }

    fn next_close<'py>(
        &self,
        py: Python<'py>,
        when: &Bound<'py, PyDateTime>,
    ) -> PyResult<Option<Bound<'py, PyDateTime>>> {
        let w = pydatetime_to_utc(when)?;
        match self.inner()?.next_close(w) {
            Some(t) => Ok(Some(utc_to_pydatetime(py, t)?)),
            None => Ok(None),
        }
    }

    /// Local early-close time for `date` as `(hour, minute)`, or `None`.
    fn early_close_for(&self, date: &Bound<'_, PyDate>) -> PyResult<Option<(u32, u32)>> {
        let d = pydate_to_naive(date)?;
        Ok(self
            .inner()?
            .early_close_for(d)
            .map(|t| (t.hour(), t.minute())))
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            Some(inner) => format!("Calendar({}, {})", inner.name, inner.market_type.as_str()),
            None => "Calendar(range)".to_string(),
        }
    }
}

#[pymodule]
fn finance_dates(_py: Python, m: &Bound<PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(date_range, m)?)?;
    m.add_class::<PyCalendar>()?;
    m.add("EXCHANGE_CODES", EXCHANGE_CODES.to_vec())?;
    m.add("REGION_CODES", REGION_CODES.to_vec())?;
    Ok(())
}
