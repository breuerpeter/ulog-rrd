//! ulog-rrd for Python: the `_ulog_rrd` extension the `ulog_rrd` package wraps.

use pyo3::prelude::*;
use pyo3::types::PyBytes;

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

#[pymodule]
fn _ulog_rrd(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", ulog_rrd::VERSION)?;
    m.add("RERUN_VERSION", ulog_rrd::RERUN_VERSION)?;
    m.add_function(wrap_pyfunction!(convert, m)?)
}
