//! The environment as a layer: each flag read from the variable its long name gives, under the program's prefix
//! (ADR-0028).

use super::{ENVIRONMENT, NativeError, ProgramName};
use aethereal_data::config::{FlagTable, Overlay};
use std::ffi::OsString;

/// The layer `variables` set: each variable with the program's prefix read as its flag reads a value, refusing one
/// that names no flag.
pub(super) fn read(
    name: &ProgramName,
    table: &FlagTable,
    variables: &[(OsString, OsString)],
) -> Result<Overlay, NativeError> {
    let mut overlay = Overlay::new(ENVIRONMENT);
    for (variable, value) in variables {
        let lossy = variable.to_string_lossy();
        let carried = name.carries(&lossy);
        if !carried {
            continue;
        }

        let flag = table.flags().iter().find(|flag| name.variable(flag) == lossy);
        let Some(flag) = flag else {
            return Err(NativeError::UnknownVariable { variable: lossy.into_owned() });
        };
        let Some(text) = value.to_str() else {
            return Err(NativeError::NotUnicode { variable: lossy.into_owned() });
        };
        overlay.read(flag, text)?;
    }

    Ok(overlay)
}
