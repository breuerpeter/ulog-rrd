//! ulog-rrd for Python: the `_ulog_rrd` extension the `ulog_rrd` package wraps.

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use ulog_rrd::InfoValue;

/// The recording of a ULog as `.rrd` bytes. See `ulog_rrd.convert` for the arguments.
#[pyfunction]
#[pyo3(signature = (ulog, *, application_id, recording_id=None, entity_path_prefix=None, timeline="time", time_offset_us=0, scalars=false, translation=None, rotation=None))]
#[allow(clippy::too_many_arguments)]
fn convert<'py>(
    py: Python<'py>,
    ulog: &[u8],
    application_id: String,
    recording_id: Option<String>,
    entity_path_prefix: Option<String>,
    timeline: &str,
    time_offset_us: i64,
    scalars: bool,
    translation: Option<[f32; 3]>,
    rotation: Option<[f32; 4]>,
) -> PyResult<Bound<'py, PyBytes>> {
    let opts = ulog_rrd::Options {
        application_id,
        recording_id,
        entity_path_prefix,
        timeline: timeline.to_owned(),
        time_offset_us,
        scalars,
        translation,
        rotation,
    };
    let rrd = py.detach(|| ulog_rrd::convert(ulog, &opts)).map_err(|e| pyo3::exceptions::PyValueError::new_err(e.0))?;
    Ok(PyBytes::new(py, &rrd))
}

/// The recording at `rrd` with the ULog added. See `ulog_rrd.append` for the arguments.
#[pyfunction]
#[pyo3(signature = (ulog, rrd, *, application_id, recording_id, entity_path_prefix=None, timeline="time", time_offset_us=0, scalars=false, translation=None, rotation=None))]
#[allow(clippy::too_many_arguments)]
fn append(
    py: Python<'_>,
    ulog: &[u8],
    rrd: std::path::PathBuf,
    application_id: String,
    recording_id: String,
    entity_path_prefix: Option<String>,
    timeline: &str,
    time_offset_us: i64,
    scalars: bool,
    translation: Option<[f32; 3]>,
    rotation: Option<[f32; 4]>,
) -> PyResult<()> {
    let opts = ulog_rrd::Options {
        application_id,
        recording_id: Some(recording_id),
        entity_path_prefix,
        timeline: timeline.to_owned(),
        time_offset_us,
        scalars,
        translation,
        rotation,
    };
    py.detach(|| ulog_rrd::append(ulog, &opts, &rrd)).map_err(|e| pyo3::exceptions::PyValueError::new_err(e.0))
}

/// One topic instance's numeric fields as time series.
#[pyclass(module = "ulog_rrd", frozen)]
struct Topic(ulog_rrd::Topic);

#[pymethods]
impl Topic {
    #[getter]
    fn name(&self) -> &str {
        &self.0.name
    }

    /// The ULog multi id: 0 for the first instance.
    #[getter]
    fn instance(&self) -> u8 {
        self.0.instance
    }

    /// Each sample's time on the recording's timeline, in microseconds.
    #[getter]
    fn timestamps_us(&self) -> Vec<i64> {
        self.0.timestamps_us.clone()
    }

    /// The fields in definition order, each with one value per sample.
    #[getter]
    fn columns<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let columns = PyDict::new(py);
        for (name, values) in &self.0.columns {
            columns.set_item(name, values)?;
        }
        Ok(columns)
    }

    fn __repr__(&self) -> String {
        format!("Topic({:?}, {}, {} samples)", self.0.name, self.0.instance, self.0.timestamps_us.len())
    }
}

/// A text message PX4 logged.
#[pyclass(module = "ulog_rrd", frozen)]
struct Message(ulog_rrd::Message);

#[pymethods]
impl Message {
    #[getter]
    fn timestamp_us(&self) -> i64 {
        self.0.timestamp_us
    }

    /// The syslog severity, 0 (emergency) to 7 (debug).
    #[getter]
    fn level(&self) -> u8 {
        self.0.level
    }

    #[getter]
    fn text(&self) -> &str {
        &self.0.text
    }

    fn __repr__(&self) -> String {
        format!("Message({}, {}, {:?})", self.0.timestamp_us, self.0.level, self.0.text)
    }
}

/// One PX4 log in a recording, under the prefix it was logged with.
#[pyclass(module = "ulog_rrd", frozen)]
struct Log(ulog_rrd::Log);

#[pymethods]
impl Log {
    /// The recording the log sits in.
    #[getter]
    fn recording_id(&self) -> &str {
        &self.0.recording_id
    }

    /// The entity path prefix the log sits under; `None` at the root.
    #[getter]
    fn prefix(&self) -> Option<&str> {
        self.0.prefix.as_deref()
    }

    /// By name, then instance.
    #[getter]
    fn topics(&self) -> Vec<Topic> {
        self.0.topics.iter().cloned().map(Topic).collect()
    }

    #[getter]
    fn messages(&self) -> Vec<Message> {
        self.0.messages.iter().cloned().map(Message).collect()
    }

    /// The info messages by key, each key's values in log order, in the type the ULog gave them.
    #[getter]
    fn info<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let info = PyDict::new(py);
        for (key, values) in &self.0.info {
            let values: Vec<Bound<'py, PyAny>> = values.iter().map(|v| info_value(py, v)).collect::<PyResult<_>>()?;
            info.set_item(key, values)?;
        }
        Ok(info)
    }

    fn __repr__(&self) -> String {
        format!("Log({:?}, {} topics)", self.0.prefix, self.0.topics.len())
    }
}

fn info_value<'py>(py: Python<'py>, value: &InfoValue) -> PyResult<Bound<'py, PyAny>> {
    Ok(match value {
        InfoValue::Text(v) => v.into_pyobject(py)?.into_any(),
        InfoValue::Bytes(v) => PyBytes::new(py, v).into_any(),
        InfoValue::Bool(v) => v.into_pyobject(py)?.to_owned().into_any(),
        InfoValue::U8(v) => v.into_pyobject(py)?.into_any(),
        InfoValue::U16(v) => v.into_pyobject(py)?.into_any(),
        InfoValue::U32(v) => v.into_pyobject(py)?.into_any(),
        InfoValue::U64(v) => v.into_pyobject(py)?.into_any(),
        InfoValue::I8(v) => v.into_pyobject(py)?.into_any(),
        InfoValue::I16(v) => v.into_pyobject(py)?.into_any(),
        InfoValue::I32(v) => v.into_pyobject(py)?.into_any(),
        InfoValue::I64(v) => v.into_pyobject(py)?.into_any(),
        InfoValue::F32(v) => v.into_pyobject(py)?.into_any(),
        InfoValue::F64(v) => v.into_pyobject(py)?.into_any(),
    })
}

/// The PX4 logs in a recording's `.rrd` bytes. See `ulog_rrd.read`.
#[pyfunction]
fn read(py: Python<'_>, rrd: &[u8]) -> PyResult<Vec<Log>> {
    let logs = py.detach(|| ulog_rrd::read(rrd)).map_err(|e| pyo3::exceptions::PyValueError::new_err(e.0))?;
    Ok(logs.into_iter().map(Log).collect())
}

#[pymodule]
fn _ulog_rrd(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", ulog_rrd::VERSION)?;
    m.add("RERUN_VERSION", ulog_rrd::RERUN_VERSION)?;
    m.add_function(wrap_pyfunction!(convert, m)?)?;
    m.add_function(wrap_pyfunction!(append, m)?)?;
    m.add_function(wrap_pyfunction!(read, m)?)?;
    m.add_class::<Log>()?;
    m.add_class::<Topic>()?;
    m.add_class::<Message>()
}
