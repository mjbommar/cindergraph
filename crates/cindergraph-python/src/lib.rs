#![forbid(unsafe_code)]

use pyo3::prelude::*;

mod source_cfg;
mod source_metrics;

#[pymodule]
fn _native(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    // The crate version is the one source of truth for the package version
    // (`version.workspace = true` here, `dynamic = ["version"]` in
    // pyproject.toml), so the extension carries it and Python reads it back.
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    source_cfg::register_source_cfg_bindings(py, module)?;
    source_metrics::register_source_metrics_bindings(py, module)?;
    Ok(())
}
