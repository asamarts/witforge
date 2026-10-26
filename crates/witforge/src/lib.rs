//! witforge: a WIT-first contract compiler.
//!
//! A WIT package is the single source of truth for a contract. witforge checks that the package
//! stays inside a restricted, pure subset of WIT that maps cleanly onto every target, keeps a
//! committed, append-only lockfile of wire numbers so the protobuf projection stays stable across
//! versions, and (in later milestones) emits plain Rust types and traits and a `.proto` schema.
//!
//! This first slice provides [`load`], the [`subset`] checker, and the [`lock`] lockfile.

pub mod lock;
pub mod model;
pub mod subset;

use std::path::Path;

use anyhow::{Context, Result};
use wit_parser::{PackageId, Resolve};

/// A parsed WIT package together with the resolve that owns it.
///
/// The fields are crate-private so that `wit-parser` types do not become part of witforge's
/// public API. Callers use [`load`] and the model/subset/lock modules instead.
#[derive(Debug)]
pub struct Contract {
    pub(crate) resolve: Resolve,
    pub(crate) package: PackageId,
}

/// Load the WIT package in `dir`.
///
/// All features are enabled, so items gated by `@unstable` are kept; otherwise they would
/// silently disappear and the lockfile would report their members as removed.
///
/// # Errors
///
/// Returns an error when the directory cannot be read or the WIT does not parse or resolve.
pub fn load(dir: &Path) -> Result<Contract> {
    let mut resolve = Resolve {
        all_features: true,
        ..Resolve::default()
    };
    let (package, _sources) = resolve
        .push_dir(dir)
        .with_context(|| format!("failed to load the WIT package in {}", dir.display()))?;
    Ok(Contract { resolve, package })
}
