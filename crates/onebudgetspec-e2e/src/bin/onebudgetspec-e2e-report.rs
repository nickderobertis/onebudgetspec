//! A measurement written in Rust: `onebudgetspec-e2e-report VALUE [DETAIL]` reports VALUE,
//! and DETAIL when given, through `onebudgetspec_core::report`, the way a consumer's own
//! command would.
//!
//! It prints what `report` returned, `true` or `false`. A refused value exits 2 and a
//! failed write exits 1, each with the error on stderr.

use std::io::ErrorKind;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(value) = args.next().and_then(|value| value.parse::<f64>().ok()) else {
        eprintln!("usage: onebudgetspec-e2e-report VALUE [DETAIL], VALUE a number");
        return ExitCode::from(64);
    };
    let detail = args.next();
    match onebudgetspec_core::report(value, detail.as_deref()) {
        Ok(written) => {
            println!("{written}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("onebudgetspec-e2e-report: {error}");
            ExitCode::from(if error.kind() == ErrorKind::InvalidInput {
                2
            } else {
                1
            })
        }
    }
}
