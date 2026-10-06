//! Writing a `reported` budget's result from inside the command that measures it.

use std::io;

use serde_json::{Map, Value};

use crate::model::RESULT_ENV;

/// Report `value`, and `detail` when given, as the result of the `reported` budget whose
/// command is running this process.
///
/// When `ONEBUDGETSPEC_RESULT` is set and non-empty, the file it names is replaced by one
/// JSON object, `{"value": value}` with `"detail": detail` when a detail is given, and this
/// returns `true`. When it is unset or empty, nothing is written and this returns `false`,
/// so a test that measures behaves the same outside a check. Nothing here reads a budgets
/// file or compares the value with a threshold: `onebudgetspec check` is the only judge.
///
/// ```no_run
/// let reported = onebudgetspec_core::report(1395.0, Some("p95 of 200 requests"))?;
/// # Ok::<(), std::io::Error>(())
/// ```
///
/// # Errors
///
/// An error of kind [`io::ErrorKind::InvalidInput`] when `value` is not finite, having
/// written nothing; otherwise the error writing the file.
pub fn report(value: f64, detail: Option<&str>) -> io::Result<bool> {
    if !value.is_finite() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("a reported value must be a finite number, not {value}"),
        ));
    }
    let Some(path) = std::env::var_os(RESULT_ENV).filter(|path| !path.is_empty()) else {
        return Ok(false);
    };
    let mut result = Map::new();
    result.insert("value".to_owned(), Value::from(value));
    if let Some(detail) = detail {
        result.insert("detail".to_owned(), Value::from(detail));
    }
    std::fs::write(path, Value::Object(result).to_string())?;
    Ok(true)
}
