//! Versioned JSON contract IR (`version: 1`) and human summary export.
//!
//! This module serializes the owned contract [`Model`] together with resolved `deps/` packages,
//! canonical types, normalized documentation, stability metadata, and `wire.lock` wire numbers,
//! reservations, and declaration-order fingerprints into a deterministic, schema-versioned JSON
//! intermediate representation (`IR_VERSION = 1`). External emitters, documentation generators,
//! and non-Rust linters can consume this IR without re-parsing WIT or `wire.lock`.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::diagnostic::{Diagnostic, LocatedDiagnostic, codes};
use crate::emit::docs;
use crate::emit::versions::load_versions_with_released;
use crate::lock::{PackageName, WireLock};
use crate::model::{
    Dependency, Function, Interface, Model, Package, Stability, TypeDef, TypeDefKind, TypeExpr,
    World, WorldInterface,
};
use crate::{
    ContractError, ensure_directory_version, ensure_version_directory, load,
    load_root_released_manifest, model, operational_error, operational_error_at,
    parse_version_directory,
};

/// Current schema version of the JSON contract IR emitted by this module.
pub const IR_VERSION: u32 = 1;

/// Emit the versioned JSON contract IR (`version: 1`) for one validated [`Model`] and optional
/// [`WireLock`].
///
/// The output is deterministic, 2-space indented, LF-only, and ends with a single trailing
/// newline.
///
/// # Errors
///
/// Returns an error if a canonical type cannot be resolved against `model`.
pub fn emit_version_json(model: &Model, lock: Option<&WireLock>) -> Result<String> {
    let mut out = String::new();
    writeln!(out, "{{")?;
    writeln!(out, "  \"version\": {IR_VERSION},")?;
    writeln!(out, "  \"kind\": \"version\",")?;
    write!(out, "  \"contract\": ")?;
    write_contract(&mut out, model, lock, 2)?;
    writeln!(out)?;
    writeln!(out, "}}")?;
    Ok(out)
}

/// Load and validate a single WIT version directory and emit its versioned JSON contract IR
/// (`version: 1`).
///
/// If `<version_dir>/wire.lock` exists, it is parsed and included in the IR; otherwise `lockfile`
/// is `null` and member `wire_number` fields are `null`.
///
/// # Errors
///
/// Returns a [`crate::ContractError`] for contract or subset violations, or a plain error for
/// I/O, parse, or symlink-guard failures.
pub fn inspect_version_json(version_dir: &Path) -> Result<String> {
    let (model, lock) = load_single_version(version_dir)?;
    emit_version_json(&model, lock.as_ref())
}

/// Load and validate every live version under `wit_root` (honoring an optional explicit
/// `--released` manifest path) and emit a versioned JSON contract IR (`version: 1`) containing
/// every live version and the list of retired version directories.
///
/// # Errors
///
/// Returns a [`crate::ContractError`] for contract, lockfile, or retirement-order violations, or a
/// plain error for I/O or manifest failures.
pub fn inspect_root_json(wit_root: &Path, released: Option<&Path>) -> Result<String> {
    let versions = load_versions_with_released(wit_root, released)?;
    let manifest = load_root_released_manifest(wit_root, released)?;
    let retired = sorted_retired_directories(&manifest);

    let mut out = String::new();
    writeln!(out, "{{")?;
    writeln!(out, "  \"version\": {IR_VERSION},")?;
    writeln!(out, "  \"kind\": \"root\",")?;
    write!(out, "  \"retired\": [")?;
    for (idx, name) in retired.iter().enumerate() {
        if idx > 0 {
            write!(out, ", ")?;
        }
        write!(out, "{}", json_string(name))?;
    }
    writeln!(out, "],")?;
    writeln!(out, "  \"versions\": [")?;
    for (idx, version) in versions.iter().enumerate() {
        let dir_name = version
            .directory
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default();
        let is_released = manifest.contains(dir_name);
        writeln!(out, "    {{")?;
        writeln!(out, "      \"directory\": {},", json_string(dir_name))?;
        writeln!(out, "      \"major\": {},", version.major)?;
        writeln!(
            out,
            "      \"minor\": {},",
            version.model.package.version.minor
        )?;
        writeln!(out, "      \"released\": {is_released},")?;
        write!(out, "      \"contract\": ")?;
        write_contract(&mut out, &version.model, Some(&version.lock), 6)?;
        writeln!(out)?;
        if idx + 1 < versions.len() {
            writeln!(out, "    }},")?;
        } else {
            writeln!(out, "    }}")?;
        }
    }
    writeln!(out, "  ]")?;
    writeln!(out, "}}")?;
    Ok(out)
}

/// Format a concise human-readable summary for a single WIT version directory.
///
/// # Errors
///
/// Returns the same errors as [`inspect_version_json`].
pub fn summarize_version(version_dir: &Path) -> Result<String> {
    let (model, lock) = load_single_version(version_dir)?;
    let mut out = String::new();
    write_version_summary(&mut out, None, None, &model, lock.as_ref())?;
    Ok(out)
}

/// Format a concise human-readable summary for every live version under `wit_root`.
///
/// # Errors
///
/// Returns the same errors as [`inspect_root_json`].
pub fn summarize_root(wit_root: &Path, released: Option<&Path>) -> Result<String> {
    let versions = load_versions_with_released(wit_root, released)?;
    let manifest = load_root_released_manifest(wit_root, released)?;
    let retired = sorted_retired_directories(&manifest);
    let mut out = String::new();
    writeln!(
        out,
        "root: {} ({} live version(s), {} retired)",
        wit_root.display(),
        versions.len(),
        retired.len()
    )?;
    for version in &versions {
        let dir_name = version
            .directory
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default();
        let status = if manifest.contains(dir_name) {
            "released"
        } else {
            "draft"
        };
        writeln!(out)?;
        write_version_summary(
            &mut out,
            Some(dir_name),
            Some(status),
            &version.model,
            Some(&version.lock),
        )?;
    }
    Ok(out)
}

fn sorted_retired_directories(manifest: &crate::ReleasedManifest) -> Vec<&str> {
    let mut retired = manifest.retired_directories();
    retired.sort_by(|a, b| {
        let ka = parse_version_directory(a).ok().flatten();
        let kb = parse_version_directory(b).ok().flatten();
        ka.cmp(&kb).then_with(|| a.cmp(b))
    });
    retired
}

#[allow(clippy::too_many_lines)]
fn load_single_version(version_dir: &Path) -> Result<(Model, Option<WireLock>)> {
    ensure_version_directory(version_dir)?;
    let contract = load(version_dir)?;
    let built = model::build(&contract)?;
    ensure_directory_version(version_dir, &built)?;
    let lock_path = version_dir.join("wire.lock");
    let lock_file = lock_path.display().to_string();
    let lock = match fs::symlink_metadata(&lock_path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(operational_error(
                codes::IO_ERROR,
                format!("failed to read lockfile {lock_file}: {err}"),
                Some(lock_file),
            ));
        }
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(operational_error(
                codes::GUARD_REFUSED,
                format!("refusing to follow symlinked lockfile {lock_file}"),
                Some(lock_file),
            ));
        }
        Ok(meta) if !meta.is_file() => {
            return Err(operational_error(
                codes::IO_ERROR,
                format!("lockfile {lock_file} is not a regular file"),
                Some(lock_file),
            ));
        }
        Ok(_) => {
            let text = fs::read_to_string(&lock_path)
                .with_context(|| format!("failed to read lockfile {lock_file}"))?;
            let parsed = WireLock::parse(&text).map_err(|err| {
                let position = crate::error_diagnostics(&err)
                    .and_then(|diags| diags.first())
                    .and_then(|diag| diag.position);
                operational_error_at(
                    codes::PARSE_ERROR,
                    format!("in {lock_file}: {err:#}"),
                    lock_file.clone(),
                    position,
                )
            })?;
            let lock_positions = WireLock::entry_positions(&text);
            let pkg = PackageName::new(built.package.namespace.clone(), built.package.name.clone());
            parsed.check_package(&pkg).map_err(|err| {
                let human = format!("in {lock_file}: {err:#}");
                let foreign_pos = parsed
                    .entries()
                    .find(|(key, _)| key.package() != &pkg)
                    .and_then(|(key, _)| lock_positions.get(&key.key()).copied());
                let mut diag = Diagnostic::error(codes::LOCKFILE_WRONG_PACKAGE, format!("{err:#}"))
                    .with_path(lock_file.clone());
                if let Some(pos) = foreign_pos {
                    diag = diag.with_position(pos);
                }
                let located = LocatedDiagnostic::from_diagnostic(diag).with_file(lock_file.clone());
                ContractError::with_located_diagnostics(human, vec![located])
            })?;
            let members = built.members()?;
            for member in &members {
                let key = member.key();
                let Some(entry) = parsed.get(&key) else {
                    continue;
                };
                if entry.reserved {
                    let mut diag = Diagnostic::error(
                        codes::LOCKFILE_REINTRODUCED,
                        format!(
                            "member `{key}` was previously removed and reserved as number {} in {lock_file}",
                            entry.number
                        ),
                    )
                    .with_path(key.clone());
                    if let Some(pos) = lock_positions.get(&key).copied() {
                        diag = diag.with_position(pos);
                    }
                    return Err(ContractError::with_located_diagnostic(
                        LocatedDiagnostic::from_diagnostic(diag).with_file(lock_file),
                    )
                    .into());
                }
                if entry.kind != member.kind || entry.ty != member.ty {
                    let mut diag = Diagnostic::error(
                        codes::LOCKFILE_CHANGED_DRAFT,
                        format!(
                            "member `{key}` in {} disagrees with {lock_file} (lockfile has kind `{}` type `{:?}`, WIT has kind `{}` type `{:?}`)",
                            version_dir.display(),
                            entry.kind,
                            entry.ty,
                            member.kind,
                            member.ty,
                        ),
                    )
                    .with_path(key.clone());
                    if let Some(pos) = lock_positions.get(&key).copied() {
                        diag = diag.with_position(pos);
                    }
                    return Err(ContractError::with_located_diagnostic(
                        LocatedDiagnostic::from_diagnostic(diag).with_file(lock_file),
                    )
                    .into());
                }
            }
            Some(parsed)
        }
    };
    Ok((built, lock))
}

fn write_version_summary(
    out: &mut String,
    directory: Option<&str>,
    status: Option<&str>,
    model: &Model,
    lock: Option<&WireLock>,
) -> Result<()> {
    let header_prefix = match (directory, status) {
        (Some(dir), Some(status)) => format!("{dir} [{status}] "),
        (Some(dir), None) => format!("{dir} "),
        _ => String::new(),
    };
    writeln!(
        out,
        "{header_prefix}{}:{}@{}",
        model.package.namespace, model.package.name, model.package.version
    )?;
    let total_types: usize = model.interfaces.iter().map(|i| i.types.len()).sum();
    let total_funcs: usize = model.interfaces.iter().map(|i| i.functions.len()).sum();
    writeln!(
        out,
        "  interfaces: {} ({} type(s), {} function(s)), worlds: {}, dependencies: {}",
        model.interfaces.len(),
        total_types,
        total_funcs,
        model.worlds.len(),
        model.dependencies.len(),
    )?;
    for interface in &model.interfaces {
        writeln!(
            out,
            "  interface {}: {} type(s), {} function(s)",
            interface.name,
            interface.types.len(),
            interface.functions.len()
        )?;
    }
    for world in &model.worlds {
        writeln!(
            out,
            "  world {}: {} import(s), {} export(s)",
            world.name,
            world.imports.len(),
            world.exports.len()
        )?;
    }
    for dep in &model.dependencies {
        writeln!(
            out,
            "  dependency {}:{}@{} ({} interface(s))",
            dep.package.namespace,
            dep.package.name,
            dep.package.version,
            dep.interfaces.len()
        )?;
    }
    if let Some(lock) = lock {
        let mut live = 0usize;
        let mut reserved = 0usize;
        for (_, entry) in lock.entries() {
            if entry.reserved {
                reserved += 1;
            } else {
                live += 1;
            }
        }
        let orders = lock.order_fingerprints().count();
        writeln!(
            out,
            "  lockfile: {live} live, {reserved} reserved, {orders} order fingerprint(s)"
        )?;
    } else {
        writeln!(out, "  lockfile: none")?;
    }
    Ok(())
}

fn write_contract(
    out: &mut String,
    model: &Model,
    lock: Option<&WireLock>,
    indent: usize,
) -> Result<()> {
    let pad = " ".repeat(indent);
    let inner = " ".repeat(indent + 2);
    writeln!(out, "{{")?;
    write!(out, "{inner}\"package\": ")?;
    write_package(out, &model.package, indent + 2)?;
    writeln!(out, ",")?;

    write!(out, "{inner}\"interfaces\": ")?;
    write_interfaces(
        out,
        model,
        &model.package,
        &model.interfaces,
        lock,
        indent + 2,
    )?;
    writeln!(out, ",")?;

    write!(out, "{inner}\"worlds\": ")?;
    write_worlds(out, &model.worlds, indent + 2)?;
    writeln!(out, ",")?;

    write!(out, "{inner}\"dependencies\": ")?;
    write_dependencies(out, model, &model.dependencies, indent + 2)?;
    writeln!(out, ",")?;

    write!(out, "{inner}\"lockfile\": ")?;
    match lock {
        Some(lock) => write_lockfile(out, lock, indent + 2)?,
        None => write!(out, "null")?,
    }
    writeln!(out)?;
    write!(out, "{pad}}}")?;
    Ok(())
}

fn write_package(out: &mut String, package: &Package, indent: usize) -> Result<()> {
    let pad = " ".repeat(indent);
    let inner = " ".repeat(indent + 2);
    let doc = docs::normalize_plain(&package.docs);
    writeln!(out, "{{")?;
    writeln!(
        out,
        "{inner}\"namespace\": {},",
        json_string(&package.namespace)
    )?;
    writeln!(out, "{inner}\"name\": {},", json_string(&package.name))?;
    writeln!(
        out,
        "{inner}\"version\": {},",
        json_string(&package.version.to_string())
    )?;
    writeln!(out, "{inner}\"docs\": {}", json_string(&doc))?;
    write!(out, "{pad}}}")?;
    Ok(())
}

fn write_interfaces(
    out: &mut String,
    model: &Model,
    owner_package: &Package,
    interfaces: &[Interface],
    lock: Option<&WireLock>,
    indent: usize,
) -> Result<()> {
    if interfaces.is_empty() {
        write!(out, "[]")?;
        return Ok(());
    }
    let pad = " ".repeat(indent);
    let item_pad = " ".repeat(indent + 2);
    let inner = " ".repeat(indent + 4);
    writeln!(out, "[")?;
    for (idx, interface) in interfaces.iter().enumerate() {
        writeln!(out, "{item_pad}{{")?;
        writeln!(out, "{inner}\"name\": {},", json_string(&interface.name))?;
        writeln!(
            out,
            "{inner}\"docs\": {},",
            json_string(&docs::normalize_plain(&interface.docs))
        )?;
        write!(out, "{inner}\"stability\": ")?;
        write_stability(out, &interface.stability)?;
        writeln!(out, ",")?;

        write!(out, "{inner}\"types\": ")?;
        write_types(
            out,
            model,
            owner_package,
            &interface.name,
            &interface.types,
            lock,
            indent + 4,
        )?;
        writeln!(out, ",")?;

        write!(out, "{inner}\"functions\": ")?;
        write_functions(
            out,
            model,
            owner_package,
            &interface.name,
            &interface.functions,
            lock,
            indent + 4,
        )?;
        writeln!(out)?;
        if idx + 1 < interfaces.len() {
            writeln!(out, "{item_pad}}},")?;
        } else {
            writeln!(out, "{item_pad}}}")?;
        }
    }
    write!(out, "{pad}]")?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn write_types(
    out: &mut String,
    model: &Model,
    owner_package: &Package,
    interface_name: &str,
    types: &[TypeDef],
    lock: Option<&WireLock>,
    indent: usize,
) -> Result<()> {
    if types.is_empty() {
        write!(out, "[]")?;
        return Ok(());
    }
    let pad = " ".repeat(indent);
    let item_pad = " ".repeat(indent + 2);
    let inner = " ".repeat(indent + 4);
    let pkg_prefix = format!("{}:{}", owner_package.namespace, owner_package.name);
    writeln!(out, "[")?;
    for (idx, def) in types.iter().enumerate() {
        writeln!(out, "{item_pad}{{")?;
        writeln!(out, "{inner}\"name\": {},", json_string(&def.name))?;
        writeln!(
            out,
            "{inner}\"docs\": {},",
            json_string(&docs::normalize_plain(&def.docs))
        )?;
        write!(out, "{inner}\"stability\": ")?;
        write_stability(out, &def.stability)?;
        writeln!(out, ",")?;
        match &def.kind {
            TypeDefKind::Record(fields) => {
                writeln!(out, "{inner}\"kind\": \"record\",")?;
                write!(out, "{inner}\"fields\": ")?;
                if fields.is_empty() {
                    writeln!(out, "[]")?;
                } else {
                    let elem_pad = " ".repeat(indent + 6);
                    writeln!(out, "[")?;
                    for (f_idx, field) in fields.iter().enumerate() {
                        let key =
                            format!("{pkg_prefix}/{interface_name}/{}.{}", def.name, field.name);
                        let num = live_number(lock, &key);
                        let canonical = model.canonical_type(&field.ty)?;
                        write!(
                            out,
                            "{elem_pad}{{\"name\":{},\"docs\":{},\"type\":",
                            json_string(&field.name),
                            json_string(&docs::normalize_plain(&field.docs))
                        )?;
                        write_type_expr(out, &field.ty)?;
                        write!(
                            out,
                            ",\"canonical_type\":{},\"wire_number\":{}}}",
                            json_string(&canonical),
                            opt_u32(num)
                        )?;
                        if f_idx + 1 < fields.len() {
                            writeln!(out, ",")?;
                        } else {
                            writeln!(out)?;
                        }
                    }
                    writeln!(out, "{inner}]")?;
                }
            }
            TypeDefKind::Variant(cases) => {
                writeln!(out, "{inner}\"kind\": \"variant\",")?;
                write!(out, "{inner}\"cases\": ")?;
                if cases.is_empty() {
                    writeln!(out, "[]")?;
                } else {
                    let elem_pad = " ".repeat(indent + 6);
                    writeln!(out, "[")?;
                    for (c_idx, case) in cases.iter().enumerate() {
                        let key =
                            format!("{pkg_prefix}/{interface_name}/{}.{}", def.name, case.name);
                        let num = live_number(lock, &key);
                        write!(
                            out,
                            "{elem_pad}{{\"name\":{},\"docs\":{},\"payload\":",
                            json_string(&case.name),
                            json_string(&docs::normalize_plain(&case.docs))
                        )?;
                        let canonical = if let Some(payload) = &case.payload {
                            write_type_expr(out, payload)?;
                            Some(model.canonical_type(payload)?)
                        } else {
                            write!(out, "null")?;
                            None
                        };
                        write!(
                            out,
                            ",\"canonical_type\":{},\"wire_number\":{}}}",
                            opt_json_string(canonical.as_deref()),
                            opt_u32(num)
                        )?;
                        if c_idx + 1 < cases.len() {
                            writeln!(out, ",")?;
                        } else {
                            writeln!(out)?;
                        }
                    }
                    writeln!(out, "{inner}]")?;
                }
            }
            TypeDefKind::Enum(cases) => {
                writeln!(out, "{inner}\"kind\": \"enum\",")?;
                write!(out, "{inner}\"cases\": ")?;
                if cases.is_empty() {
                    writeln!(out, "[]")?;
                } else {
                    let elem_pad = " ".repeat(indent + 6);
                    writeln!(out, "[")?;
                    for (c_idx, case) in cases.iter().enumerate() {
                        let key =
                            format!("{pkg_prefix}/{interface_name}/{}.{}", def.name, case.name);
                        let num = live_number(lock, &key);
                        write!(
                            out,
                            "{elem_pad}{{\"name\":{},\"docs\":{},\"wire_number\":{}}}",
                            json_string(&case.name),
                            json_string(&docs::normalize_plain(&case.docs)),
                            opt_u32(num)
                        )?;
                        if c_idx + 1 < cases.len() {
                            writeln!(out, ",")?;
                        } else {
                            writeln!(out)?;
                        }
                    }
                    writeln!(out, "{inner}]")?;
                }
            }
            TypeDefKind::Flags(flags) => {
                writeln!(out, "{inner}\"kind\": \"flags\",")?;
                write!(out, "{inner}\"flags\": ")?;
                if flags.is_empty() {
                    writeln!(out, "[]")?;
                } else {
                    let elem_pad = " ".repeat(indent + 6);
                    writeln!(out, "[")?;
                    for (f_idx, flag) in flags.iter().enumerate() {
                        let key =
                            format!("{pkg_prefix}/{interface_name}/{}.{}", def.name, flag.name);
                        let num = live_number(lock, &key);
                        write!(
                            out,
                            "{elem_pad}{{\"name\":{},\"docs\":{},\"wire_number\":{}}}",
                            json_string(&flag.name),
                            json_string(&docs::normalize_plain(&flag.docs)),
                            opt_u32(num)
                        )?;
                        if f_idx + 1 < flags.len() {
                            writeln!(out, ",")?;
                        } else {
                            writeln!(out)?;
                        }
                    }
                    writeln!(out, "{inner}]")?;
                }
            }
            TypeDefKind::Alias(target) => {
                writeln!(out, "{inner}\"kind\": \"alias\",")?;
                write!(out, "{inner}\"target\": ")?;
                write_type_expr(out, target)?;
                writeln!(out, ",")?;
                writeln!(
                    out,
                    "{inner}\"canonical_type\": {}",
                    json_string(&model.canonical_type(target)?)
                )?;
            }
            TypeDefKind::List(element) => {
                writeln!(out, "{inner}\"kind\": \"list\",")?;
                write!(out, "{inner}\"element\": ")?;
                write_type_expr(out, element)?;
                writeln!(out, ",")?;
                let list_ty = TypeExpr::List(Box::new(element.clone()));
                writeln!(
                    out,
                    "{inner}\"canonical_type\": {}",
                    json_string(&model.canonical_type(&list_ty)?)
                )?;
            }
            TypeDefKind::FixedLengthList(element, length) => {
                writeln!(out, "{inner}\"kind\": \"fixed-length-list\",")?;
                write!(out, "{inner}\"element\": ")?;
                write_type_expr(out, element)?;
                writeln!(out, ",")?;
                writeln!(out, "{inner}\"length\": {length},")?;
                let list_ty = TypeExpr::FixedLengthList(Box::new(element.clone()), *length);
                writeln!(
                    out,
                    "{inner}\"canonical_type\": {}",
                    json_string(&model.canonical_type(&list_ty)?)
                )?;
            }
            TypeDefKind::Option(payload) => {
                writeln!(out, "{inner}\"kind\": \"option\",")?;
                write!(out, "{inner}\"payload\": ")?;
                write_type_expr(out, payload)?;
                writeln!(out, ",")?;
                let opt_ty = TypeExpr::Option(Box::new(payload.clone()));
                writeln!(
                    out,
                    "{inner}\"canonical_type\": {}",
                    json_string(&model.canonical_type(&opt_ty)?)
                )?;
            }
            TypeDefKind::Result(result) => {
                writeln!(out, "{inner}\"kind\": \"result\",")?;
                write!(out, "{inner}\"ok\": ")?;
                match &result.ok {
                    Some(ty) => write_type_expr(out, ty)?,
                    None => write!(out, "null")?,
                }
                writeln!(out, ",")?;
                write!(out, "{inner}\"err\": ")?;
                match &result.err {
                    Some(ty) => write_type_expr(out, ty)?,
                    None => write!(out, "null")?,
                }
                writeln!(out, ",")?;
                let res_ty = TypeExpr::Result(result.clone());
                writeln!(
                    out,
                    "{inner}\"canonical_type\": {}",
                    json_string(&model.canonical_type(&res_ty)?)
                )?;
            }
            TypeDefKind::Map(key, value) => {
                writeln!(out, "{inner}\"kind\": \"map\",")?;
                write!(out, "{inner}\"key\": ")?;
                write_type_expr(out, key)?;
                writeln!(out, ",")?;
                write!(out, "{inner}\"value\": ")?;
                write_type_expr(out, value)?;
                writeln!(out, ",")?;
                let map_ty = TypeExpr::Map(Box::new(key.clone()), Box::new(value.clone()));
                writeln!(
                    out,
                    "{inner}\"canonical_type\": {}",
                    json_string(&model.canonical_type(&map_ty)?)
                )?;
            }
        }
        if idx + 1 < types.len() {
            writeln!(out, "{item_pad}}},")?;
        } else {
            writeln!(out, "{item_pad}}}")?;
        }
    }
    write!(out, "{pad}]")?;
    Ok(())
}

fn write_functions(
    out: &mut String,
    model: &Model,
    owner_package: &Package,
    interface_name: &str,
    functions: &[Function],
    lock: Option<&WireLock>,
    indent: usize,
) -> Result<()> {
    if functions.is_empty() {
        write!(out, "[]")?;
        return Ok(());
    }
    let pad = " ".repeat(indent);
    let item_pad = " ".repeat(indent + 2);
    let inner = " ".repeat(indent + 4);
    let pkg_prefix = format!("{}:{}", owner_package.namespace, owner_package.name);
    writeln!(out, "[")?;
    for (idx, func) in functions.iter().enumerate() {
        let method_key = format!("{pkg_prefix}/{interface_name}.{}", func.name);
        let method_num = live_number(lock, &method_key);
        writeln!(out, "{item_pad}{{")?;
        writeln!(out, "{inner}\"name\": {},", json_string(&func.name))?;
        writeln!(
            out,
            "{inner}\"docs\": {},",
            json_string(&docs::normalize_plain(&func.docs))
        )?;
        write!(out, "{inner}\"stability\": ")?;
        write_stability(out, &func.stability)?;
        writeln!(out, ",")?;
        writeln!(out, "{inner}\"wire_number\": {},", opt_u32(method_num))?;
        write!(out, "{inner}\"params\": ")?;
        if func.params.is_empty() {
            writeln!(out, "[],")?;
        } else {
            let elem_pad = " ".repeat(indent + 6);
            writeln!(out, "[")?;
            for (p_idx, param) in func.params.iter().enumerate() {
                let param_key =
                    format!("{pkg_prefix}/{interface_name}#{}.{}", func.name, param.name);
                let param_num = live_number(lock, &param_key);
                let canonical = model.canonical_type(&param.ty)?;
                write!(
                    out,
                    "{elem_pad}{{\"name\":{},\"type\":",
                    json_string(&param.name)
                )?;
                write_type_expr(out, &param.ty)?;
                write!(
                    out,
                    ",\"canonical_type\":{},\"wire_number\":{}}}",
                    json_string(&canonical),
                    opt_u32(param_num)
                )?;
                if p_idx + 1 < func.params.len() {
                    writeln!(out, ",")?;
                } else {
                    writeln!(out)?;
                }
            }
            writeln!(out, "{inner}],")?;
        }
        write!(out, "{inner}\"result\": ")?;
        let canonical_result = if let Some(ty) = &func.result {
            write_type_expr(out, ty)?;
            Some(model.canonical_type(ty)?)
        } else {
            write!(out, "null")?;
            None
        };
        writeln!(out, ",")?;
        writeln!(
            out,
            "{inner}\"canonical_result\": {}",
            opt_json_string(canonical_result.as_deref())
        )?;
        if idx + 1 < functions.len() {
            writeln!(out, "{item_pad}}},")?;
        } else {
            writeln!(out, "{item_pad}}}")?;
        }
    }
    write!(out, "{pad}]")?;
    Ok(())
}

fn write_worlds(out: &mut String, worlds: &[World], indent: usize) -> Result<()> {
    if worlds.is_empty() {
        write!(out, "[]")?;
        return Ok(());
    }
    let pad = " ".repeat(indent);
    let item_pad = " ".repeat(indent + 2);
    let inner = " ".repeat(indent + 4);
    writeln!(out, "[")?;
    for (idx, world) in worlds.iter().enumerate() {
        writeln!(out, "{item_pad}{{")?;
        writeln!(out, "{inner}\"name\": {},", json_string(&world.name))?;
        writeln!(
            out,
            "{inner}\"docs\": {},",
            json_string(&docs::normalize_plain(&world.docs))
        )?;
        write!(out, "{inner}\"stability\": ")?;
        write_stability(out, &world.stability)?;
        writeln!(out, ",")?;
        write!(out, "{inner}\"imports\": ")?;
        write_world_interfaces(out, &world.imports, indent + 4)?;
        writeln!(out, ",")?;
        write!(out, "{inner}\"exports\": ")?;
        write_world_interfaces(out, &world.exports, indent + 4)?;
        writeln!(out)?;
        if idx + 1 < worlds.len() {
            writeln!(out, "{item_pad}}},")?;
        } else {
            writeln!(out, "{item_pad}}}")?;
        }
    }
    write!(out, "{pad}]")?;
    Ok(())
}

fn write_world_interfaces(out: &mut String, items: &[WorldInterface], indent: usize) -> Result<()> {
    if items.is_empty() {
        write!(out, "[]")?;
        return Ok(());
    }
    let pad = " ".repeat(indent);
    let elem_pad = " ".repeat(indent + 2);
    writeln!(out, "[")?;
    for (idx, item) in items.iter().enumerate() {
        let pkg = item
            .package
            .as_ref()
            .map(|p| format!("{}:{}@{}", p.namespace, p.name, p.version));
        write!(
            out,
            "{elem_pad}{{\"interface\":{},\"package\":{},\"docs\":{},\"stability\":",
            json_string(&item.interface),
            opt_json_string(pkg.as_deref()),
            json_string(&docs::normalize_plain(&item.docs))
        )?;
        write_stability(out, &item.stability)?;
        write!(out, "}}")?;
        if idx + 1 < items.len() {
            writeln!(out, ",")?;
        } else {
            writeln!(out)?;
        }
    }
    write!(out, "{pad}]")?;
    Ok(())
}

fn write_dependencies(
    out: &mut String,
    root_model: &Model,
    dependencies: &[Dependency],
    indent: usize,
) -> Result<()> {
    if dependencies.is_empty() {
        write!(out, "[]")?;
        return Ok(());
    }
    let pad = " ".repeat(indent);
    let item_pad = " ".repeat(indent + 2);
    let inner = " ".repeat(indent + 4);
    writeln!(out, "[")?;
    for (idx, dep) in dependencies.iter().enumerate() {
        let dep_model = Model {
            package: dep.package.clone(),
            interfaces: dep.interfaces.clone(),
            worlds: Vec::new(),
            dependencies: root_model
                .dependencies
                .iter()
                .filter(|other| {
                    other.package.namespace != dep.package.namespace
                        || other.package.name != dep.package.name
                })
                .cloned()
                .collect(),
        };
        let dep_members = dep_model.members()?;
        let mut dep_lock = WireLock::default();
        dep_lock.reconcile(&dep_members)?;
        writeln!(out, "{item_pad}{{")?;
        write!(out, "{inner}\"package\": ")?;
        write_package(out, &dep.package, indent + 4)?;
        writeln!(out, ",")?;
        write!(out, "{inner}\"interfaces\": ")?;
        write_interfaces(
            out,
            &dep_model,
            &dep.package,
            &dep.interfaces,
            Some(&dep_lock),
            indent + 4,
        )?;
        writeln!(out)?;
        if idx + 1 < dependencies.len() {
            writeln!(out, "{item_pad}}},")?;
        } else {
            writeln!(out, "{item_pad}}}")?;
        }
    }
    write!(out, "{pad}]")?;
    Ok(())
}

fn write_lockfile(out: &mut String, lock: &WireLock, indent: usize) -> Result<()> {
    let pad = " ".repeat(indent);
    let inner = " ".repeat(indent + 2);
    let elem_pad = " ".repeat(indent + 4);
    let reserved: Vec<_> = lock.entries().filter(|(_, entry)| entry.reserved).collect();
    let orders: Vec<_> = lock.order_fingerprints().collect();

    writeln!(out, "{{")?;
    write!(out, "{inner}\"reserved\": ")?;
    if reserved.is_empty() {
        writeln!(out, "[],")?;
    } else {
        writeln!(out, "[")?;
        for (idx, (key, entry)) in reserved.iter().enumerate() {
            write!(
                out,
                "{elem_pad}{{\"key\":{},\"number\":{},\"kind\":{},\"type\":{}}}",
                json_string(&key.key()),
                entry.number,
                json_string(&entry.kind.to_string()),
                opt_json_string(entry.ty.as_deref())
            )?;
            if idx + 1 < reserved.len() {
                writeln!(out, ",")?;
            } else {
                writeln!(out)?;
            }
        }
        writeln!(out, "{inner}],")?;
    }

    write!(out, "{inner}\"order\": ")?;
    if orders.is_empty() {
        writeln!(out, "[]")?;
    } else {
        writeln!(out, "[")?;
        for (idx, (parent, fingerprint, members)) in orders.iter().enumerate() {
            let member_list: Vec<String> = members.iter().map(|m| json_string(m)).collect();
            write!(
                out,
                "{elem_pad}{{\"parent\":{},\"fingerprint\":{},\"members\":[{}]}}",
                json_string(parent),
                json_string(&format!("{fingerprint:016x}")),
                member_list.join(",")
            )?;
            if idx + 1 < orders.len() {
                writeln!(out, ",")?;
            } else {
                writeln!(out)?;
            }
        }
        writeln!(out, "{inner}]")?;
    }
    write!(out, "{pad}}}")?;
    Ok(())
}

fn write_stability(out: &mut String, stability: &Stability) -> Result<()> {
    match stability {
        Stability::Unknown => write!(out, "{{\"kind\":\"unknown\"}}")?,
        Stability::Unstable { feature } => {
            write!(
                out,
                "{{\"kind\":\"unstable\",\"feature\":{}}}",
                json_string(feature)
            )?;
        }
        Stability::Stable { since } => {
            write!(
                out,
                "{{\"kind\":\"stable\",\"since\":{}}}",
                json_string(&since.to_string())
            )?;
        }
        Stability::Deprecated {
            since,
            feature,
            deprecated,
            note,
        } => {
            let since_str = since.map(|v| v.to_string());
            write!(
                out,
                "{{\"kind\":\"deprecated\",\"since\":{},\"feature\":{},\"deprecated\":{},\"note\":{}}}",
                opt_json_string(since_str.as_deref()),
                opt_json_string(feature.as_deref()),
                json_string(&deprecated.to_string()),
                json_string(note)
            )?;
        }
    }
    Ok(())
}

fn write_type_expr(out: &mut String, ty: &TypeExpr) -> Result<()> {
    match ty {
        TypeExpr::Bool => write!(out, "{{\"kind\":\"bool\"}}")?,
        TypeExpr::U8 => write!(out, "{{\"kind\":\"u8\"}}")?,
        TypeExpr::U16 => write!(out, "{{\"kind\":\"u16\"}}")?,
        TypeExpr::U32 => write!(out, "{{\"kind\":\"u32\"}}")?,
        TypeExpr::U64 => write!(out, "{{\"kind\":\"u64\"}}")?,
        TypeExpr::S8 => write!(out, "{{\"kind\":\"s8\"}}")?,
        TypeExpr::S16 => write!(out, "{{\"kind\":\"s16\"}}")?,
        TypeExpr::S32 => write!(out, "{{\"kind\":\"s32\"}}")?,
        TypeExpr::S64 => write!(out, "{{\"kind\":\"s64\"}}")?,
        TypeExpr::F32 => write!(out, "{{\"kind\":\"f32\"}}")?,
        TypeExpr::F64 => write!(out, "{{\"kind\":\"f64\"}}")?,
        TypeExpr::Char => write!(out, "{{\"kind\":\"char\"}}")?,
        TypeExpr::String => write!(out, "{{\"kind\":\"string\"}}")?,
        TypeExpr::Named(name) => {
            let pkg = name.package().map(|(ns, pkg)| format!("{ns}:{pkg}"));
            write!(
                out,
                "{{\"kind\":\"named\",\"interface\":{},\"name\":{},\"package\":{}}}",
                json_string(name.interface_name()),
                json_string(&name.name),
                opt_json_string(pkg.as_deref())
            )?;
        }
        TypeExpr::List(element) => {
            write!(out, "{{\"kind\":\"list\",\"element\":")?;
            write_type_expr(out, element)?;
            write!(out, "}}")?;
        }
        TypeExpr::FixedLengthList(element, length) => {
            write!(out, "{{\"kind\":\"fixed-length-list\",\"element\":")?;
            write_type_expr(out, element)?;
            write!(out, ",\"length\":{length}}}")?;
        }
        TypeExpr::Option(payload) => {
            write!(out, "{{\"kind\":\"option\",\"payload\":")?;
            write_type_expr(out, payload)?;
            write!(out, "}}")?;
        }
        TypeExpr::Result(result) => {
            write!(out, "{{\"kind\":\"result\",\"ok\":")?;
            match &result.ok {
                Some(ty) => write_type_expr(out, ty)?,
                None => write!(out, "null")?,
            }
            write!(out, ",\"err\":")?;
            match &result.err {
                Some(ty) => write_type_expr(out, ty)?,
                None => write!(out, "null")?,
            }
            write!(out, "}}")?;
        }
        TypeExpr::Map(key, value) => {
            write!(out, "{{\"kind\":\"map\",\"key\":")?;
            write_type_expr(out, key)?;
            write!(out, ",\"value\":")?;
            write_type_expr(out, value)?;
            write!(out, "}}")?;
        }
    }
    Ok(())
}

fn live_number(lock: Option<&WireLock>, key: &str) -> Option<u32> {
    lock.and_then(|lock| lock.get(key))
        .filter(|entry| !entry.reserved)
        .map(|entry| entry.number)
}

fn opt_u32(value: Option<u32>) -> String {
    value.map_or_else(|| "null".to_string(), |v| v.to_string())
}

fn opt_json_string(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_string(), json_string)
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            ch if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') => {
                let _ = write!(out, "\\u{:04x}", u32::from(ch));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
