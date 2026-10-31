//! Emitters for witforge output formats.
//!
//! Emitters consume the owned [`crate::model::Model`], never `wit-parser` types.

pub mod glue;
pub mod output;
pub mod proto;
pub mod rust;
mod versions;
