use pyo3::prelude::*;

/// MATBOX Python module — evaluate MATLAB code from Python.
#[pyclass]
struct Engine {
    inner: matbox_core::Engine,
}

#[pymethods]
impl Engine {
    #[new]
    fn new() -> Self {
        Engine { inner: matbox_core::Engine::new() }
    }

    fn eval(&mut self, code: &str) -> PyResult<String> {
        match self.inner.eval(code) {
            Ok(Some(v)) => Ok(matbox_core::builtins::format_value(&v)),
            Ok(None) => Ok(String::new()),
            Err(e) => Err(pyo3::exceptions::PyRuntimeError::new_err(format!("{}", e))),
        }
    }
}

#[pymodule]
fn matbox(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Engine>()?;
    Ok(())
}
