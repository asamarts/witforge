//! Deterministic Go 1.22+ plain types and interfaces emitter (`W13-006`).
//!
//! Emits zero-dependency, `gofmt`-formatted Go 1.22+ stdlib packages (`struct`, typed `uint32`
//! enums with `Valid()` and `String()`, sealed variant interfaces with marker methods and case
//! structs, native fixed-length arrays `[N]T`, exact signed/unsigned integer widths, `rune`,
//! generic `Option[T]` / `Result[T, E]` helpers in `witforge_types`, `DepLayout`-aware
//! `witforge_deps/` packages, and `worlds` metadata) directly from the owned [`Model`] per
//! `docs/design/polyglot-emitters.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::diagnostic::codes;
use crate::emit::output::{
    GeneratedFileFinding, GeneratedFiles, ensure_outside_wit_root, is_windows_reserved_stem,
};
use crate::emit::polyglot::{
    self, TargetLanguage, dep_package_segment, escape_go_ident, is_go_keyword, is_go_predeclared,
    to_camel_case, to_pascal_case, to_snake_case,
};
use crate::emit::typescript::dependency_model;
use crate::emit::versions::{DepLayout, Version, load_versions_with_released};
use crate::model::{
    Case, Dependency, Field, Flag, Interface, Model, QualifiedName, ResultExpr, Stability, TypeDef,
    TypeDefKind, TypeExpr, WorldInterface,
};
use crate::operational_error;

/// Options controlling plain Go 1.22+ code generation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct GoOptions {}

impl GoOptions {
    /// Create default Go emission options.
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

pub(crate) fn header_for(namespace: &str, name: &str) -> String {
    TargetLanguage::Go.plain_header(namespace, name)
}

/// The Go version directory name for one contract version: `v<major>_<minor>`.
#[must_use]
pub fn version_dir_name(model: &Model) -> String {
    polyglot::version_dir_name(model.package.version.major, model.package.version.minor)
}

/// Validate a Go module import path prefix (`--go-module` / `go-module`).
///
/// The module path is the Go import prefix under which the emitted package tree is imported (for
/// example `github.com/acme/contracts/gen/plain` or `example.com/app/gen/plain`). Every emitted
/// package appends its directory path (`witforge_types`, `v0_1/<iface>`, `witforge_deps/...`,
/// `worlds`) to this prefix.
///
/// # Errors
///
/// Returns an operational error (`WF9001`) if `module_path` is empty, contains whitespace, quotes,
/// backslashes, semicolons, control characters, leading/trailing `/`, empty segments, `.` or `..`
/// segments, segments starting with `-` or `.` or ending with `.`, Windows reserved device names
/// before the first `.` (`CON`, `PRN`, `AUX`, `NUL`, `COM0`..`9`, `LPT0`..`9`), Windows 8.3
/// short-name suffixes (`~<digits>` before the first `.`), or characters outside Go's module
/// import path alphabet (`[A-Za-z0-9._~+-]`).
pub fn validate_module_path(module_path: &str) -> Result<()> {
    validate_module_path_inner(module_path)
        .map_err(|err| operational_error(codes::CLI_USAGE, format!("{err:#}"), None))
}

fn validate_module_path_inner(path: &str) -> Result<()> {
    if path.is_empty() {
        bail!("Go module path is empty");
    }
    if path
        .bytes()
        .any(|b| !(0x21..=0x7E).contains(&b) || b == b'"' || b == b'\\' || b == b';')
    {
        bail!(
            "invalid Go module path `{path}`: must contain only printable ASCII without \
             whitespace, quotes, backslashes, or `;`"
        );
    }
    if path.starts_with('/') || path.ends_with('/') {
        bail!("invalid Go module path `{path}`: must not start or end with `/`");
    }
    for segment in path.split('/') {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.starts_with('-')
            || segment.starts_with('.')
            || segment.ends_with('.')
            || segment.contains("..")
        {
            bail!("invalid Go module path `{path}`: contains invalid segment `{segment}`");
        }
        if !segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'~' | b'+'))
        {
            bail!(
                "invalid Go module path `{path}`: segment `{segment}` contains unsupported \
                 characters"
            );
        }
        let short = segment.split('.').next().unwrap_or(segment);
        if is_windows_reserved_stem(short) {
            bail!(
                "invalid Go module path `{path}`: segment `{segment}` uses Windows reserved name \
                 `{short}` disallowed by Go's module path rules"
            );
        }
        if let Some((_, suffix)) = short.rsplit_once('~')
            && !suffix.is_empty()
            && suffix.bytes().all(|b| b.is_ascii_digit())
        {
            bail!(
                "invalid Go module path `{path}`: segment `{segment}` contains trailing `~digits` \
                 short-name suffix disallowed by Go's module path rules"
            );
        }
    }
    Ok(())
}

/// Plan the complete nested Go package output for a WIT root without writing anything.
///
/// Every `v<major>.<minor>` directory is loaded, checked against the subset, modelled, and
/// checked against its committed v2 lockfile. All versions must share one package namespace and
/// name.
///
/// # Errors
///
/// Returns an error for an invalid `go_module`, a missing, duplicate, or symlinked version
/// directory, or an unreadable or invalid lockfile, and a [`crate::ContractError`] for an
/// inconsistent package, subset violations, a stale lockfile, or generated-name collisions.
pub fn plan(wit_root: &Path, go_module: &str) -> Result<GeneratedFiles> {
    plan_with_options(wit_root, go_module, None, &GoOptions::new())
}

/// Plan the Go output for `wit_root`, using an optional explicit released-version manifest path
/// (`--released`) when determining retired versions.
///
/// # Errors
///
/// Returns the same errors as [`plan`].
pub fn plan_with_released(
    wit_root: &Path,
    go_module: &str,
    released: Option<&Path>,
) -> Result<GeneratedFiles> {
    plan_with_options(wit_root, go_module, released, &GoOptions::new())
}

/// Plan the Go output for `wit_root` with an optional explicit released-version manifest path and
/// explicit [`GoOptions`].
///
/// # Errors
///
/// Returns the same errors as [`plan`].
pub fn plan_with_options(
    wit_root: &Path,
    go_module: &str,
    released: Option<&Path>,
    options: &GoOptions,
) -> Result<GeneratedFiles> {
    validate_module_path(go_module)?;
    plan_versions_with_options(
        &load_versions_with_released(wit_root, released)?,
        go_module,
        options,
    )
}

/// Generate Go output under `out`.
///
/// # Errors
///
/// Returns an error for an invalid `go_module`, invalid input, an output root inside the WIT tree,
/// a foreign or unowned file collision, or an I/O failure.
pub fn generate(wit_root: &Path, out: &Path, go_module: &str) -> Result<()> {
    generate_with_options(wit_root, out, go_module, &GoOptions::new())
}

/// Generate Go output under `out` with explicit [`GoOptions`].
///
/// # Errors
///
/// Returns the same errors as [`generate`].
pub fn generate_with_options(
    wit_root: &Path,
    out: &Path,
    go_module: &str,
    options: &GoOptions,
) -> Result<()> {
    validate_module_path(go_module)?;
    ensure_outside_wit_root(wit_root, out)?;
    plan_with_options(wit_root, go_module, None, options)?.write(out)
}

/// Check generated Go output under `out` without writing.
///
/// Returns one structured finding per missing, changed, extra, symlinked, or unowned file.
///
/// # Errors
///
/// Returns an error for an invalid `go_module`, invalid input, an output root inside the WIT tree,
/// a foreign-owned file, or an I/O failure.
pub fn check(wit_root: &Path, out: &Path, go_module: &str) -> Result<Vec<GeneratedFileFinding>> {
    check_with_options(wit_root, out, go_module, &GoOptions::new())
}

/// Check generated Go output under `out` with explicit [`GoOptions`].
///
/// # Errors
///
/// Returns the same errors as [`check`].
pub fn check_with_options(
    wit_root: &Path,
    out: &Path,
    go_module: &str,
    options: &GoOptions,
) -> Result<Vec<GeneratedFileFinding>> {
    validate_module_path(go_module)?;
    ensure_outside_wit_root(wit_root, out)?;
    plan_with_options(wit_root, go_module, None, options)?.check(out)
}

/// Emit the per-interface Go files for one single contract version.
///
/// Keys are relative to the version directory (for example `types/types.go` and `api/types.go`).
/// Emitted interface packages import shared `Option`/`Result` helpers from
/// `<go_module>/witforge_types` and pinned dependencies from `<go_module>/witforge_deps/<pkg>`;
/// use [`plan`] (or [`generate`]) for the complete self-contained Go package tree including
/// root `doc.go`, `witforge_types/types.go`, `witforge_deps/`, and `worlds/worlds.go`.
///
/// # Errors
///
/// Returns an operational error (`WF9001`) if `go_module` is invalid, a [`crate::ContractError`]
/// when a WIT name cannot be represented in Go or two generated names collide, and a plain error
/// when a named type is missing from `model`.
pub fn emit_version(model: &Model, go_module: &str) -> Result<BTreeMap<String, String>> {
    emit_version_with_options(model, go_module, &GoOptions::new())
}

/// Emit the per-interface Go files for one single contract version with explicit [`GoOptions`].
///
/// # Errors
///
/// Returns the same errors as [`emit_version`].
pub fn emit_version_with_options(
    model: &Model,
    go_module: &str,
    options: &GoOptions,
) -> Result<BTreeMap<String, String>> {
    validate_module_path(go_module)?;
    let header = header_for(&model.package.namespace, &model.package.name);
    let ver_dir = version_dir_name(model);
    let version_import_prefix = format!("{go_module}/{ver_dir}");
    let shared_types_import = format!("{go_module}/witforge_types");
    let resolver = |ns: &str, pkg: &str| -> Result<String> {
        let seg = safe_dep_pkg_dir(ns, pkg)?;
        Ok(format!("{go_module}/witforge_deps/{seg}"))
    };
    emit_version_files(
        &header,
        model,
        None,
        &version_import_prefix,
        &shared_types_import,
        options,
        &resolver,
    )
}

#[allow(clippy::too_many_lines)]
pub(crate) fn plan_versions_with_options(
    versions: &[Version],
    go_module: &str,
    options: &GoOptions,
) -> Result<GeneratedFiles> {
    validate_module_path(go_module)?;
    let dep_layout = DepLayout::from_versions(versions)?;
    let package = &versions
        .first()
        .context("no version directories found under WIT root")?
        .model
        .package;
    let header = header_for(&package.namespace, &package.name);
    let mut files = GeneratedFiles::new_nested(&header);

    files.insert_relative(
        "doc.go",
        emit_root_doc(&header, go_module, &package.namespace, &package.name)?,
    )?;

    let shared_types_import = format!("{go_module}/witforge_types");
    files.insert_relative(
        "witforge_types/types.go",
        emit_shared_types(&header, &package.namespace, &package.name)?,
    )?;

    let mut version_dirs: BTreeMap<String, &Path> = BTreeMap::new();
    for version in versions {
        let dir_name = version_dir_name(&version.model);
        if let Some(previous) = version_dirs.insert(dir_name.clone(), version.directory.as_path()) {
            contract_bail!(
                "version directory `{dir_name}` is produced by both {} and {}",
                previous.display(),
                version.directory.display()
            );
        }
        let major = version.major;
        let version_import_prefix = format!("{go_module}/{dir_name}");
        let resolver = |ns: &str, pkg: &str| -> Result<String> {
            let seg = safe_dep_pkg_dir(ns, pkg)?;
            if dep_layout.is_scoped(ns, pkg) {
                Ok(format!(
                    "{go_module}/witforge_deps/{}/{seg}",
                    polyglot::major_dir_name(major)
                ))
            } else {
                Ok(format!("{go_module}/witforge_deps/{seg}"))
            }
        };
        let emitted = emit_version_files(
            &header,
            &version.model,
            None,
            &version_import_prefix,
            &shared_types_import,
            options,
            &resolver,
        )
        .with_context(|| version.directory.display().to_string())?;
        for (rel_file, contents) in emitted {
            files.insert_relative(format!("{dir_name}/{rel_file}"), contents)?;
        }
    }

    if !dep_layout.is_empty() {
        emit_dependency_tree(
            &mut files,
            &header,
            go_module,
            &shared_types_import,
            &dep_layout,
            options,
        )?;
    }

    if versions.iter().any(|v| !v.model.worlds.is_empty()) {
        files.insert_relative("worlds/worlds.go", emit_worlds(&header, versions)?)?;
    }

    Ok(files)
}

// ---------------------------------------------------------------------------
// Root package documentation & ownership anchor (`doc.go`)
// ---------------------------------------------------------------------------

fn is_go_major_version_suffix(segment: &str) -> bool {
    let Some(digits) = segment.strip_prefix('v') else {
        return false;
    };
    !digits.is_empty()
        && !digits.starts_with('0')
        && digits.bytes().all(|b| b.is_ascii_digit())
        && digits.parse::<u64>().is_ok_and(|n| n >= 2)
}

fn root_package_name(go_module: &str, namespace: &str, name: &str) -> String {
    let segments: Vec<&str> = go_module.split('/').collect();
    let candidate =
        if segments.len() >= 2 && is_go_major_version_suffix(segments[segments.len() - 1]) {
            segments[segments.len() - 2]
        } else {
            segments[segments.len() - 1]
        };
    let mut pkg = if candidate.contains('.') {
        dep_package_segment(namespace, name)
    } else {
        let mut s = String::with_capacity(candidate.len());
        let mut prev_underscore = false;
        for ch in candidate.chars() {
            if ch.is_ascii_alphanumeric() {
                s.push(ch.to_ascii_lowercase());
                prev_underscore = false;
            } else if !prev_underscore && !s.is_empty() {
                s.push('_');
                prev_underscore = true;
            }
        }
        while s.ends_with('_') {
            s.pop();
        }
        if s.is_empty() {
            dep_package_segment(namespace, name)
        } else {
            s
        }
    };
    if pkg.as_bytes()[0].is_ascii_digit() {
        pkg = format!("p_{pkg}");
    }
    if is_go_keyword(&pkg)
        || is_go_predeclared(&pkg)
        || matches!(pkg.as_str(), "main" | "init" | "documentation")
        || pkg.ends_with("_test")
        || pkg == "_"
    {
        pkg.push('_');
    }
    pkg
}

fn emit_root_doc(header: &str, go_module: &str, namespace: &str, name: &str) -> Result<String> {
    let root_pkg = root_package_name(go_module, namespace, name);
    let mut out = String::new();
    writeln!(out, "{header}")?;
    writeln!(out, "// source: {namespace}:{name}")?;
    writeln!(out)?;
    writeln!(
        out,
        "// Package {root_pkg} is the generated Go root for WIT contract `{namespace}:{name}`."
    )?;
    writeln!(out, "//")?;
    writeln!(
        out,
        "// Import contract versions from `v<major>_<minor>/<interface>`, shared"
    )?;
    writeln!(
        out,
        "// generic Option and Result helpers from `witforge_types`, and dependencies"
    )?;
    writeln!(out, "// from `witforge_deps`.")?;
    writeln!(out, "package {root_pkg}")?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Shared generic types package (`witforge_types/types.go`)
// ---------------------------------------------------------------------------

fn emit_shared_types(header: &str, namespace: &str, name: &str) -> Result<String> {
    let mut out = String::new();
    writeln!(out, "{header}")?;
    writeln!(out, "// source: {namespace}:{name}")?;
    writeln!(out)?;
    writeln!(
        out,
        "// Package witforge_types provides shared generic Option and Result value types"
    )?;
    writeln!(
        out,
        "// used by generated Go packages for nested WIT options and WIT results."
    )?;
    writeln!(out, "package witforge_types")?;
    writeln!(out)?;
    writeln!(
        out,
        "// Option represents an explicit optional value for nested WIT `option<option<T>>`"
    )?;
    writeln!(
        out,
        "// shapes so `Some(nil)` stays distinct from `None()`."
    )?;
    writeln!(out, "type Option[T any] struct {{")?;
    writeln!(out, "\tValue T")?;
    writeln!(out, "\tSome  bool")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "// Some constructs a present Option carrying value.")?;
    writeln!(out, "func Some[T any](value T) Option[T] {{")?;
    writeln!(out, "\treturn Option[T]{{Value: value, Some: true}}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "// None constructs an absent Option.")?;
    writeln!(out, "func None[T any]() Option[T] {{")?;
    writeln!(out, "\treturn Option[T]{{}}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "// Result represents a WIT `result<T, E>` value.")?;
    writeln!(
        out,
        "// When IsOk is true, Ok holds the success payload; otherwise Err holds the error payload."
    )?;
    writeln!(out, "type Result[T, E any] struct {{")?;
    writeln!(out, "\tOk   T")?;
    writeln!(out, "\tErr  E")?;
    writeln!(out, "\tIsOk bool")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "// NewOk constructs a successful Result carrying value."
    )?;
    writeln!(out, "func NewOk[T, E any](value T) Result[T, E] {{")?;
    writeln!(out, "\treturn Result[T, E]{{Ok: value, IsOk: true}}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "// NewErr constructs an error Result carrying err.")?;
    writeln!(out, "func NewErr[T, E any](err E) Result[T, E] {{")?;
    writeln!(out, "\treturn Result[T, E]{{Err: err, IsOk: false}}")?;
    writeln!(out, "}}")?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Dependency packages (`witforge_deps/...`)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
fn emit_dependency_tree(
    files: &mut GeneratedFiles,
    header: &str,
    go_module: &str,
    shared_types_import: &str,
    dep_layout: &DepLayout,
    options: &GoOptions,
) -> Result<()> {
    // 1. Shared dependencies (identical across every live major that uses them).
    let mut shared_segs: BTreeMap<String, String> = BTreeMap::new();
    let shared_slice: Vec<Dependency> = dep_layout.shared.values().cloned().collect();
    for dep in &shared_slice {
        let seg = safe_dep_pkg_dir(&dep.package.namespace, &dep.package.name)?;
        let seg_lower = seg.to_ascii_lowercase();
        let pkg_label = format!("{}:{}", dep.package.namespace, dep.package.name);
        if let Some(previous) = shared_segs.insert(seg_lower, pkg_label.clone()) {
            contract_bail!(
                "dependency packages `{previous}` and `{pkg_label}` both map to Go directory \
                 `witforge_deps/{seg}`"
            );
        }
        let dep_model = dependency_model(dep, &shared_slice);
        let pkg_import_prefix = format!("{go_module}/witforge_deps/{seg}");
        let resolver = |ns: &str, pkg: &str| -> Result<String> {
            let target_seg = safe_dep_pkg_dir(ns, pkg)?;
            Ok(format!("{go_module}/witforge_deps/{target_seg}"))
        };
        let emitted = emit_version_files(
            header,
            &dep_model,
            Some(dep),
            &pkg_import_prefix,
            shared_types_import,
            options,
            &resolver,
        )?;
        for (rel_file, contents) in emitted {
            files.insert_relative(format!("witforge_deps/{seg}/{rel_file}"), contents)?;
        }
    }

    // 2. Per-major scoped dependencies (differ across live majors, with canonical-owner
    //    type/const alias re-exports when two majors share an identical definition).
    for (&major, scoped_deps) in &dep_layout.per_major {
        let major_dir = polyglot::major_dir_name(major);
        let major_all = dep_layout
            .by_major
            .get(&major)
            .map_or(scoped_deps.as_slice(), Vec::as_slice);
        let mut major_segs: BTreeMap<String, String> = BTreeMap::new();
        for dep in scoped_deps {
            let seg = safe_dep_pkg_dir(&dep.package.namespace, &dep.package.name)?;
            let seg_lower = seg.to_ascii_lowercase();
            let pkg_label = format!("{}:{}", dep.package.namespace, dep.package.name);
            if let Some(previous) = major_segs.insert(seg_lower, pkg_label.clone()) {
                contract_bail!(
                    "dependency packages `{previous}` and `{pkg_label}` both map to Go directory \
                     `witforge_deps/{major_dir}/{seg}`"
                );
            }
            let owner =
                dep_layout.canonical_owner_major(major, &dep.package.namespace, &dep.package.name);
            if owner == major {
                let dep_model = dependency_model(dep, major_all);
                let pkg_import_prefix = format!("{go_module}/witforge_deps/{major_dir}/{seg}");
                let resolver = |ns: &str, pkg: &str| -> Result<String> {
                    let target_seg = safe_dep_pkg_dir(ns, pkg)?;
                    if dep_layout.is_scoped(ns, pkg) {
                        Ok(format!(
                            "{go_module}/witforge_deps/{major_dir}/{target_seg}"
                        ))
                    } else {
                        Ok(format!("{go_module}/witforge_deps/{target_seg}"))
                    }
                };
                let emitted = emit_version_files(
                    header,
                    &dep_model,
                    Some(dep),
                    &pkg_import_prefix,
                    shared_types_import,
                    options,
                    &resolver,
                )?;
                for (rel_file, contents) in emitted {
                    files.insert_relative(
                        format!("witforge_deps/{major_dir}/{seg}/{rel_file}"),
                        contents,
                    )?;
                }
            } else {
                let owner_dir = polyglot::major_dir_name(owner);
                let iface_plan = plan_interface_packages(&dep.interfaces, &pkg_label)?;
                for (iface, iface_pkg) in iface_plan {
                    let owner_import =
                        format!("{go_module}/witforge_deps/{owner_dir}/{seg}/{iface_pkg}");
                    let content =
                        emit_reexport_interface(header, dep, iface, &iface_pkg, &owner_import)?;
                    files.insert_relative(
                        format!("witforge_deps/{major_dir}/{seg}/{iface_pkg}/types.go"),
                        content,
                    )?;
                }
            }
        }
    }

    Ok(())
}

fn emit_reexport_interface(
    header: &str,
    dep: &Dependency,
    interface: &Interface,
    iface_pkg: &str,
    owner_import: &str,
) -> Result<String> {
    let symbols = plan_interface_symbols(interface)?;
    let mut out = String::new();
    writeln!(out, "{header}")?;
    writeln!(
        out,
        "// source: {}:{}@{} (interface {})",
        dep.package.namespace, dep.package.name, dep.package.version, interface.name
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "// Package {iface_pkg} re-exports generated Go bindings for WIT interface `{}`",
        interface.name
    )?;
    writeln!(
        out,
        "// in `{}:{}@{}` from its canonical owner package.",
        dep.package.namespace, dep.package.name, dep.package.version
    )?;
    writeln!(out, "package {iface_pkg}")?;

    let has_exports = !interface.types.is_empty() || symbols.service_interface_name.is_some();
    if !has_exports {
        return Ok(out);
    }

    writeln!(out)?;
    writeln!(out, "import wf_owner {}", escape_go_string(owner_import))?;

    for (def_idx, def) in interface.types.iter().enumerate() {
        let type_name = &symbols.type_names[def_idx];
        writeln!(out)?;
        writeln!(
            out,
            "// {type_name} aliases the canonical `{}/{}` type.",
            interface.name, def.name
        )?;
        writeln!(out, "type {type_name} = wf_owner.{type_name}")?;

        match &def.kind {
            TypeDefKind::Enum(cases) if !cases.is_empty() => {
                writeln!(out)?;
                writeln!(out, "const (")?;
                let entries: Vec<(Vec<String>, String, String)> = cases
                    .iter()
                    .enumerate()
                    .map(|(case_idx, _)| {
                        let c_name = symbols.enum_const_names[&(def_idx, case_idx)].clone();
                        let rhs = format!("= wf_owner.{c_name}");
                        (Vec::new(), c_name, rhs)
                    })
                    .collect();
                write_aligned_entries(&mut out, &entries)?;
                writeln!(out, ")")?;
            }
            TypeDefKind::Variant(cases) => {
                for (case_idx, case) in cases.iter().enumerate() {
                    let case_name = &symbols.variant_case_names[&(def_idx, case_idx)];
                    writeln!(out)?;
                    writeln!(
                        out,
                        "// {case_name} aliases the `{}` case of `{type_name}`.",
                        case.name
                    )?;
                    writeln!(out, "type {case_name} = wf_owner.{case_name}")?;
                }
            }
            _ => {}
        }
    }

    if let Some(service_name) = &symbols.service_interface_name {
        writeln!(out)?;
        writeln!(
            out,
            "// {service_name} aliases the canonical `{}` interface.",
            interface.name
        )?;
        writeln!(out, "type {service_name} = wf_owner.{service_name}")?;
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Worlds metadata package (`worlds/worlds.go`)
// ---------------------------------------------------------------------------

fn emit_worlds(header: &str, versions: &[Version]) -> Result<String> {
    let package = &versions
        .first()
        .context("no version directories found under WIT root")?
        .model
        .package;
    let mut out = String::new();
    writeln!(out, "{header}")?;
    writeln!(out, "// source: {}:{}", package.namespace, package.name)?;
    writeln!(out)?;
    writeln!(
        out,
        "// Package worlds describes the WIT worlds declared across live contract versions."
    )?;
    writeln!(out, "package worlds")?;
    writeln!(out)?;
    writeln!(
        out,
        "// World describes one WIT world's imported and exported interfaces in source order."
    )?;
    writeln!(out, "type World struct {{")?;
    writeln!(out, "\tName    string")?;
    writeln!(out, "\tImports []string")?;
    writeln!(out, "\tExports []string")?;
    writeln!(out, "}}")?;

    let mut version_vars: Vec<(String, String)> = Vec::with_capacity(versions.len());
    for version in versions {
        let dir_name = version_dir_name(&version.model);
        let var_name = format!(
            "V{}_{}",
            version.model.package.version.major, version.model.package.version.minor
        );
        writeln!(out)?;
        writeln!(
            out,
            "// {var_name} lists the worlds declared in contract version `{}`.",
            version.model.package.version
        )?;
        if version.model.worlds.is_empty() {
            writeln!(out, "var {var_name} = []World{{}}")?;
        } else {
            writeln!(out, "var {var_name} = []World{{")?;
            for world in &version.model.worlds {
                let doc_lines = combined_doc_lines(&world.docs, &world.stability, None);
                write_doc_comment(
                    &mut out,
                    "\t",
                    &doc_lines,
                    world.stability.deprecation_note(),
                )?;
                writeln!(out, "\t{{")?;
                writeln!(out, "\t\tName:    {},", escape_go_string(&world.name))?;
                writeln!(
                    out,
                    "\t\tImports: {},",
                    format_go_world_interfaces(&world.imports)
                )?;
                writeln!(
                    out,
                    "\t\tExports: {},",
                    format_go_world_interfaces(&world.exports)
                )?;
                writeln!(out, "\t}},")?;
            }
            writeln!(out, "}}")?;
        }
        version_vars.push((dir_name, var_name));
    }

    writeln!(out)?;
    writeln!(
        out,
        "// ByVersion maps each live version directory name (`v<major>_<minor>`) to its worlds."
    )?;
    writeln!(out, "var ByVersion = map[string][]World{{")?;
    let max_key_len = version_vars
        .iter()
        .map(|(dir, _)| escape_go_string(dir).len() + 1)
        .max()
        .unwrap_or(0);
    for (dir, var_name) in &version_vars {
        let key_colon = format!("{}:", escape_go_string(dir));
        let pad = " ".repeat(max_key_len.saturating_sub(key_colon.len()) + 1);
        writeln!(out, "\t{key_colon}{pad}{var_name},")?;
    }
    writeln!(out, "}}")?;
    Ok(out)
}

fn format_go_world_interfaces(items: &[WorldInterface]) -> String {
    if items.is_empty() {
        return "[]string{}".to_string();
    }
    let elems: Vec<String> = items
        .iter()
        .map(|item| escape_go_string(&item.qualified_name()))
        .collect();
    format!("[]string{{{}}}", elems.join(", "))
}

// ---------------------------------------------------------------------------
// Per-version / per-interface emission
// ---------------------------------------------------------------------------

fn emit_version_files(
    header: &str,
    model: &Model,
    dep_context: Option<&Dependency>,
    pkg_import_prefix: &str,
    shared_types_import: &str,
    _options: &GoOptions,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
) -> Result<BTreeMap<String, String>> {
    let scope = format!(
        "{}:{}@{}",
        model.package.namespace, model.package.name, model.package.version
    );
    let iface_plan = plan_interface_packages(&model.interfaces, &scope)?;
    let local_pkgs: BTreeMap<&str, &str> = iface_plan
        .iter()
        .map(|(iface, pkg)| (iface.name.as_str(), pkg.as_str()))
        .collect();

    let mut out_files = BTreeMap::new();
    for (interface, iface_pkg) in &iface_plan {
        let source = emit_interface_file(
            header,
            model,
            dep_context,
            interface,
            iface_pkg,
            pkg_import_prefix,
            &local_pkgs,
            shared_types_import,
            foreign_resolver,
        )?;
        out_files.insert(format!("{iface_pkg}/types.go"), source);
    }
    Ok(out_files)
}

pub(crate) fn plan_interface_packages<'a>(
    interfaces: &'a [Interface],
    scope: &str,
) -> Result<Vec<(&'a Interface, String)>> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut planned = Vec::with_capacity(interfaces.len());
    for interface in interfaces {
        let pkg = safe_interface_pkg_name(&interface.name)?;
        let lower = pkg.to_ascii_lowercase();
        if let Some(previous) = seen.insert(lower, interface.name.clone()) {
            contract_bail!(
                "{scope}: interfaces `{previous}` and `{}` both map to Go package `{pkg}`",
                interface.name
            );
        }
        planned.push((interface, pkg));
    }
    Ok(planned)
}

/// Planned Go package-level symbol names for one WIT interface (shared with `W13-007` Go glue).
pub(crate) struct GoInterfaceSymbolPlan {
    pub(crate) service_interface_name: Option<String>,
    pub(crate) type_names: Vec<String>,
    pub(crate) enum_const_names: BTreeMap<(usize, usize), String>,
    pub(crate) variant_case_names: BTreeMap<(usize, usize), String>,
}

pub(crate) fn plan_interface_symbols(interface: &Interface) -> Result<GoInterfaceSymbolPlan> {
    let mut pkg_symbols: BTreeMap<String, String> = BTreeMap::new();
    let service_interface_name = if interface.functions.is_empty() {
        None
    } else {
        let name = go_exported_ident(&interface.name, "interface")?;
        pkg_symbols.insert(name.clone(), format!("interface {}", interface.name));
        Some(name)
    };

    let mut type_names = Vec::with_capacity(interface.types.len());
    for def in &interface.types {
        let name = go_exported_ident(&def.name, "type")?;
        insert_name(
            &mut pkg_symbols,
            &name,
            &format!("type {}/{}", interface.name, def.name),
            &format!("interface `{}`", interface.name),
        )?;
        type_names.push(name);
    }

    let mut enum_const_names = BTreeMap::new();
    let mut variant_case_names = BTreeMap::new();

    for (def_idx, def) in interface.types.iter().enumerate() {
        let parent_name = &type_names[def_idx];
        let scope = format!("{}/{}", interface.name, def.name);
        match &def.kind {
            TypeDefKind::Enum(cases) => {
                let mut seen_cases = BTreeMap::new();
                for (case_idx, case) in cases.iter().enumerate() {
                    let case_pascal = go_exported_ident(&case.name, "enum case")?;
                    insert_name(&mut seen_cases, &case_pascal, &case.name, &scope)?;
                    let const_name = allocate_prefixed_symbol(
                        &mut pkg_symbols,
                        parent_name,
                        &case_pascal,
                        format!("enum case {}/{}.{}", interface.name, def.name, case.name),
                    );
                    enum_const_names.insert((def_idx, case_idx), const_name);
                }
            }
            TypeDefKind::Variant(cases) => {
                let mut seen_cases = BTreeMap::new();
                for (case_idx, case) in cases.iter().enumerate() {
                    let case_pascal = go_exported_ident(&case.name, "variant case")?;
                    insert_name(&mut seen_cases, &case_pascal, &case.name, &scope)?;
                    let case_struct = allocate_prefixed_symbol(
                        &mut pkg_symbols,
                        parent_name,
                        &case_pascal,
                        format!("variant case {}/{}.{}", interface.name, def.name, case.name),
                    );
                    variant_case_names.insert((def_idx, case_idx), case_struct);
                }
            }
            _ => {}
        }
    }

    Ok(GoInterfaceSymbolPlan {
        service_interface_name,
        type_names,
        enum_const_names,
        variant_case_names,
    })
}

fn allocate_prefixed_symbol(
    pkg_symbols: &mut BTreeMap<String, String>,
    parent: &str,
    suffix_pascal: &str,
    label: String,
) -> String {
    let preferred = format!("{parent}{suffix_pascal}");
    let chosen = if pkg_symbols.contains_key(&preferred) {
        let disambiguated = format!("{parent}_{suffix_pascal}");
        if pkg_symbols.contains_key(&disambiguated) {
            let mut n = 2usize;
            loop {
                let candidate = format!("{parent}_{suffix_pascal}_{n}");
                if !pkg_symbols.contains_key(&candidate) {
                    break candidate;
                }
                n += 1;
            }
        } else {
            disambiguated
        }
    } else {
        preferred
    };
    pkg_symbols.insert(chosen.clone(), label);
    chosen
}

#[derive(Default)]
struct GoImports {
    /// Import path -> alias (sorted lexicographically by import path for deterministic output).
    by_path: BTreeMap<String, String>,
    used_aliases: BTreeSet<String>,
}

impl GoImports {
    fn import_alias(&mut self, import_path: String, preferred_alias: &str) -> String {
        if let Some(existing) = self.by_path.get(&import_path) {
            return existing.clone();
        }
        let mut candidate = preferred_alias.to_string();
        let mut suffix = 2usize;
        while self.used_aliases.contains(&candidate) {
            candidate = format!("{preferred_alias}_{suffix}");
            suffix += 1;
        }
        self.used_aliases.insert(candidate.clone());
        self.by_path.insert(import_path, candidate.clone());
        candidate
    }

    fn write_to(&self, out: &mut String) -> Result<()> {
        match self.by_path.len() {
            0 => Ok(()),
            1 => {
                let (path, alias) = self.by_path.iter().next().expect("checked len == 1");
                writeln!(out)?;
                writeln!(out, "import {alias} {}", escape_go_string(path))?;
                Ok(())
            }
            _ => {
                writeln!(out)?;
                writeln!(out, "import (")?;
                for (path, alias) in &self.by_path {
                    writeln!(out, "\t{alias} {}", escape_go_string(path))?;
                }
                writeln!(out, ")")?;
                Ok(())
            }
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn emit_interface_file(
    header: &str,
    model: &Model,
    dep_context: Option<&Dependency>,
    interface: &Interface,
    iface_pkg: &str,
    pkg_import_prefix: &str,
    local_pkgs: &BTreeMap<&str, &str>,
    shared_types_import: &str,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
) -> Result<String> {
    let symbols = plan_interface_symbols(interface)?;
    let mut imports = GoImports::default();
    let mut body = String::new();

    for (def_idx, def) in interface.types.iter().enumerate() {
        writeln!(body)?;
        emit_type_def(
            &mut body,
            model,
            dep_context,
            interface,
            def_idx,
            def,
            &symbols,
            pkg_import_prefix,
            local_pkgs,
            shared_types_import,
            foreign_resolver,
            &mut imports,
        )?;
    }

    if let Some(service_name) = &symbols.service_interface_name {
        writeln!(body)?;
        emit_service_interface(
            &mut body,
            model,
            dep_context,
            interface,
            service_name,
            pkg_import_prefix,
            local_pkgs,
            shared_types_import,
            foreign_resolver,
            &mut imports,
        )?;
    }

    let mut out = String::new();
    writeln!(out, "{header}")?;
    writeln!(
        out,
        "// source: {}:{}@{} (interface {})",
        model.package.namespace, model.package.name, model.package.version, interface.name
    )?;
    writeln!(out)?;
    let fallback_pkg_doc = format!(
        "Package {iface_pkg} contains generated Go bindings for WIT interface `{}` in `{}:{}@{}`.",
        interface.name, model.package.namespace, model.package.name, model.package.version
    );
    let pkg_docs = combined_doc_lines(
        &interface.docs,
        &interface.stability,
        Some(&fallback_pkg_doc),
    );
    write_doc_comment(
        &mut out,
        "",
        &pkg_docs,
        interface.stability.deprecation_note(),
    )?;
    writeln!(out, "package {iface_pkg}")?;
    imports.write_to(&mut out)?;
    out.push_str(&body);
    Ok(out)
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn emit_type_def(
    out: &mut String,
    model: &Model,
    dep_context: Option<&Dependency>,
    interface: &Interface,
    def_idx: usize,
    def: &TypeDef,
    symbols: &GoInterfaceSymbolPlan,
    pkg_import_prefix: &str,
    local_pkgs: &BTreeMap<&str, &str>,
    shared_types_import: &str,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut GoImports,
) -> Result<()> {
    let type_name = &symbols.type_names[def_idx];
    let fallback = format!(
        "{type_name} represents WIT type `{}/{}`.",
        interface.name, def.name
    );
    let docs = combined_doc_lines(&def.docs, &def.stability, Some(&fallback));
    write_doc_comment(out, "", &docs, def.stability.deprecation_note())?;

    match &def.kind {
        TypeDefKind::Record(fields) => {
            emit_record(
                out,
                model,
                dep_context,
                interface,
                type_name,
                &def.name,
                fields,
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
        }
        TypeDefKind::Flags(flags) => {
            emit_flags(out, interface, type_name, &def.name, flags)?;
        }
        TypeDefKind::Enum(cases) => {
            emit_enum(out, type_name, def_idx, cases, symbols)?;
        }
        TypeDefKind::Variant(cases) => {
            emit_variant(
                out,
                model,
                dep_context,
                interface,
                type_name,
                def_idx,
                cases,
                symbols,
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
        }
        TypeDefKind::Alias(target) => {
            let rhs = go_type_expr(
                model,
                dep_context,
                interface,
                target,
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "type {type_name} = {rhs}")?;
        }
        TypeDefKind::List(elem) => {
            let rhs = go_type_expr(
                model,
                dep_context,
                interface,
                &TypeExpr::List(Box::new(elem.clone())),
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "type {type_name} = {rhs}")?;
        }
        TypeDefKind::FixedLengthList(elem, len) => {
            let rhs = go_type_expr(
                model,
                dep_context,
                interface,
                &TypeExpr::FixedLengthList(Box::new(elem.clone()), *len),
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "type {type_name} = {rhs}")?;
        }
        TypeDefKind::Map(key, value) => {
            let rhs = go_type_expr(
                model,
                dep_context,
                interface,
                &TypeExpr::Map(Box::new(key.clone()), Box::new(value.clone())),
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "type {type_name} = {rhs}")?;
        }
        TypeDefKind::Option(payload) => {
            let rhs = go_type_expr(
                model,
                dep_context,
                interface,
                &TypeExpr::Option(Box::new(payload.clone())),
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "type {type_name} = {rhs}")?;
        }
        TypeDefKind::Result(result) => {
            let rhs = go_type_expr(
                model,
                dep_context,
                interface,
                &TypeExpr::Result(result.clone()),
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "type {type_name} = {rhs}")?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_record(
    out: &mut String,
    model: &Model,
    dep_context: Option<&Dependency>,
    interface: &Interface,
    type_name: &str,
    wit_name: &str,
    fields: &[Field],
    pkg_import_prefix: &str,
    local_pkgs: &BTreeMap<&str, &str>,
    shared_types_import: &str,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut GoImports,
) -> Result<()> {
    if fields.is_empty() {
        writeln!(out, "type {type_name} struct{{}}")?;
        return Ok(());
    }
    let scope = format!("{}/{wit_name}", interface.name);
    let mut seen_fields = BTreeMap::new();
    let mut entries: Vec<(Vec<String>, String, String)> = Vec::with_capacity(fields.len());
    for field in fields {
        let field_name = go_exported_ident(&field.name, "record field")?;
        insert_name(&mut seen_fields, &field_name, &field.name, &scope)?;
        let field_ty = go_type_expr(
            model,
            dep_context,
            interface,
            &field.ty,
            pkg_import_prefix,
            local_pkgs,
            shared_types_import,
            foreign_resolver,
            imports,
        )?;
        let docs = normalize_go_doc_lines(&polyglot::doc_lines(&field.docs));
        entries.push((docs, field_name, field_ty));
    }

    writeln!(out, "type {type_name} struct {{")?;
    write_aligned_entries(out, &entries)?;
    writeln!(out, "}}")?;
    Ok(())
}

fn emit_flags(
    out: &mut String,
    interface: &Interface,
    type_name: &str,
    wit_name: &str,
    flags: &[Flag],
) -> Result<()> {
    if flags.is_empty() {
        writeln!(out, "type {type_name} struct{{}}")?;
        return Ok(());
    }
    let scope = format!("{}/{wit_name}", interface.name);
    let mut seen_flags = BTreeMap::new();
    let mut entries: Vec<(Vec<String>, String, String)> = Vec::with_capacity(flags.len());
    for flag in flags {
        let flag_name = go_exported_ident(&flag.name, "flag")?;
        insert_name(&mut seen_flags, &flag_name, &flag.name, &scope)?;
        let docs = normalize_go_doc_lines(&polyglot::doc_lines(&flag.docs));
        entries.push((docs, flag_name, "bool".to_string()));
    }

    writeln!(out, "type {type_name} struct {{")?;
    write_aligned_entries(out, &entries)?;
    writeln!(out, "}}")?;
    Ok(())
}

fn emit_enum(
    out: &mut String,
    type_name: &str,
    def_idx: usize,
    cases: &[Case],
    symbols: &GoInterfaceSymbolPlan,
) -> Result<()> {
    writeln!(out, "type {type_name} uint32")?;

    if !cases.is_empty() {
        writeln!(out)?;
        writeln!(out, "const (")?;
        let mut entries: Vec<(Vec<String>, String, String)> = Vec::with_capacity(cases.len());
        for (case_idx, case) in cases.iter().enumerate() {
            let const_name = symbols.enum_const_names[&(def_idx, case_idx)].clone();
            let docs = normalize_go_doc_lines(&polyglot::doc_lines(&case.docs));
            let discriminant = case_idx + 1;
            entries.push((docs, const_name, format!("{type_name} = {discriminant}")));
        }
        write_aligned_entries(out, &entries)?;
        writeln!(out, ")")?;
    }

    // Valid() bool
    writeln!(out)?;
    writeln!(
        out,
        "// Valid reports whether v is a defined {type_name} value."
    )?;
    writeln!(out, "func (v {type_name}) Valid() bool {{")?;
    if cases.is_empty() {
        writeln!(out, "\treturn false")?;
    } else {
        let const_list: Vec<&str> = (0..cases.len())
            .map(|case_idx| symbols.enum_const_names[&(def_idx, case_idx)].as_str())
            .collect();
        writeln!(out, "\tswitch v {{")?;
        writeln!(out, "\tcase {}:", const_list.join(", "))?;
        writeln!(out, "\t\treturn true")?;
        writeln!(out, "\tdefault:")?;
        writeln!(out, "\t\treturn false")?;
        writeln!(out, "\t}}")?;
    }
    writeln!(out, "}}")?;

    // String() string
    writeln!(out)?;
    writeln!(
        out,
        "// String returns the canonical WIT case name of v, or an empty string if invalid."
    )?;
    writeln!(out, "func (v {type_name}) String() string {{")?;
    if cases.is_empty() {
        writeln!(out, "\treturn \"\"")?;
    } else {
        writeln!(out, "\tswitch v {{")?;
        for (case_idx, case) in cases.iter().enumerate() {
            let const_name = &symbols.enum_const_names[&(def_idx, case_idx)];
            writeln!(out, "\tcase {const_name}:")?;
            writeln!(out, "\t\treturn {}", escape_go_string(&case.name))?;
        }
        writeln!(out, "\tdefault:")?;
        writeln!(out, "\t\treturn \"\"")?;
        writeln!(out, "\t}}")?;
    }
    writeln!(out, "}}")?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_variant(
    out: &mut String,
    model: &Model,
    dep_context: Option<&Dependency>,
    interface: &Interface,
    type_name: &str,
    def_idx: usize,
    cases: &[Case],
    symbols: &GoInterfaceSymbolPlan,
    pkg_import_prefix: &str,
    local_pkgs: &BTreeMap<&str, &str>,
    shared_types_import: &str,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut GoImports,
) -> Result<()> {
    let marker_method = format!("is{type_name}");
    writeln!(out, "type {type_name} interface {{")?;
    writeln!(out, "\t{marker_method}()")?;
    writeln!(out, "}}")?;

    for (case_idx, case) in cases.iter().enumerate() {
        let case_struct = &symbols.variant_case_names[&(def_idx, case_idx)];
        let fallback = format!(
            "{case_struct} represents the `{}` case of `{type_name}`.",
            case.name
        );
        let case_docs = combined_doc_lines(&case.docs, &Stability::Unknown, Some(&fallback));
        writeln!(out)?;
        write_doc_comment(out, "", &case_docs, None)?;
        if let Some(payload) = &case.payload {
            let payload_ty = go_type_expr(
                model,
                dep_context,
                interface,
                payload,
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "type {case_struct} struct {{")?;
            writeln!(out, "\tValue {payload_ty}")?;
            writeln!(out, "}}")?;
        } else {
            writeln!(out, "type {case_struct} struct{{}}")?;
        }
        writeln!(out)?;
        writeln!(out, "func ({case_struct}) {marker_method}() {{}}")?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_service_interface(
    out: &mut String,
    model: &Model,
    dep_context: Option<&Dependency>,
    interface: &Interface,
    service_name: &str,
    pkg_import_prefix: &str,
    local_pkgs: &BTreeMap<&str, &str>,
    shared_types_import: &str,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut GoImports,
) -> Result<()> {
    let fallback = format!(
        "{service_name} is the Go interface for WIT interface `{}`.",
        interface.name
    );
    let docs = combined_doc_lines(&interface.docs, &interface.stability, Some(&fallback));
    write_doc_comment(out, "", &docs, interface.stability.deprecation_note())?;
    writeln!(out, "type {service_name} interface {{")?;

    let iface_scope = format!("interface `{}`", interface.name);
    let mut seen_methods = BTreeMap::new();
    for (idx, function) in interface.functions.iter().enumerate() {
        if idx > 0 {
            writeln!(out)?;
        }
        let method_name = go_exported_ident(&function.name, "function")?;
        insert_name(
            &mut seen_methods,
            &method_name,
            &function.name,
            &iface_scope,
        )?;

        let fn_scope = format!("{}#{}", interface.name, function.name);
        let fallback_fn = format!("{method_name} implements WIT function `{fn_scope}`.");
        let fn_docs = combined_doc_lines(&function.docs, &function.stability, Some(&fallback_fn));
        write_doc_comment(out, "\t", &fn_docs, function.stability.deprecation_note())?;

        let mut seen_params = BTreeMap::new();
        let mut params = Vec::with_capacity(function.params.len());
        for param in &function.params {
            let param_name = go_param_ident(&param.name)?;
            insert_name(&mut seen_params, &param_name, &param.name, &fn_scope)?;
            let param_ty = go_type_expr(
                model,
                dep_context,
                interface,
                &param.ty,
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            params.push(format!("{param_name} {param_ty}"));
        }
        if let Some(result_ty) = &function.result {
            let ret = go_type_expr(
                model,
                dep_context,
                interface,
                result_ty,
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "\t{method_name}({}) {ret}", params.join(", "))?;
        } else {
            writeln!(out, "\t{method_name}({})", params.join(", "))?;
        }
    }

    writeln!(out, "}}")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// WIT TypeExpr -> Go type expression
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn go_type_expr(
    model: &Model,
    dep_context: Option<&Dependency>,
    current_iface: &Interface,
    ty: &TypeExpr,
    pkg_import_prefix: &str,
    local_pkgs: &BTreeMap<&str, &str>,
    shared_types_import: &str,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut GoImports,
) -> Result<String> {
    match ty {
        TypeExpr::Bool => Ok("bool".to_string()),
        TypeExpr::U8 => Ok("uint8".to_string()),
        TypeExpr::U16 => Ok("uint16".to_string()),
        TypeExpr::U32 => Ok("uint32".to_string()),
        TypeExpr::U64 => Ok("uint64".to_string()),
        TypeExpr::S8 => Ok("int8".to_string()),
        TypeExpr::S16 => Ok("int16".to_string()),
        TypeExpr::S32 => Ok("int32".to_string()),
        TypeExpr::S64 => Ok("int64".to_string()),
        TypeExpr::F32 => Ok("float32".to_string()),
        TypeExpr::F64 => Ok("float64".to_string()),
        TypeExpr::Char => Ok("rune".to_string()),
        TypeExpr::String => Ok("string".to_string()),
        TypeExpr::List(elem) => {
            if model.resolves_to_u8_in(dep_context, elem) {
                return Ok("[]byte".to_string());
            }
            let inner = go_type_expr(
                model,
                dep_context,
                current_iface,
                elem,
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            Ok(format!("[]{inner}"))
        }
        TypeExpr::FixedLengthList(elem, len) => {
            if model.resolves_to_u8_in(dep_context, elem) {
                return Ok(format!("[{len}]byte"));
            }
            let inner = go_type_expr(
                model,
                dep_context,
                current_iface,
                elem,
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            Ok(format!("[{len}]{inner}"))
        }
        TypeExpr::Map(key, value) => {
            let k = go_type_expr(
                model,
                dep_context,
                current_iface,
                key,
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            let v = go_type_expr(
                model,
                dep_context,
                current_iface,
                value,
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            Ok(format!("map[{k}]{v}"))
        }
        TypeExpr::Option(payload) => {
            let inner = go_type_expr(
                model,
                dep_context,
                current_iface,
                payload,
                pkg_import_prefix,
                local_pkgs,
                shared_types_import,
                foreign_resolver,
                imports,
            )?;
            if model.resolves_to_option_in(dep_context, payload) {
                let alias = imports.import_alias(shared_types_import.to_string(), "wf_types");
                Ok(format!("{alias}.Option[{inner}]"))
            } else {
                Ok(format!("*{inner}"))
            }
        }
        TypeExpr::Result(ResultExpr { ok, err }) => {
            let alias = imports.import_alias(shared_types_import.to_string(), "wf_types");
            let ok_ty = match ok {
                Some(ty) => go_type_expr(
                    model,
                    dep_context,
                    current_iface,
                    ty,
                    pkg_import_prefix,
                    local_pkgs,
                    shared_types_import,
                    foreign_resolver,
                    imports,
                )?,
                None => "struct{}".to_string(),
            };
            let err_ty = match err {
                Some(ty) => go_type_expr(
                    model,
                    dep_context,
                    current_iface,
                    ty,
                    pkg_import_prefix,
                    local_pkgs,
                    shared_types_import,
                    foreign_resolver,
                    imports,
                )?,
                None => "struct{}".to_string(),
            };
            Ok(format!("{alias}.Result[{ok_ty}, {err_ty}]"))
        }
        TypeExpr::Named(name) => go_named_type(
            model,
            dep_context,
            current_iface,
            name,
            pkg_import_prefix,
            local_pkgs,
            foreign_resolver,
            imports,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn go_named_type(
    model: &Model,
    dep_context: Option<&Dependency>,
    current_iface: &Interface,
    name: &QualifiedName,
    pkg_import_prefix: &str,
    local_pkgs: &BTreeMap<&str, &str>,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut GoImports,
) -> Result<String> {
    let type_ident = go_exported_ident(&name.name, "type")?;
    if let Some((ns, pkg)) = name.package() {
        model.type_def_in(dep_context, name).with_context(|| {
            format!(
                "named dependency type `{}/{}` is not in the model",
                name.interface, name.name
            )
        })?;
        let base_import = foreign_resolver(ns, pkg)?;
        let iface_pkg = safe_interface_pkg_name(name.interface_name())?;
        let full_import = format!("{base_import}/{iface_pkg}");
        let dep_seg = safe_dep_pkg_dir(ns, pkg)?;
        let alias = imports.import_alias(full_import, &format!("wf_dep_{dep_seg}_{iface_pkg}"));
        return Ok(format!("{alias}.{type_ident}"));
    }
    model.type_def_in(dep_context, name).with_context(|| {
        format!(
            "named type `{}/{}` is not in the model",
            name.interface, name.name
        )
    })?;
    if name.interface == current_iface.name {
        return Ok(type_ident);
    }
    let sibling_pkg = local_pkgs
        .get(name.interface.as_str())
        .copied()
        .map(str::to_string)
        .map_or_else(|| safe_interface_pkg_name(&name.interface), Ok)?;
    let full_import = format!("{pkg_import_prefix}/{sibling_pkg}");
    let alias = imports.import_alias(full_import, &format!("wf_if_{sibling_pkg}"));
    Ok(format!("{alias}.{type_ident}"))
}

// ---------------------------------------------------------------------------
// `gofmt`-exact alignment, doc comments, and identifier helpers
// ---------------------------------------------------------------------------

/// Write struct fields or `const` block entries matching `gofmt`'s exact alignment rules:
/// a leading doc comment begins a new alignment group, while consecutive entries without a
/// leading doc comment align their second column to `max_name_len + 1`.
fn write_aligned_entries(
    out: &mut String,
    entries: &[(Vec<String>, String, String)],
) -> Result<()> {
    let mut start = 0usize;
    while start < entries.len() {
        let mut end = start + 1;
        while end < entries.len() && entries[end].0.is_empty() {
            end += 1;
        }
        let max_name = entries[start..end]
            .iter()
            .map(|(_, name, _)| name.len())
            .max()
            .unwrap_or(0);
        for (docs, name, rhs) in &entries[start..end] {
            write_doc_comment(out, "\t", docs, None)?;
            let pad = " ".repeat(max_name.saturating_sub(name.len()) + 1);
            writeln!(out, "\t{name}{pad}{rhs}")?;
        }
        start = end;
    }
    Ok(())
}

fn combined_doc_lines(
    raw_docs: &str,
    stability: &Stability,
    fallback: Option<&str>,
) -> Vec<String> {
    let mut lines: Vec<String> = polyglot::doc_lines(raw_docs);
    if lines.is_empty()
        && let Some(fb) = fallback
    {
        lines.push(fb.to_string());
    }
    // Emit non-deprecation stability sentences in the main doc body; deprecation gets its own
    // `// Deprecated: ...` paragraph per Go doc convention.
    let non_deprecated_stability = if let Some(since) = stability.since() {
        Some(format!("Stable since WIT version {since}."))
    } else {
        stability
            .unstable_feature()
            .map(|feature| format!("Unstable WIT feature `{feature}`."))
    };
    if let Some(sentence) = non_deprecated_stability {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(sentence);
    }
    lines
}

fn sanitize_go_doc_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.strip_prefix("+build").is_some_and(|rest| {
        rest.is_empty() || rest.starts_with(|ch: char| ch.is_ascii_whitespace())
    }) {
        // Neutralize Go legacy `+build` constraint lines so `go vet` never rejects them as
        // misplaced build tags and `gofmt` never hoists `//go:build` above line 1's header.
        format!("\\{trimmed}")
    } else {
        trimmed.to_string()
    }
}

fn normalize_go_doc_lines(lines: &[String]) -> Vec<String> {
    let mut normalized = Vec::with_capacity(lines.len());
    for line in lines {
        let cleaned = sanitize_go_doc_line(line);
        if cleaned.is_empty() {
            if !normalized.is_empty() && !normalized.last().is_some_and(String::is_empty) {
                normalized.push(String::new());
            }
        } else {
            normalized.push(cleaned);
        }
    }
    while normalized.last().is_some_and(String::is_empty) {
        normalized.pop();
    }
    normalized
}

fn write_doc_comment(
    out: &mut String,
    indent: &str,
    lines: &[String],
    deprecated_note: Option<&str>,
) -> Result<()> {
    let normalized = normalize_go_doc_lines(lines);
    for line in &normalized {
        if line.is_empty() {
            writeln!(out, "{indent}//")?;
        } else {
            writeln!(out, "{indent}// {line}")?;
        }
    }
    if let Some(note) = deprecated_note {
        let clean_note = normalize_go_doc_lines(&polyglot::doc_lines(note))
            .into_iter()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !normalized.is_empty() {
            writeln!(out, "{indent}//")?;
        }
        writeln!(out, "{indent}// Deprecated: {clean_note}")?;
    }
    Ok(())
}

pub(crate) fn safe_interface_pkg_name(wit_name: &str) -> Result<String> {
    let mut pkg = to_snake_case(wit_name);
    if pkg.is_empty() {
        contract_bail!("interface `{wit_name}` cannot be represented as a Go package name");
    }
    if pkg.as_bytes()[0].is_ascii_digit() {
        pkg = format!("p_{pkg}");
    }
    if is_go_keyword(&pkg)
        || is_go_predeclared(&pkg)
        || is_windows_reserved_stem(&pkg)
        || matches!(
            pkg.as_str(),
            "internal" | "testdata" | "vendor" | "main" | "init" | "documentation"
        )
        || pkg.ends_with("_test")
    {
        pkg.push('_');
    }
    Ok(pkg)
}

pub(crate) fn safe_dep_pkg_dir(namespace: &str, name: &str) -> Result<String> {
    let mut seg = dep_package_segment(namespace, name);
    if seg.is_empty() || seg == "_" {
        contract_bail!(
            "dependency package `{namespace}:{name}` cannot be represented as a Go directory name"
        );
    }
    if seg.as_bytes()[0].is_ascii_digit() || seg.starts_with('_') {
        seg = format!("p_{seg}");
    }
    if is_windows_reserved_stem(&seg)
        || matches!(seg.as_str(), "internal" | "testdata" | "vendor")
        || seg.ends_with("_test")
    {
        seg.push('_');
    }
    Ok(seg)
}

pub(crate) fn go_exported_ident(wit_name: &str, kind: &str) -> Result<String> {
    let pascal = to_pascal_case(wit_name);
    if pascal.is_empty() || !pascal.as_bytes()[0].is_ascii_alphabetic() {
        contract_bail!("{kind} `{wit_name}` cannot be represented as an exported Go identifier");
    }
    // `to_pascal_case` always starts with an uppercase ASCII letter; no Go keyword or
    // predeclared universe identifier starts with uppercase.
    Ok(pascal)
}

pub(crate) fn go_param_ident(wit_name: &str) -> Result<String> {
    let camel = to_camel_case(wit_name);
    if camel.is_empty() || camel.as_bytes()[0].is_ascii_digit() {
        contract_bail!("parameter `{wit_name}` cannot be represented as a Go identifier");
    }
    Ok(escape_go_ident(&camel))
}

fn insert_name(
    seen: &mut BTreeMap<String, String>,
    generated: &str,
    wit_name: &str,
    scope: &str,
) -> Result<()> {
    if let Some(previous) = seen.insert(generated.to_string(), wit_name.to_string()) {
        contract_bail!(
            "{scope}: `{previous}` and `{wit_name}` both map to Go identifier `{generated}`"
        );
    }
    Ok(())
}

pub(crate) fn escape_go_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\x7F' => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c if (0x80..=0x9F).contains(&(c as u32)) => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_lines)]
    #[test]
    fn module_path_validation_and_go_package_guards_handle_special_names() {
        for valid in [
            "example.com/acme/contracts/gen/go",
            "github.com/org/repo/internal/gen",
            "acme.internal/v2/gen-go",
            "my_module",
            "mod/sub_1/pkg+v2~rc1",
        ] {
            validate_module_path(valid).unwrap_or_else(|e| panic!("must accept `{valid}`: {e}"));
        }
        for invalid in [
            "",
            "   ",
            "/leading/slash",
            "trailing/slash/",
            "double//slash",
            "./relative",
            "../parent",
            "example.com/a/../b",
            "example.com/-bad",
            "example.com/.hidden",
            "example.com/dot.",
            "example.com/a..b",
            "example.com/pkg;alias",
            "example.com/has space",
            "example.com/has\"quote",
            "example.com/back\\slash",
            "example.com/bad@ver",
            "example.com/con/gen",
            "example.com/nul.v1/gen",
            "example.com/COM1/gen",
            "example.com/pkg~1",
            "example.com/progra~1/gen",
            "example.com/foo~42.bar",
        ] {
            assert!(
                validate_module_path(invalid).is_err(),
                "must reject `{invalid}`"
            );
        }

        // Go keywords, predeclared names, Windows reserved stems, Go special directories,
        // Go special package names (`main`, `init`, `documentation`), and `_test` external-test
        // package suffixes are all escaped deterministically.
        assert_eq!(safe_interface_pkg_name("types").unwrap(), "types");
        assert_eq!(safe_interface_pkg_name("my-api").unwrap(), "my_api");
        assert_eq!(safe_interface_pkg_name("type").unwrap(), "type_");
        assert_eq!(safe_interface_pkg_name("error").unwrap(), "error_");
        assert_eq!(safe_interface_pkg_name("string").unwrap(), "string_");
        assert_eq!(safe_interface_pkg_name("con").unwrap(), "con_");
        assert_eq!(safe_interface_pkg_name("internal").unwrap(), "internal_");
        assert_eq!(safe_interface_pkg_name("testdata").unwrap(), "testdata_");
        assert_eq!(safe_interface_pkg_name("vendor").unwrap(), "vendor_");
        assert_eq!(safe_interface_pkg_name("main").unwrap(), "main_");
        assert_eq!(safe_interface_pkg_name("init").unwrap(), "init_");
        assert_eq!(
            safe_interface_pkg_name("documentation").unwrap(),
            "documentation_"
        );
        assert_eq!(safe_interface_pkg_name("my-test").unwrap(), "my_test_");
        assert_eq!(safe_interface_pkg_name("api-linux").unwrap(), "api_linux");

        assert_eq!(go_param_ident("type").unwrap(), "type_");
        assert_eq!(go_param_ident("string").unwrap(), "string_");
        assert_eq!(go_param_ident("error").unwrap(), "error_");
        assert_eq!(go_param_ident("start-byte").unwrap(), "startByte");
        assert_eq!(
            go_exported_ident("start-byte", "field").unwrap(),
            "StartByte"
        );

        assert_eq!(
            root_package_name("example.com/witforge/conf", "witforge", "conformance"),
            "conf"
        );
        assert_eq!(
            root_package_name("example.com/acme/contracts/gen/go", "acme", "app"),
            "go_"
        );
        assert_eq!(
            root_package_name("example.com/acme/contracts/v2", "acme", "app"),
            "contracts"
        );
        assert_eq!(
            root_package_name("example.com/v2", "acme", "app"),
            "acme_app"
        );
        assert_eq!(
            root_package_name("example.com/acme/my-test", "acme", "app"),
            "my_test_"
        );
        assert_eq!(
            root_package_name("example.com/acme/main", "acme", "app"),
            "main_"
        );
        assert_eq!(
            root_package_name("example.com/acme/MAIN/v2", "acme", "app"),
            "main_"
        );
        assert_eq!(
            root_package_name("example.com/acme/init", "acme", "app"),
            "init_"
        );
        assert_eq!(
            root_package_name("example.com/acme/documentation", "acme", "app"),
            "documentation_"
        );

        // Doc comment normalization collapses consecutive blank lines, strips indentation, and
        // neutralizes Go `+build` directives.
        let mut rendered = String::new();
        write_doc_comment(
            &mut rendered,
            "",
            &[
                String::new(),
                "  Summary line.  ".to_string(),
                String::new(),
                String::new(),
                "    +build ignore".to_string(),
                "\tcode()".to_string(),
                String::new(),
            ],
            None,
        )
        .unwrap();
        assert_eq!(
            rendered,
            "// Summary line.\n//\n// \\+build ignore\n// code()\n"
        );
    }
}
