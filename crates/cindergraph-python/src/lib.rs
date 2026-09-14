#![forbid(unsafe_code)]

use pyo3::prelude::*;

mod source_cfg;
mod source_metrics;

#[pymodule]
fn _native(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    source_cfg::register_source_cfg_bindings(py, module)?;
    source_metrics::register_source_metrics_bindings(py, module)?;
    Ok(())
}
