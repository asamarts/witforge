//! Emitters for witforge output formats.
//!
//! Emitters consume the owned [`crate::model::Model`], never `wit-parser` types.

/// Return a [`crate::ContractError`] built from a format string: a contract the emitter cannot
/// represent, such as two WIT names that map to one generated name. Internal errors use plain
/// `bail!`, so the CLI exits with `2` for them rather than reporting a contract violation.
macro_rules! contract_bail {
    ($($arg:tt)*) => {
        return Err(anyhow::Error::new($crate::ContractError::new(format!($($arg)*))))
    };
}

pub mod glue;
pub mod output;
pub mod proto;
pub mod rust;
mod versions;
