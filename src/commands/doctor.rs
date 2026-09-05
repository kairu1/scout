//! `scout doctor`: print the report on stdout so it can be piped into a
//! bug report; log to stderr like every other CLI path.

use super::logging;
use crate::Error;

pub fn doctor(format: &str) -> crate::Result<u8> {
    if !matches!(format, "human" | "tsv") {
        return Err(Error::UnknownFormat { given: format.to_string(), wanted: "human|tsv" });
    }
    logging::init(logging::Sink::Stderr);
    let report = crate::doctor::report();
    match format {
        "tsv" => print!("{}", report.render_tsv()),
        _ => print!("{}", report.render()),
    }
    Ok(report.exit_code())
}
