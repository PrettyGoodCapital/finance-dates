//! pyo3 bindings for finance-dates.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDateTime, PyTzInfo};

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};

use ::finance_dates::{
    calendar_for_exchange, calendar_for_region, business_day_range as core_business_day_range,
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

    let nd = NaiveDate::from_ymd_opt(y, mo, d)
        .ok_or_else(|| PyValueError::new_err("invalid date"))?;
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

fn utc_to_pydatetime<'py>(py: Python<'py>, when: DateTime<Utc>) -> PyResult<Bound<'py, PyDateTime>> {
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

/// Inclusive Mon–Fri business-day range with no holiday calendar.
#[pyfunction]
#[pyo3(signature = (start, end))]
fn business_day_range<'py>(
    py: Python<'py>,
    start: &Bound<'py, PyDate>,
    end: &Bound<'py, PyDate>,
) -> PyResult<Vec<Bound<'py, PyDate>>> {
    let s = pydate_to_naive(start)?;
    let e = pydate_to_naive(end)?;
    core_business_day_range(s, e, &STANDARD_WEEKMASK, &Default::default())
        .into_iter()
        .map(|d| naive_to_pydate(py, d))
        .collect()
}

/// Holiday calendar for an exchange or region.
#[pyclass(module = "finance_dates", name = "Calendar")]
struct PyCalendar {
    inner: ::finance_dates::Calendar,
}

#[pymethods]
impl PyCalendar {
    #[classmethod]
    fn for_exchange(_cls: &Bound<'_, pyo3::types::PyType>, code: &str) -> PyResult<Self> {
        calendar_for_exchange(code)
            .map(|inner| Self { inner })
            .ok_or_else(|| PyValueError::new_err(format!("unknown exchange code: {code}")))
    }

    #[classmethod]
    fn for_region(_cls: &Bound<'_, pyo3::types::PyType>, code: &str) -> PyResult<Self> {
        calendar_for_region(code)
            .map(|inner| Self { inner })
            .ok_or_else(|| PyValueError::new_err(format!("unknown region code: {code}")))
    }

    #[getter]
    fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    fn weekmask(&self) -> Vec<bool> {
        self.inner.weekmask.to_vec()
    }

    fn is_business_day(&self, d: &Bound<'_, PyDate>) -> PyResult<bool> {
        Ok(self.inner.is_business_day(pydate_to_naive(d)?))
    }

    fn is_holiday(&self, d: &Bound<'_, PyDate>) -> PyResult<bool> {
        Ok(self.inner.is_holiday(pydate_to_naive(d)?))
    }

    fn next_business_day<'py>(
        &self,
        py: Python<'py>,
        d: &Bound<'py, PyDate>,
    ) -> PyResult<Bound<'py, PyDate>> {
        naive_to_pydate(py, self.inner.next_business_day(pydate_to_naive(d)?))
    }

    fn previous_business_day<'py>(
        &self,
        py: Python<'py>,
        d: &Bound<'py, PyDate>,
    ) -> PyResult<Bound<'py, PyDate>> {
        naive_to_pydate(py, self.inner.previous_business_day(pydate_to_naive(d)?))
    }

    fn business_days_between(
        &self,
        start: &Bound<'_, PyDate>,
        end: &Bound<'_, PyDate>,
    ) -> PyResult<i64> {
        Ok(self.inner.business_days_between(pydate_to_naive(start)?, pydate_to_naive(end)?))
    }

    fn business_day_range<'py>(
        &self,
        py: Python<'py>,
        start: &Bound<'py, PyDate>,
        end: &Bound<'py, PyDate>,
    ) -> PyResult<Vec<Bound<'py, PyDate>>> {
        let s = pydate_to_naive(start)?;
        let e = pydate_to_naive(end)?;
        self.inner
            .business_day_range(s, e)
            .into_iter()
            .map(|d| naive_to_pydate(py, d))
            .collect()
    }

    fn holidays<'py>(&self, py: Python<'py>, year: i32) -> PyResult<Vec<Bound<'py, PyDate>>> {
        self.inner
            .holidays(year)
            .iter()
            .map(|d| naive_to_pydate(py, *d))
            .collect()
    }

    fn is_open(&self, when: &Bound<'_, PyDateTime>) -> PyResult<bool> {
        Ok(self.inner.is_open(pydatetime_to_utc(when)?))
    }

    fn next_open<'py>(
        &self,
        py: Python<'py>,
        when: &Bound<'py, PyDateTime>,
    ) -> PyResult<Option<Bound<'py, PyDateTime>>> {
        let w = pydatetime_to_utc(when)?;
        match self.inner.next_open(w) {
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
        match self.inner.next_close(w) {
            Some(t) => Ok(Some(utc_to_pydatetime(py, t)?)),
            None => Ok(None),
        }
    }

    fn __repr__(&self) -> String {
        format!("Calendar({})", self.inner.name)
    }
}

#[pymodule]
fn finance_dates(_py: Python, m: &Bound<PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(date_range, m)?)?;
    m.add_function(wrap_pyfunction!(business_day_range, m)?)?;
    m.add_class::<PyCalendar>()?;
    m.add("EXCHANGE_CODES", EXCHANGE_CODES.to_vec())?;
    m.add("REGION_CODES", REGION_CODES.to_vec())?;
    Ok(())
}
