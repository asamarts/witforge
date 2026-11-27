//! Deterministic Python 3.11+ plain types and `Protocol` interface emitter (`W13-004`).
//!
//! Emits zero-dependency Python 3.11+ stdlib packages (`@dataclass(frozen=True, slots=True)`,
//! `StrEnum`, `Protocol`, union variants, PEP 561 `py.typed` marker) directly from the owned
//! [`Model`] per `docs/design/polyglot-emitters.md`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::emit::output::{
    GeneratedFileFinding, GeneratedFiles, ensure_outside_wit_root, is_windows_reserved_stem,
};
use crate::emit::polyglot::{
    self, TargetLanguage, dep_package_segment, escape_python_ident, is_python_keyword,
    to_pascal_case, to_snake_case, to_upper_snake_case,
};
use crate::emit::stability_doc;
use crate::emit::typescript::dependency_model;
use crate::emit::versions::{DepLayout, Version, load_versions_with_released};
use crate::model::{
    Dependency, Interface, Model, QualifiedName, Stability, TypeDef, TypeDefKind, TypeExpr,
};

/// Options controlling plain Python code generation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PythonOptions {}

impl PythonOptions {
    /// Create default Python emission options.
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

pub(crate) fn header_for(namespace: &str, name: &str) -> String {
    TargetLanguage::Python.plain_header(namespace, name)
}

/// The Python package directory name for one contract version: `v<major>_<minor>`.
#[must_use]
pub fn version_dir_name(model: &Model) -> String {
    polyglot::version_dir_name(model.package.version.major, model.package.version.minor)
}

/// Plan the complete nested Python package output for a WIT root without writing anything.
///
/// Every `v<major>.<minor>` directory is loaded, checked against the subset, modelled, and
/// checked against its committed v2 lockfile. All versions must share one package namespace and
/// name.
///
/// # Errors
///
/// Returns an error for a missing, duplicate, or symlinked version directory, or an unreadable or
/// invalid lockfile, and a [`crate::ContractError`] for an inconsistent package, subset violations,
/// a stale lockfile, or generated-name collisions.
pub fn plan(wit_root: &Path) -> Result<GeneratedFiles> {
    plan_with_options(wit_root, None, &PythonOptions::new())
}

/// Plan the Python output for `wit_root`, using an optional explicit released-version manifest
/// path (`--released`) when determining retired versions.
///
/// # Errors
///
/// Returns the same errors as [`plan`].
pub fn plan_with_released(wit_root: &Path, released: Option<&Path>) -> Result<GeneratedFiles> {
    plan_with_options(wit_root, released, &PythonOptions::new())
}

/// Plan the Python output for `wit_root` with an optional explicit released-version manifest path
/// and explicit [`PythonOptions`].
///
/// # Errors
///
/// Returns the same errors as [`plan`].
pub fn plan_with_options(
    wit_root: &Path,
    released: Option<&Path>,
    options: &PythonOptions,
) -> Result<GeneratedFiles> {
    plan_versions_with_options(&load_versions_with_released(wit_root, released)?, options)
}

/// Generate Python output under `out`.
///
/// # Errors
///
/// Returns an error for invalid input, an output root inside the WIT tree, a foreign or unowned
/// file collision, or an I/O failure.
pub fn generate(wit_root: &Path, out: &Path) -> Result<()> {
    generate_with_options(wit_root, out, &PythonOptions::new())
}

/// Generate Python output under `out` with explicit [`PythonOptions`].
///
/// # Errors
///
/// Returns the same errors as [`generate`].
pub fn generate_with_options(wit_root: &Path, out: &Path, options: &PythonOptions) -> Result<()> {
    ensure_outside_wit_root(wit_root, out)?;
    plan_with_options(wit_root, None, options)?.write(out)
}

/// Check generated Python output under `out` without writing.
///
/// Returns one structured finding per missing, changed, extra, symlinked, or unowned file.
///
/// # Errors
///
/// Returns an error for invalid input, an output root inside the WIT tree, a foreign-owned file,
/// or an I/O failure.
pub fn check(wit_root: &Path, out: &Path) -> Result<Vec<GeneratedFileFinding>> {
    check_with_options(wit_root, out, &PythonOptions::new())
}

/// Check generated Python output under `out` with explicit [`PythonOptions`].
///
/// # Errors
///
/// Returns the same errors as [`check`].
pub fn check_with_options(
    wit_root: &Path,
    out: &Path,
    options: &PythonOptions,
) -> Result<Vec<GeneratedFileFinding>> {
    ensure_outside_wit_root(wit_root, out)?;
    plan_with_options(wit_root, None, options)?.check(out)
}

/// Emit the per-interface Python modules and `__init__.py` for one single contract version.
///
/// Keys are relative to the version directory (for example `types.py` and `__init__.py`). Emitted
/// interface modules import shared `Option`/`Ok`/`Err` helpers from `..witforge_types` and pinned
/// dependencies from `..witforge_deps.<pkg>`; use [`plan`] (or [`generate`]) for the complete
/// self-contained Python package including `py.typed`, `witforge_types.py`, `witforge_deps/`,
/// `worlds.py`, and the root `__init__.py`.
///
/// # Errors
///
/// Returns a [`crate::ContractError`] when a WIT name cannot be represented in Python or two
/// generated names collide, and a plain error when a named type is missing from `model`.
pub fn emit_version(model: &Model) -> Result<BTreeMap<String, String>> {
    emit_version_with_options(model, &PythonOptions::new())
}

/// Emit the per-interface Python modules and `__init__.py` for one single contract version with
/// explicit [`PythonOptions`].
///
/// # Errors
///
/// Returns the same errors as [`emit_version`].
pub fn emit_version_with_options(
    model: &Model,
    options: &PythonOptions,
) -> Result<BTreeMap<String, String>> {
    let header = header_for(&model.package.namespace, &model.package.name);
    let resolver = |ns: &str, pkg: &str| -> Result<String> {
        let seg = safe_pkg_segment(ns, pkg)?;
        Ok(format!("..witforge_deps.{seg}"))
    };
    emit_version_files(&header, model, options, &resolver)
}

#[allow(clippy::too_many_lines)]
pub(crate) fn plan_versions_with_options(
    versions: &[Version],
    options: &PythonOptions,
) -> Result<GeneratedFiles> {
    let dep_layout = DepLayout::from_versions(versions)?;
    let package = &versions
        .first()
        .context("no version directories found under WIT root")?
        .model
        .package;
    let header = header_for(&package.namespace, &package.name);
    let mut files = GeneratedFiles::new_nested(&header);

    files.insert_marker("py.typed", "")?;
    files.insert_relative(
        "witforge_types.py",
        emit_shared_types(&header, &package.namespace, &package.name)?,
    )?;

    let mut version_dirs: BTreeMap<String, &Path> = BTreeMap::new();
    let mut ordered_versions: Vec<String> = Vec::new();
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
        let resolver = |ns: &str, pkg: &str| -> Result<String> {
            let seg = safe_pkg_segment(ns, pkg)?;
            if dep_layout.is_scoped(ns, pkg) {
                Ok(format!(
                    "..witforge_deps.{}.{seg}",
                    polyglot::major_dir_name(major)
                ))
            } else {
                Ok(format!("..witforge_deps.{seg}"))
            }
        };
        let emitted = emit_version_files(&header, &version.model, options, &resolver)
            .with_context(|| version.directory.display().to_string())?;
        for (rel_file, contents) in emitted {
            files.insert_relative(format!("{dir_name}/{rel_file}"), contents)?;
        }
        ordered_versions.push(dir_name);
    }

    if !dep_layout.is_empty() {
        emit_dependency_tree(&mut files, &header, &dep_layout, options)?;
    }

    let has_worlds = versions.iter().any(|v| !v.model.worlds.is_empty());
    if has_worlds {
        files.insert_relative("worlds.py", emit_worlds(&header, versions)?)?;
    }

    let mut root_init = String::new();
    writeln!(root_init, "{header}")?;
    writeln!(
        root_init,
        "# source: {}:{}",
        package.namespace, package.name
    )?;
    writeln!(root_init)?;
    writeln!(root_init, "from __future__ import annotations")?;
    writeln!(root_init)?;

    let mut submods: Vec<String> = ordered_versions;
    if !dep_layout.is_empty() {
        submods.push("witforge_deps".to_string());
    }
    submods.sort();
    for sub in &submods {
        writeln!(root_init, "from . import {sub} as {sub}")?;
    }
    writeln!(root_init, "from .witforge_types import (")?;
    for sym in ["Err", "NONE", "NoneValue", "Ok", "Option", "Result", "Some"] {
        writeln!(root_init, "    {sym} as {sym},")?;
    }
    writeln!(root_init, ")")?;
    if has_worlds {
        writeln!(root_init, "from .worlds import (")?;
        writeln!(root_init, "    WORLDS as WORLDS,")?;
        writeln!(root_init, "    WorldMeta as WorldMeta,")?;
        writeln!(root_init, ")")?;
    }

    let mut all_names: Vec<String> = vec![
        "Err".to_string(),
        "NONE".to_string(),
        "NoneValue".to_string(),
        "Ok".to_string(),
        "Option".to_string(),
        "Result".to_string(),
        "Some".to_string(),
    ];
    if has_worlds {
        all_names.push("WORLDS".to_string());
        all_names.push("WorldMeta".to_string());
    }
    all_names.extend(submods);
    all_names.sort();
    writeln!(root_init)?;
    write_all_tuple(&mut root_init, &all_names)?;
    files.insert_relative("__init__.py", root_init)?;

    Ok(files)
}

fn emit_shared_types(header: &str, namespace: &str, name: &str) -> Result<String> {
    let mut out = String::new();
    writeln!(out, "{header}")?;
    writeln!(out, "# source: {namespace}:{name}")?;
    writeln!(out)?;
    writeln!(out, "from __future__ import annotations")?;
    writeln!(out)?;
    writeln!(out, "from dataclasses import dataclass")?;
    writeln!(out, "from typing import Final, Generic, TypeAlias, TypeVar")?;
    writeln!(out)?;
    writeln!(out, "T_co = TypeVar(\"T_co\", covariant=True)")?;
    writeln!(out, "E_co = TypeVar(\"E_co\", covariant=True)")?;
    writeln!(out)?;
    writeln!(out)?;
    writeln!(out, "@dataclass(frozen=True, slots=True)")?;
    writeln!(out, "class Some(Generic[T_co]):")?;
    writeln!(
        out,
        "    \"\"\"Present branch of a nested optional value (`Some(None)` stays distinct from `NONE`).\"\"\""
    )?;
    writeln!(out)?;
    writeln!(out, "    value: T_co")?;
    writeln!(out)?;
    writeln!(out)?;
    writeln!(out, "@dataclass(frozen=True, slots=True)")?;
    writeln!(out, "class NoneValue:")?;
    writeln!(
        out,
        "    \"\"\"Absent branch of a nested optional value.\"\"\""
    )?;
    writeln!(out)?;
    writeln!(out)?;
    writeln!(out, "NONE: Final[NoneValue] = NoneValue()")?;
    writeln!(out)?;
    writeln!(out, "Option: TypeAlias = Some[T_co] | NoneValue")?;
    writeln!(out)?;
    writeln!(out)?;
    writeln!(out, "@dataclass(frozen=True, slots=True)")?;
    writeln!(out, "class Ok(Generic[T_co]):")?;
    writeln!(
        out,
        "    \"\"\"Success branch of a WIT `result` value.\"\"\""
    )?;
    writeln!(out)?;
    writeln!(out, "    value: T_co")?;
    writeln!(out)?;
    writeln!(out)?;
    writeln!(out, "@dataclass(frozen=True, slots=True)")?;
    writeln!(out, "class Err(Generic[E_co]):")?;
    writeln!(out, "    \"\"\"Error branch of a WIT `result` value.\"\"\"")?;
    writeln!(out)?;
    writeln!(out, "    error: E_co")?;
    writeln!(out)?;
    writeln!(out)?;
    writeln!(out, "Result: TypeAlias = Ok[T_co] | Err[E_co]")?;
    writeln!(out)?;
    let all = [
        "Err".to_string(),
        "NONE".to_string(),
        "NoneValue".to_string(),
        "Ok".to_string(),
        "Option".to_string(),
        "Result".to_string(),
        "Some".to_string(),
    ];
    write_all_tuple(&mut out, &all)?;
    Ok(out)
}

#[allow(clippy::too_many_lines)]
fn emit_dependency_tree(
    files: &mut GeneratedFiles,
    header: &str,
    layout: &DepLayout,
    options: &PythonOptions,
) -> Result<()> {
    let mut top_entries: Vec<String> = Vec::new();
    let mut shared_segs: BTreeMap<String, String> = BTreeMap::new();
    let shared_slice: Vec<Dependency> = layout.shared.values().cloned().collect();

    for dependency in &shared_slice {
        let seg = safe_pkg_segment(&dependency.package.namespace, &dependency.package.name)?;
        let full_pkg = format!(
            "{}:{}",
            dependency.package.namespace, dependency.package.name
        );
        if let Some(previous) = shared_segs.insert(seg.clone(), full_pkg.clone()) {
            contract_bail!(
                "dependency packages `{previous}` and `{full_pkg}` both map to Python dependency \
                 directory `{seg}`"
            );
        }
        let dep_model = dependency_model(dependency, &shared_slice);
        let shared_resolver = |ns: &str, pkg: &str| -> Result<String> {
            let other = safe_pkg_segment(ns, pkg)?;
            Ok(format!("..{other}"))
        };
        let emitted = emit_package_interfaces(
            header,
            &dep_model,
            Some(dependency),
            "...witforge_types",
            options,
            &shared_resolver,
        )?;
        for (rel_file, contents) in emitted {
            files.insert_relative(format!("witforge_deps/{seg}/{rel_file}"), contents)?;
        }
        top_entries.push(seg);
    }

    for (&major, scoped_deps) in &layout.per_major {
        let major_dir = polyglot::major_dir_name(major);
        let major_all = layout
            .by_major
            .get(&major)
            .map_or(scoped_deps.as_slice(), Vec::as_slice);
        let mut major_segs: BTreeMap<String, String> = BTreeMap::new();
        let mut major_entries: Vec<String> = Vec::new();

        for dependency in scoped_deps {
            let seg = safe_pkg_segment(&dependency.package.namespace, &dependency.package.name)?;
            let full_pkg = format!(
                "{}:{}",
                dependency.package.namespace, dependency.package.name
            );
            if let Some(previous) = major_segs.insert(seg.clone(), full_pkg.clone()) {
                contract_bail!(
                    "dependency packages `{previous}` and `{full_pkg}` in `{major_dir}` both map \
                     to Python dependency directory `{seg}`"
                );
            }
            let owner = layout.canonical_owner_major(
                major,
                &dependency.package.namespace,
                &dependency.package.name,
            );
            if owner < major {
                let owner_dir = polyglot::major_dir_name(owner);
                let iface_plan = plan_interface_names(&dependency.interfaces, &full_pkg)?;
                let mut pkg_stems: Vec<String> = Vec::with_capacity(iface_plan.len());
                for (interface, stem) in &iface_plan {
                    let plan = plan_interface_symbols(interface)?;
                    let mut exports: Vec<String> = plan.local_type_names.keys().cloned().collect();
                    exports.sort();

                    let mut reexport = String::new();
                    writeln!(reexport, "{header}")?;
                    writeln!(
                        reexport,
                        "# source: {full_pkg}@{} (re-exported from {owner_dir})",
                        dependency.package.version
                    )?;
                    writeln!(reexport)?;
                    writeln!(reexport, "from __future__ import annotations")?;
                    writeln!(reexport)?;
                    if !exports.is_empty() {
                        writeln!(reexport, "from ...{owner_dir}.{seg}.{stem} import (")?;
                        for sym in &exports {
                            writeln!(reexport, "    {sym} as {sym},")?;
                        }
                        writeln!(reexport, ")")?;
                        writeln!(reexport)?;
                    }
                    write_all_tuple(&mut reexport, &exports)?;
                    files.insert_relative(
                        format!("witforge_deps/{major_dir}/{seg}/{stem}.py"),
                        reexport,
                    )?;
                    pkg_stems.push(stem.clone());
                }
                pkg_stems.sort();
                let mut pkg_init = String::new();
                writeln!(pkg_init, "{header}")?;
                writeln!(
                    pkg_init,
                    "# source: {full_pkg}@{} (re-exported from {owner_dir})",
                    dependency.package.version
                )?;
                writeln!(pkg_init)?;
                writeln!(pkg_init, "from __future__ import annotations")?;
                writeln!(pkg_init)?;
                for stem in &pkg_stems {
                    writeln!(pkg_init, "from . import {stem} as {stem}")?;
                }
                if !pkg_stems.is_empty() {
                    writeln!(pkg_init)?;
                }
                write_all_tuple(&mut pkg_init, &pkg_stems)?;
                files.insert_relative(
                    format!("witforge_deps/{major_dir}/{seg}/__init__.py"),
                    pkg_init,
                )?;
            } else {
                let dep_model = dependency_model(dependency, major_all);
                let major_resolver = |ns: &str, pkg: &str| -> Result<String> {
                    let other = safe_pkg_segment(ns, pkg)?;
                    if layout.is_scoped(ns, pkg) {
                        Ok(format!("..{other}"))
                    } else {
                        Ok(format!("...{other}"))
                    }
                };
                let emitted = emit_package_interfaces(
                    header,
                    &dep_model,
                    Some(dependency),
                    "....witforge_types",
                    options,
                    &major_resolver,
                )?;
                for (rel_file, contents) in emitted {
                    files.insert_relative(
                        format!("witforge_deps/{major_dir}/{seg}/{rel_file}"),
                        contents,
                    )?;
                }
            }
            major_entries.push(seg);
        }

        major_entries.sort();
        let mut major_init = String::new();
        writeln!(major_init, "{header}")?;
        writeln!(
            major_init,
            "# source: wit/deps ({major_dir} scoped dependencies)"
        )?;
        writeln!(major_init)?;
        writeln!(major_init, "from __future__ import annotations")?;
        writeln!(major_init)?;
        for seg in &major_entries {
            writeln!(major_init, "from . import {seg} as {seg}")?;
        }
        if !major_entries.is_empty() {
            writeln!(major_init)?;
        }
        write_all_tuple(&mut major_init, &major_entries)?;
        files.insert_relative(format!("witforge_deps/{major_dir}/__init__.py"), major_init)?;
        top_entries.push(major_dir);
    }

    top_entries.sort();
    let mut deps_init = String::new();
    writeln!(deps_init, "{header}")?;
    writeln!(deps_init, "# source: wit/deps")?;
    writeln!(deps_init)?;
    writeln!(deps_init, "from __future__ import annotations")?;
    writeln!(deps_init)?;
    for entry in &top_entries {
        writeln!(deps_init, "from . import {entry} as {entry}")?;
    }
    if !top_entries.is_empty() {
        writeln!(deps_init)?;
    }
    write_all_tuple(&mut deps_init, &top_entries)?;
    files.insert_relative("witforge_deps/__init__.py", deps_init)?;
    Ok(())
}

fn emit_version_files(
    header: &str,
    model: &Model,
    options: &PythonOptions,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
) -> Result<BTreeMap<String, String>> {
    emit_package_interfaces(
        header,
        model,
        None,
        "..witforge_types",
        options,
        foreign_resolver,
    )
}

fn emit_package_interfaces(
    header: &str,
    model: &Model,
    dep_context: Option<&Dependency>,
    shared_types_import: &str,
    _options: &PythonOptions,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
) -> Result<BTreeMap<String, String>> {
    let pkg_label = format!(
        "{}:{}@{}",
        model.package.namespace, model.package.name, model.package.version
    );
    let iface_plan = plan_interface_names(&model.interfaces, &pkg_label)?;
    let stem_by_name: BTreeMap<&str, &str> = iface_plan
        .iter()
        .map(|(iface, stem)| (iface.name.as_str(), stem.as_str()))
        .collect();

    let mut out_files = BTreeMap::new();
    for (interface, stem) in &iface_plan {
        let source = emit_interface_file(
            header,
            model,
            dep_context,
            interface,
            &stem_by_name,
            shared_types_import,
            foreign_resolver,
        )?;
        out_files.insert(format!("{stem}.py"), source);
    }

    let mut pkg_init = String::new();
    writeln!(pkg_init, "{header}")?;
    writeln!(pkg_init, "# source: {pkg_label}")?;
    let pkg_docs = polyglot::doc_lines(&model.package.docs);
    if !pkg_docs.is_empty() {
        write_docstring(&mut pkg_init, 0, &pkg_docs)?;
    }
    writeln!(pkg_init)?;
    writeln!(pkg_init, "from __future__ import annotations")?;
    writeln!(pkg_init)?;

    let mut stems: Vec<String> = iface_plan.iter().map(|(_, stem)| stem.clone()).collect();
    stems.sort();
    for stem in &stems {
        writeln!(pkg_init, "from . import {stem} as {stem}")?;
    }
    if !stems.is_empty() {
        writeln!(pkg_init)?;
    }
    write_all_tuple(&mut pkg_init, &stems)?;
    out_files.insert("__init__.py".to_string(), pkg_init);
    Ok(out_files)
}

fn plan_interface_names<'a>(
    interfaces: &'a [Interface],
    scope: &str,
) -> Result<Vec<(&'a Interface, String)>> {
    let mut stems: BTreeMap<String, String> = BTreeMap::new();
    let mut planned = Vec::with_capacity(interfaces.len());

    for interface in interfaces {
        let stem = safe_interface_file_stem(&interface.name)?;
        let stem_lower = stem.to_ascii_lowercase();
        if let Some(previous) = stems.insert(stem_lower, interface.name.clone()) {
            contract_bail!(
                "{scope}: interfaces `{previous}` and `{}` both map to Python module `{stem}.py`",
                interface.name
            );
        }
        planned.push((interface, stem));
    }
    Ok(planned)
}

struct InterfaceSymbolPlan {
    service_protocol_name: Option<String>,
    type_names: Vec<String>,
    variant_case_names: BTreeMap<(usize, usize), String>,
    local_type_names: BTreeMap<String, String>,
}

fn plan_interface_symbols(interface: &Interface) -> Result<InterfaceSymbolPlan> {
    let mut local_type_names: BTreeMap<String, String> = BTreeMap::new();
    let service_protocol_name = if interface.functions.is_empty() {
        None
    } else {
        let name = py_type_ident(&interface.name, "interface")?;
        local_type_names.insert(name.clone(), format!("interface {}", interface.name));
        Some(name)
    };
    let mut type_names = Vec::with_capacity(interface.types.len());
    for def in &interface.types {
        let name = py_type_ident(&def.name, "type")?;
        insert_name(
            &mut local_type_names,
            &name,
            &format!("type {}/{}", interface.name, def.name),
            &format!("interface `{}`", interface.name),
        )?;
        type_names.push(name);
    }

    let mut variant_case_names = BTreeMap::new();
    for (def_idx, def) in interface.types.iter().enumerate() {
        let TypeDefKind::Variant(cases) = &def.kind else {
            continue;
        };
        let variant_name = &type_names[def_idx];
        let scope = format!("{}/{}", interface.name, def.name);
        let mut seen_cases = BTreeMap::new();
        for (case_idx, case) in cases.iter().enumerate() {
            let case_pascal = to_pascal_case(&case.name);
            if case_pascal.is_empty() {
                contract_bail!(
                    "{scope}: variant case `{}` cannot be represented as a Python class suffix",
                    case.name
                );
            }
            insert_name(&mut seen_cases, &case_pascal, &case.name, &scope)?;
            let preferred = escape_python_ident(&format!("{variant_name}{case_pascal}"));
            let class_name = if local_type_names.contains_key(&preferred) {
                let disambiguated = escape_python_ident(&format!("{variant_name}_{case_pascal}"));
                if local_type_names.contains_key(&disambiguated) {
                    let mut suffix = 2usize;
                    loop {
                        let candidate =
                            escape_python_ident(&format!("{variant_name}_{case_pascal}_{suffix}"));
                        if !local_type_names.contains_key(&candidate) {
                            break candidate;
                        }
                        suffix += 1;
                    }
                } else {
                    disambiguated
                }
            } else {
                preferred
            };
            local_type_names.insert(
                class_name.clone(),
                format!("variant case {}/{}.{}", interface.name, def.name, case.name),
            );
            variant_case_names.insert((def_idx, case_idx), class_name);
        }
    }

    Ok(InterfaceSymbolPlan {
        service_protocol_name,
        type_names,
        variant_case_names,
        local_type_names,
    })
}

#[derive(Default)]
struct SymbolSlot {
    shadowed: bool,
    used: bool,
}

impl SymbolSlot {
    fn new(local_names: &BTreeMap<String, String>, name: &str) -> Self {
        Self {
            shadowed: local_names.contains_key(name),
            used: false,
        }
    }

    fn use_name<'a>(&mut self, plain: &'a str, aliased: &'a str) -> &'a str {
        self.used = true;
        if self.shadowed { aliased } else { plain }
    }
}

/// Imports collected while rendering one interface module.
struct InterfaceImports {
    /// `(from_module, import_member)` -> local alias (`_i_*` or `_d_*`).
    by_target: BTreeMap<(String, String), String>,
    used_aliases: BTreeSet<String>,
    dataclass_used: bool,
    builtins_used: bool,
    str_enum: SymbolSlot,
    mapping: SymbolSlot,
    protocol: SymbolSlot,
    type_alias: SymbolSlot,
    never: SymbolSlot,
    option: SymbolSlot,
    ok: SymbolSlot,
    err: SymbolSlot,
}

impl InterfaceImports {
    fn new(local_type_names: &BTreeMap<String, String>) -> Self {
        Self {
            by_target: BTreeMap::new(),
            used_aliases: BTreeSet::new(),
            dataclass_used: false,
            builtins_used: false,
            str_enum: SymbolSlot::new(local_type_names, "StrEnum"),
            mapping: SymbolSlot::new(local_type_names, "Mapping"),
            protocol: SymbolSlot::new(local_type_names, "Protocol"),
            type_alias: SymbolSlot::new(local_type_names, "TypeAlias"),
            never: SymbolSlot::new(local_type_names, "Never"),
            option: SymbolSlot::new(local_type_names, "Option"),
            ok: SymbolSlot::new(local_type_names, "Ok"),
            err: SymbolSlot::new(local_type_names, "Err"),
        }
    }

    fn alias_for(&mut self, from_module: String, member: String, preferred_base: &str) -> String {
        let key = (from_module, member);
        if let Some(existing) = self.by_target.get(&key) {
            return existing.clone();
        }
        let mut candidate = preferred_base.to_string();
        let mut suffix = 2usize;
        while self.used_aliases.contains(&candidate) {
            candidate = format!("{preferred_base}_{suffix}");
            suffix += 1;
        }
        self.used_aliases.insert(candidate.clone());
        self.by_target.insert(key, candidate.clone());
        candidate
    }

    fn builtin_name(&mut self, name: &'static str, shadowed: &BTreeSet<String>) -> String {
        if shadowed.contains(name) {
            self.builtins_used = true;
            format!("_wf_builtins.{name}")
        } else {
            name.to_string()
        }
    }
}

#[allow(clippy::too_many_lines)]
fn emit_interface_file(
    header: &str,
    model: &Model,
    dep_context: Option<&Dependency>,
    interface: &Interface,
    local_stems: &BTreeMap<&str, &str>,
    shared_types_import: &str,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
) -> Result<String> {
    let symbols = plan_interface_symbols(interface)?;
    let mut imports = InterfaceImports::new(&symbols.local_type_names);
    let mut blocks: Vec<(bool, String)> = Vec::new();

    // Phase 1: nominal classes (`record`, `enum`, `flags`, and `variant` case classes + union).
    // All class field annotations are postponed by `from __future__ import annotations`, so
    // nominal classes can never hit import-time `NameError` on forward references.
    let mut alias_indices: Vec<usize> = Vec::new();
    for (idx, def) in interface.types.iter().enumerate() {
        if is_nominal_type_def(&def.kind) {
            let mut block = String::new();
            emit_type_def(
                &mut block,
                model,
                dep_context,
                interface,
                idx,
                def,
                &symbols,
                local_stems,
                foreign_resolver,
                &mut imports,
            )?;
            blocks.push((true, block));
        } else {
            alias_indices.push(idx);
        }
    }

    // Phase 2: `TypeAlias` definitions in topological order (tie-broken by WIT source index) so
    // aliases referencing nominal classes or other same-interface aliases always evaluate cleanly
    // at module import time.
    let ordered_aliases = order_alias_defs(interface, &alias_indices);
    for idx in ordered_aliases {
        let def = &interface.types[idx];
        let mut block = String::new();
        emit_type_def(
            &mut block,
            model,
            dep_context,
            interface,
            idx,
            def,
            &symbols,
            local_stems,
            foreign_resolver,
            &mut imports,
        )?;
        blocks.push((false, block));
    }

    // Phase 3: service `Protocol` class for function-bearing interfaces.
    if let Some(service_name) = &symbols.service_protocol_name {
        let mut block = String::new();
        emit_service_protocol(
            &mut block,
            model,
            dep_context,
            interface,
            service_name,
            local_stems,
            foreign_resolver,
            &mut imports,
        )?;
        blocks.push((true, block));
    }

    let mut out = String::new();
    writeln!(out, "{header}")?;
    writeln!(
        out,
        "# source: {}:{}@{} (interface {})",
        model.package.namespace, model.package.name, model.package.version, interface.name
    )?;
    let iface_docs = combined_doc_lines(&interface.docs, &interface.stability, None);
    if !iface_docs.is_empty() && symbols.service_protocol_name.is_none() {
        write_docstring(&mut out, 0, &iface_docs)?;
    }
    writeln!(out)?;
    writeln!(out, "from __future__ import annotations")?;

    // Standard library imports (sorted per PEP 8 / isort).
    let mut stdlib_lines: Vec<String> = Vec::new();
    if imports.builtins_used {
        stdlib_lines.push("import builtins as _wf_builtins".to_string());
    }
    if imports.mapping.used {
        if imports.mapping.shadowed {
            stdlib_lines.push("from collections.abc import Mapping as _WfMapping".to_string());
        } else {
            stdlib_lines.push("from collections.abc import Mapping".to_string());
        }
    }
    if imports.dataclass_used {
        stdlib_lines.push("from dataclasses import dataclass".to_string());
    }
    if imports.str_enum.used {
        if imports.str_enum.shadowed {
            stdlib_lines.push("from enum import StrEnum as _WfStrEnum".to_string());
        } else {
            stdlib_lines.push("from enum import StrEnum".to_string());
        }
    }
    let mut typing_specs: Vec<&str> = Vec::new();
    if imports.never.used {
        typing_specs.push(if imports.never.shadowed {
            "Never as _WfNever"
        } else {
            "Never"
        });
    }
    if imports.protocol.used {
        typing_specs.push(if imports.protocol.shadowed {
            "Protocol as _WfProtocol"
        } else {
            "Protocol"
        });
    }
    if imports.type_alias.used {
        typing_specs.push(if imports.type_alias.shadowed {
            "TypeAlias as _WfTypeAlias"
        } else {
            "TypeAlias"
        });
    }
    if !typing_specs.is_empty() {
        stdlib_lines.push(format!("from typing import {}", typing_specs.join(", ")));
    }
    if !stdlib_lines.is_empty() {
        writeln!(out)?;
        for line in stdlib_lines {
            writeln!(out, "{line}")?;
        }
    }

    // Relative local/package imports.
    let mut rel_lines: Vec<String> = Vec::new();
    let mut shared_specs: Vec<&str> = Vec::new();
    if imports.err.used {
        shared_specs.push(if imports.err.shadowed {
            "Err as _WfErr"
        } else {
            "Err"
        });
    }
    if imports.ok.used {
        shared_specs.push(if imports.ok.shadowed {
            "Ok as _WfOk"
        } else {
            "Ok"
        });
    }
    if imports.option.used {
        shared_specs.push(if imports.option.shadowed {
            "Option as _WfOption"
        } else {
            "Option"
        });
    }
    if !shared_specs.is_empty() {
        rel_lines.push(format!(
            "from {shared_types_import} import {}",
            shared_specs.join(", ")
        ));
    }
    for ((from_mod, member), alias) in &imports.by_target {
        rel_lines.push(format!("from {from_mod} import {member} as {alias}"));
    }
    if !rel_lines.is_empty() {
        rel_lines.sort();
        writeln!(out)?;
        for line in rel_lines {
            writeln!(out, "{line}")?;
        }
    }

    let mut prev_was_class = false;
    for (idx, (is_class, block)) in blocks.iter().enumerate() {
        if idx == 0 {
            if *is_class {
                writeln!(out)?;
                writeln!(out)?;
            } else {
                writeln!(out)?;
            }
        } else if *is_class || prev_was_class {
            writeln!(out)?;
            writeln!(out)?;
        } else {
            writeln!(out)?;
        }
        out.push_str(block);
        prev_was_class = *is_class;
    }

    let mut all_names: Vec<String> = symbols.local_type_names.keys().cloned().collect();
    all_names.sort();
    if prev_was_class {
        writeln!(out)?;
        writeln!(out)?;
    } else {
        writeln!(out)?;
    }
    write_all_tuple(&mut out, &all_names)?;

    Ok(out)
}

const fn is_nominal_type_def(kind: &TypeDefKind) -> bool {
    matches!(
        kind,
        TypeDefKind::Record(_)
            | TypeDefKind::Variant(_)
            | TypeDefKind::Enum(_)
            | TypeDefKind::Flags(_)
    )
}

fn collect_same_iface_named_refs(iface_name: &str, ty: &TypeExpr, out: &mut BTreeSet<String>) {
    match ty {
        TypeExpr::Named(q) => {
            if !q.is_foreign() && q.interface == iface_name {
                out.insert(q.name.clone());
            }
        }
        TypeExpr::List(elem) | TypeExpr::FixedLengthList(elem, _) | TypeExpr::Option(elem) => {
            collect_same_iface_named_refs(iface_name, elem, out);
        }
        TypeExpr::Map(k, v) => {
            collect_same_iface_named_refs(iface_name, k, out);
            collect_same_iface_named_refs(iface_name, v, out);
        }
        TypeExpr::Result(res) => {
            if let Some(ok) = &res.ok {
                collect_same_iface_named_refs(iface_name, ok, out);
            }
            if let Some(err) = &res.err {
                collect_same_iface_named_refs(iface_name, err, out);
            }
        }
        _ => {}
    }
}

fn order_alias_defs(interface: &Interface, alias_indices: &[usize]) -> Vec<usize> {
    if alias_indices.len() <= 1 {
        return alias_indices.to_vec();
    }
    let idx_by_wit_name: BTreeMap<&str, usize> = alias_indices
        .iter()
        .map(|&idx| (interface.types[idx].name.as_str(), idx))
        .collect();
    let mut indegree: BTreeMap<usize, usize> = alias_indices.iter().map(|&idx| (idx, 0)).collect();
    let mut dependents: BTreeMap<usize, Vec<usize>> = BTreeMap::new();

    for &idx in alias_indices {
        let mut refs = BTreeSet::new();
        match &interface.types[idx].kind {
            TypeDefKind::Alias(t)
            | TypeDefKind::List(t)
            | TypeDefKind::FixedLengthList(t, _)
            | TypeDefKind::Option(t) => {
                collect_same_iface_named_refs(&interface.name, t, &mut refs);
            }
            TypeDefKind::Map(k, v) => {
                collect_same_iface_named_refs(&interface.name, k, &mut refs);
                collect_same_iface_named_refs(&interface.name, v, &mut refs);
            }
            TypeDefKind::Result(result) => {
                if let Some(ok) = &result.ok {
                    collect_same_iface_named_refs(&interface.name, ok, &mut refs);
                }
                if let Some(err) = &result.err {
                    collect_same_iface_named_refs(&interface.name, err, &mut refs);
                }
            }
            _ => {}
        }
        for dep_name in refs {
            if let Some(&dep_idx) = idx_by_wit_name.get(dep_name.as_str())
                && dep_idx != idx
            {
                *indegree.entry(idx).or_insert(0) += 1;
                dependents.entry(dep_idx).or_default().push(idx);
            }
        }
    }

    // Use a BTreeSet as a min-heap keyed by original WIT source index for deterministic ordering.
    let mut ready: BTreeSet<usize> = indegree
        .iter()
        .filter_map(|(&idx, &deg)| (deg == 0).then_some(idx))
        .collect();
    let mut ordered = Vec::with_capacity(alias_indices.len());
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    while let Some(next) = ready.pop_first() {
        queue.push_back(next);
        while let Some(cur) = queue.pop_front() {
            if !visited.insert(cur) {
                continue;
            }
            ordered.push(cur);
            if let Some(children) = dependents.get(&cur) {
                for &child in children {
                    if let Some(deg) = indegree.get_mut(&child) {
                        *deg = deg.saturating_sub(1);
                        if *deg == 0 {
                            ready.insert(child);
                        }
                    }
                }
            }
        }
    }
    // Fallback for any cyclic artificial model input: preserve remaining source order.
    for &idx in alias_indices {
        if !visited.contains(&idx) {
            ordered.push(idx);
        }
    }
    ordered
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn emit_type_def(
    out: &mut String,
    model: &Model,
    dep_context: Option<&Dependency>,
    interface: &Interface,
    def_idx: usize,
    def: &TypeDef,
    symbols: &InterfaceSymbolPlan,
    local_stems: &BTreeMap<&str, &str>,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut InterfaceImports,
) -> Result<()> {
    let name = &symbols.type_names[def_idx];
    let scope = format!("{}/{}", interface.name, def.name);
    let empty_shadow = BTreeSet::new();

    match &def.kind {
        TypeDefKind::Record(fields) => {
            imports.dataclass_used = true;
            let mut field_names = BTreeMap::new();
            let mut planned_fields = Vec::with_capacity(fields.len());
            let mut shadowed_builtins = BTreeSet::new();
            for field in fields {
                let py_field = py_field_ident(&field.name, "record field")?;
                insert_name(&mut field_names, &py_field, &field.name, &scope)?;
                if is_python_type_builtin(&py_field) {
                    shadowed_builtins.insert(py_field.clone());
                }
                planned_fields.push((field, py_field));
            }

            writeln!(out, "@dataclass(frozen=True, slots=True)")?;
            writeln!(out, "class {name}:")?;
            let docs = combined_doc_lines(&def.docs, &def.stability, None);
            if !docs.is_empty() {
                write_docstring(out, 4, &docs)?;
                if !planned_fields.is_empty() {
                    writeln!(out)?;
                }
            }
            if planned_fields.is_empty() {
                if docs.is_empty() {
                    writeln!(out, "    pass")?;
                }
                return Ok(());
            }
            for (field, py_field) in planned_fields {
                let ty = map_type(
                    model,
                    dep_context,
                    &interface.name,
                    &field.ty,
                    local_stems,
                    foreign_resolver,
                    imports,
                    &shadowed_builtins,
                )?;
                writeln!(out, "    {py_field}: {ty}")?;
                let fdocs = polyglot::doc_lines(&field.docs);
                if !fdocs.is_empty() {
                    write_docstring(out, 4, &fdocs)?;
                }
            }
        }
        TypeDefKind::Variant(cases) => {
            let docs = combined_doc_lines(&def.docs, &def.stability, None);
            let alias_kw = imports.type_alias.use_name("TypeAlias", "_WfTypeAlias");
            if cases.is_empty() {
                write_hash_comments(out, &docs)?;
                let never_kw = imports.never.use_name("Never", "_WfNever");
                writeln!(out, "{name}: {alias_kw} = {never_kw}")?;
                return Ok(());
            }
            imports.dataclass_used = true;
            let mut case_classes = Vec::with_capacity(cases.len());
            for (case_idx, case) in cases.iter().enumerate() {
                if case_idx > 0 {
                    writeln!(out)?;
                    writeln!(out)?;
                }
                let class_name = &symbols.variant_case_names[&(def_idx, case_idx)];
                writeln!(out, "@dataclass(frozen=True, slots=True)")?;
                writeln!(out, "class {class_name}:")?;
                let cdocs = polyglot::doc_lines(&case.docs);
                if !cdocs.is_empty() {
                    write_docstring(out, 4, &cdocs)?;
                    if case.payload.is_some() {
                        writeln!(out)?;
                    }
                }
                if let Some(payload) = &case.payload {
                    let ty = map_type(
                        model,
                        dep_context,
                        &interface.name,
                        payload,
                        local_stems,
                        foreign_resolver,
                        imports,
                        &empty_shadow,
                    )?;
                    writeln!(out, "    value: {ty}")?;
                } else if cdocs.is_empty() {
                    writeln!(out, "    pass")?;
                }
                case_classes.push(class_name.as_str());
            }
            writeln!(out)?;
            writeln!(out)?;
            write_hash_comments(out, &docs)?;
            writeln!(out, "{name}: {alias_kw} = {}", case_classes.join(" | "))?;
        }
        TypeDefKind::Enum(cases) => {
            let base = imports.str_enum.use_name("StrEnum", "_WfStrEnum");
            writeln!(out, "class {name}({base}):")?;
            let docs = combined_doc_lines(&def.docs, &def.stability, None);
            if !docs.is_empty() {
                write_docstring(out, 4, &docs)?;
                if !cases.is_empty() {
                    writeln!(out)?;
                }
            }
            if cases.is_empty() {
                if docs.is_empty() {
                    writeln!(out, "    pass")?;
                }
                return Ok(());
            }
            let mut case_names = BTreeMap::new();
            for case in cases {
                let member = py_enum_member_ident(&case.name)?;
                insert_name(&mut case_names, &member, &case.name, &scope)?;
                let literal = escape_py_string(&case.name);
                writeln!(out, "    {member} = {literal}")?;
                let cdocs = polyglot::doc_lines(&case.docs);
                if !cdocs.is_empty() {
                    write_docstring(out, 4, &cdocs)?;
                }
            }
        }
        TypeDefKind::Flags(flags) => {
            imports.dataclass_used = true;
            let mut flag_names = BTreeMap::new();
            let mut planned_flags = Vec::with_capacity(flags.len());
            let mut shadowed_builtins = BTreeSet::new();
            for flag in flags {
                let py_flag = py_field_ident(&flag.name, "flag")?;
                insert_name(&mut flag_names, &py_flag, &flag.name, &scope)?;
                if is_python_type_builtin(&py_flag) {
                    shadowed_builtins.insert(py_flag.clone());
                }
                planned_flags.push((flag, py_flag));
            }
            let bool_ty = imports.builtin_name("bool", &shadowed_builtins);

            writeln!(out, "@dataclass(frozen=True, slots=True)")?;
            writeln!(out, "class {name}:")?;
            let docs = combined_doc_lines(&def.docs, &def.stability, None);
            if !docs.is_empty() {
                write_docstring(out, 4, &docs)?;
                if !planned_flags.is_empty() {
                    writeln!(out)?;
                }
            }
            if planned_flags.is_empty() {
                if docs.is_empty() {
                    writeln!(out, "    pass")?;
                }
                return Ok(());
            }
            for (flag, py_flag) in planned_flags {
                writeln!(out, "    {py_flag}: {bool_ty} = False")?;
                let fdocs = polyglot::doc_lines(&flag.docs);
                if !fdocs.is_empty() {
                    write_docstring(out, 4, &fdocs)?;
                }
            }
        }
        TypeDefKind::Alias(target) => {
            let docs = combined_doc_lines(&def.docs, &def.stability, None);
            write_hash_comments(out, &docs)?;
            let alias_kw = imports.type_alias.use_name("TypeAlias", "_WfTypeAlias");
            let ty = map_type(
                model,
                dep_context,
                &interface.name,
                target,
                local_stems,
                foreign_resolver,
                imports,
                &empty_shadow,
            )?;
            writeln!(out, "{name}: {alias_kw} = {ty}")?;
        }
        TypeDefKind::List(elem) => {
            let docs = combined_doc_lines(&def.docs, &def.stability, None);
            write_hash_comments(out, &docs)?;
            let alias_kw = imports.type_alias.use_name("TypeAlias", "_WfTypeAlias");
            let ty = map_list_type(
                model,
                dep_context,
                &interface.name,
                elem,
                local_stems,
                foreign_resolver,
                imports,
                &empty_shadow,
            )?;
            writeln!(out, "{name}: {alias_kw} = {ty}")?;
        }
        TypeDefKind::FixedLengthList(elem, len) => {
            let note = format!("Fixed-length list of exactly {len} elements.");
            let combined_raw = if polyglot::doc_lines(&def.docs).is_empty() {
                note
            } else {
                format!("{}\n\n{note}", def.docs)
            };
            let docs = combined_doc_lines(&combined_raw, &def.stability, None);
            write_hash_comments(out, &docs)?;
            let alias_kw = imports.type_alias.use_name("TypeAlias", "_WfTypeAlias");
            let ty = map_list_type(
                model,
                dep_context,
                &interface.name,
                elem,
                local_stems,
                foreign_resolver,
                imports,
                &empty_shadow,
            )?;
            writeln!(out, "{name}: {alias_kw} = {ty}")?;
        }
        TypeDefKind::Option(inner) => {
            let docs = combined_doc_lines(&def.docs, &def.stability, None);
            write_hash_comments(out, &docs)?;
            let alias_kw = imports.type_alias.use_name("TypeAlias", "_WfTypeAlias");
            let ty = map_option_type(
                model,
                dep_context,
                &interface.name,
                inner,
                local_stems,
                foreign_resolver,
                imports,
                &empty_shadow,
            )?;
            writeln!(out, "{name}: {alias_kw} = {ty}")?;
        }
        TypeDefKind::Result(res) => {
            let docs = combined_doc_lines(&def.docs, &def.stability, None);
            write_hash_comments(out, &docs)?;
            let alias_kw = imports.type_alias.use_name("TypeAlias", "_WfTypeAlias");
            let ty = map_result_type(
                model,
                dep_context,
                &interface.name,
                res,
                local_stems,
                foreign_resolver,
                imports,
                &empty_shadow,
            )?;
            writeln!(out, "{name}: {alias_kw} = {ty}")?;
        }
        TypeDefKind::Map(key, value) => {
            let docs = combined_doc_lines(&def.docs, &def.stability, None);
            write_hash_comments(out, &docs)?;
            let alias_kw = imports.type_alias.use_name("TypeAlias", "_WfTypeAlias");
            let mapping_name = imports.mapping.use_name("Mapping", "_WfMapping");
            let key_ty = map_type(
                model,
                dep_context,
                &interface.name,
                key,
                local_stems,
                foreign_resolver,
                imports,
                &empty_shadow,
            )?;
            let val_ty = map_type(
                model,
                dep_context,
                &interface.name,
                value,
                local_stems,
                foreign_resolver,
                imports,
                &empty_shadow,
            )?;
            writeln!(
                out,
                "{name}: {alias_kw} = {mapping_name}[{key_ty}, {val_ty}]"
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_service_protocol(
    out: &mut String,
    model: &Model,
    dep_context: Option<&Dependency>,
    interface: &Interface,
    service_name: &str,
    local_stems: &BTreeMap<&str, &str>,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut InterfaceImports,
) -> Result<()> {
    let mut method_names = BTreeMap::new();
    let mut planned_methods = Vec::with_capacity(interface.functions.len());
    let mut shadowed_builtins = BTreeSet::new();
    for func in &interface.functions {
        let method = py_method_ident(&func.name)?;
        insert_name(
            &mut method_names,
            &method,
            &func.name,
            &format!("interface `{}`", interface.name),
        )?;
        if is_python_type_builtin(&method) {
            shadowed_builtins.insert(method.clone());
        }
        planned_methods.push((func, method));
    }

    let proto_base = imports.protocol.use_name("Protocol", "_WfProtocol");
    writeln!(out, "class {service_name}({proto_base}):")?;
    let fallback = format!("Interface `{}`.", interface.name);
    let iface_docs = combined_doc_lines(&interface.docs, &interface.stability, Some(&fallback));
    write_docstring(out, 4, &iface_docs)?;

    for (func, method) in planned_methods {
        writeln!(out)?;
        let mut param_names = BTreeMap::new();
        let mut rendered_params = Vec::with_capacity(func.params.len() + 1);
        rendered_params.push("self".to_string());
        let fn_scope = format!("{}#{}", interface.name, func.name);
        for param in &func.params {
            let param_ident = py_field_ident(&param.name, "parameter")?;
            insert_name(&mut param_names, &param_ident, &param.name, &fn_scope)?;
            let param_ty = map_type(
                model,
                dep_context,
                &interface.name,
                &param.ty,
                local_stems,
                foreign_resolver,
                imports,
                &shadowed_builtins,
            )?;
            rendered_params.push(format!("{param_ident}: {param_ty}"));
        }
        let ret_ty = match &func.result {
            Some(ty) => map_type(
                model,
                dep_context,
                &interface.name,
                ty,
                local_stems,
                foreign_resolver,
                imports,
                &shadowed_builtins,
            )?,
            None => "None".to_string(),
        };
        writeln!(
            out,
            "    def {method}({}) -> {ret_ty}:",
            rendered_params.join(", ")
        )?;
        let fn_docs = combined_doc_lines(&func.docs, &func.stability, None);
        if !fn_docs.is_empty() {
            write_docstring(out, 8, &fn_docs)?;
        }
        writeln!(out, "        ...")?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn map_type(
    model: &Model,
    dep_context: Option<&Dependency>,
    current_interface: &str,
    ty: &TypeExpr,
    local_stems: &BTreeMap<&str, &str>,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut InterfaceImports,
    shadowed_builtins: &BTreeSet<String>,
) -> Result<String> {
    match ty {
        TypeExpr::Bool => Ok(imports.builtin_name("bool", shadowed_builtins)),
        TypeExpr::U8
        | TypeExpr::U16
        | TypeExpr::U32
        | TypeExpr::U64
        | TypeExpr::S8
        | TypeExpr::S16
        | TypeExpr::S32
        | TypeExpr::S64 => Ok(imports.builtin_name("int", shadowed_builtins)),
        TypeExpr::F32 | TypeExpr::F64 => Ok(imports.builtin_name("float", shadowed_builtins)),
        TypeExpr::Char | TypeExpr::String => Ok(imports.builtin_name("str", shadowed_builtins)),
        TypeExpr::Named(name) => map_named_type(
            model,
            dep_context,
            current_interface,
            name,
            local_stems,
            foreign_resolver,
            imports,
        ),
        TypeExpr::List(elem) | TypeExpr::FixedLengthList(elem, _) => map_list_type(
            model,
            dep_context,
            current_interface,
            elem,
            local_stems,
            foreign_resolver,
            imports,
            shadowed_builtins,
        ),
        TypeExpr::Option(inner) => map_option_type(
            model,
            dep_context,
            current_interface,
            inner,
            local_stems,
            foreign_resolver,
            imports,
            shadowed_builtins,
        ),
        TypeExpr::Result(res) => map_result_type(
            model,
            dep_context,
            current_interface,
            res,
            local_stems,
            foreign_resolver,
            imports,
            shadowed_builtins,
        ),
        TypeExpr::Map(key, value) => {
            let mapping_name = imports.mapping.use_name("Mapping", "_WfMapping");
            let key_ty = map_type(
                model,
                dep_context,
                current_interface,
                key,
                local_stems,
                foreign_resolver,
                imports,
                shadowed_builtins,
            )?;
            let val_ty = map_type(
                model,
                dep_context,
                current_interface,
                value,
                local_stems,
                foreign_resolver,
                imports,
                shadowed_builtins,
            )?;
            Ok(format!("{mapping_name}[{key_ty}, {val_ty}]"))
        }
    }
}

fn map_named_type(
    model: &Model,
    dep_context: Option<&Dependency>,
    current_interface: &str,
    name: &QualifiedName,
    local_stems: &BTreeMap<&str, &str>,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut InterfaceImports,
) -> Result<String> {
    if let Some((namespace, package)) = name.package() {
        model.type_def_in(dep_context, name).with_context(|| {
            format!(
                "named dependency type `{}/{}` is not in the model",
                name.interface, name.name
            )
        })?;
        let pkg_mod = foreign_resolver(namespace, package)?;
        let iface_stem = safe_interface_file_stem(name.interface_name())?;
        let base_alias = format!(
            "_d_{}_{}",
            dep_package_segment(namespace, package),
            iface_stem
        );
        let alias = imports.alias_for(pkg_mod, iface_stem, &base_alias);
        let type_ident = py_type_ident(&name.name, "type")?;
        return Ok(format!("{alias}.{type_ident}"));
    }

    model.type_def_in(dep_context, name).with_context(|| {
        format!(
            "named type `{}/{}` is not in the model",
            name.interface, name.name
        )
    })?;
    let type_ident = py_type_ident(&name.name, "type")?;
    if name.interface == current_interface {
        Ok(type_ident)
    } else {
        let target_stem = local_stems
            .get(name.interface.as_str())
            .copied()
            .map(str::to_string)
            .map_or_else(|| safe_interface_file_stem(&name.interface), Ok)?;
        let base_alias = format!("_i_{target_stem}");
        let alias = imports.alias_for(".".to_string(), target_stem, &base_alias);
        Ok(format!("{alias}.{type_ident}"))
    }
}

#[allow(clippy::too_many_arguments)]
fn map_list_type(
    model: &Model,
    dep_context: Option<&Dependency>,
    current_interface: &str,
    elem: &TypeExpr,
    local_stems: &BTreeMap<&str, &str>,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut InterfaceImports,
    shadowed_builtins: &BTreeSet<String>,
) -> Result<String> {
    if model.resolves_to_u8_in(dep_context, elem) {
        return Ok(imports.builtin_name("bytes", shadowed_builtins));
    }
    let tuple_name = imports.builtin_name("tuple", shadowed_builtins);
    let inner = map_type(
        model,
        dep_context,
        current_interface,
        elem,
        local_stems,
        foreign_resolver,
        imports,
        shadowed_builtins,
    )?;
    Ok(format!("{tuple_name}[{inner}, ...]"))
}

#[allow(clippy::too_many_arguments)]
fn map_option_type(
    model: &Model,
    dep_context: Option<&Dependency>,
    current_interface: &str,
    inner: &TypeExpr,
    local_stems: &BTreeMap<&str, &str>,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut InterfaceImports,
    shadowed_builtins: &BTreeSet<String>,
) -> Result<String> {
    let inner_ty = map_type(
        model,
        dep_context,
        current_interface,
        inner,
        local_stems,
        foreign_resolver,
        imports,
        shadowed_builtins,
    )?;
    if model.resolves_to_option_in(dep_context, inner) {
        let opt = imports.option.use_name("Option", "_WfOption");
        Ok(format!("{opt}[{inner_ty}]"))
    } else {
        Ok(format!("{inner_ty} | None"))
    }
}

#[allow(clippy::too_many_arguments)]
fn map_result_type(
    model: &Model,
    dep_context: Option<&Dependency>,
    current_interface: &str,
    res: &crate::model::ResultExpr,
    local_stems: &BTreeMap<&str, &str>,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut InterfaceImports,
    shadowed_builtins: &BTreeSet<String>,
) -> Result<String> {
    let ok_name = imports.ok.use_name("Ok", "_WfOk");
    let err_name = imports.err.use_name("Err", "_WfErr");
    let ok_ty = match &res.ok {
        Some(ty) => map_type(
            model,
            dep_context,
            current_interface,
            ty,
            local_stems,
            foreign_resolver,
            imports,
            shadowed_builtins,
        )?,
        None => "None".to_string(),
    };
    let err_ty = match &res.err {
        Some(ty) => map_type(
            model,
            dep_context,
            current_interface,
            ty,
            local_stems,
            foreign_resolver,
            imports,
            shadowed_builtins,
        )?,
        None => "None".to_string(),
    };
    Ok(format!("{ok_name}[{ok_ty}] | {err_name}[{err_ty}]"))
}

fn emit_worlds(header: &str, versions: &[Version]) -> Result<String> {
    let Some(first) = versions.first() else {
        bail!("no version directories found under WIT root");
    };
    let mut out = String::new();
    writeln!(out, "{header}")?;
    writeln!(
        out,
        "# source: {}:{}",
        first.model.package.namespace, first.model.package.name
    )?;
    writeln!(out)?;
    writeln!(out, "from __future__ import annotations")?;
    writeln!(out)?;
    writeln!(out, "from collections.abc import Mapping")?;
    writeln!(out, "from dataclasses import dataclass")?;
    writeln!(out, "from types import MappingProxyType")?;
    writeln!(out, "from typing import Final")?;
    writeln!(out)?;
    writeln!(out)?;
    writeln!(out, "@dataclass(frozen=True, slots=True)")?;
    writeln!(out, "class WorldMeta:")?;
    writeln!(
        out,
        "    \"\"\"One contract world and the interfaces it imports and exports.\"\"\""
    )?;
    writeln!(out)?;
    writeln!(out, "    name: str")?;
    writeln!(out, "    imports: tuple[str, ...]")?;
    writeln!(out, "    exports: tuple[str, ...]")?;
    writeln!(out)?;
    writeln!(out)?;
    writeln!(
        out,
        "WORLDS: Final[Mapping[str, tuple[WorldMeta, ...]]] = MappingProxyType("
    )?;
    writeln!(out, "    {{")?;
    for version in versions {
        let dir_name = version_dir_name(&version.model);
        write_version_worlds_comments(&mut out, version)?;
        if version.model.worlds.is_empty() {
            writeln!(out, "        {}: (),", escape_py_string(&dir_name))?;
            continue;
        }
        writeln!(out, "        {}: (", escape_py_string(&dir_name))?;
        for world in &version.model.worlds {
            writeln!(out, "            WorldMeta(")?;
            writeln!(
                out,
                "                name={},",
                escape_py_string(&world.name)
            )?;
            writeln!(
                out,
                "                imports={},",
                format_py_str_tuple(&world.imports)
            )?;
            writeln!(
                out,
                "                exports={},",
                format_py_str_tuple(&world.exports)
            )?;
            writeln!(out, "            ),")?;
        }
        writeln!(out, "        ),")?;
    }
    writeln!(out, "    }}")?;
    writeln!(out, ")")?;
    writeln!(out)?;
    let all = ["WORLDS".to_string(), "WorldMeta".to_string()];
    write_all_tuple(&mut out, &all)?;
    Ok(out)
}

fn write_version_worlds_comments(out: &mut String, version: &Version) -> Result<()> {
    writeln!(
        out,
        "        # The worlds of {}.",
        version.model.package.version
    )?;
    for world in &version.model.worlds {
        let world_lines = polyglot::doc_lines(&world.docs);
        if !world_lines.is_empty() {
            writeln!(out, "        # World `{}`:", world.name)?;
            for line in world_lines {
                if line.is_empty() {
                    writeln!(out, "        #")?;
                } else {
                    writeln!(out, "        # {line}")?;
                }
            }
        }
        if let Some(stability) = stability_doc(&world.stability) {
            writeln!(
                out,
                "        # World `{}` stability: {stability}",
                world.name
            )?;
        }
        for (side, interfaces) in [("import", &world.imports), ("export", &world.exports)] {
            for iface in interfaces {
                let qname = iface.qualified_name();
                let iface_docs = polyglot::doc_lines(&iface.docs);
                if !iface_docs.is_empty() {
                    writeln!(
                        out,
                        "        # World `{}` {side} `{qname}`: {}",
                        world.name,
                        iface_docs.join(" ")
                    )?;
                }
                if let Some(stability) = stability_doc(&iface.stability) {
                    writeln!(
                        out,
                        "        # World `{}` {side} `{qname}` stability: {stability}",
                        world.name
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn format_py_str_tuple(items: &[crate::model::WorldInterface]) -> String {
    match items.len() {
        0 => "()".to_string(),
        1 => format!("({},)", escape_py_string(&items[0].qualified_name())),
        _ => {
            let rendered: Vec<String> = items
                .iter()
                .map(|iface| escape_py_string(&iface.qualified_name()))
                .collect();
            format!("({})", rendered.join(", "))
        }
    }
}

fn write_all_tuple(out: &mut String, names: &[String]) -> Result<()> {
    if names.is_empty() {
        writeln!(out, "__all__ = ()")?;
        return Ok(());
    }
    writeln!(out, "__all__ = (")?;
    for name in names {
        writeln!(out, "    {},", escape_py_string(name))?;
    }
    writeln!(out, ")")?;
    Ok(())
}

pub(crate) fn safe_interface_file_stem(wit_name: &str) -> Result<String> {
    let mut stem = to_snake_case(wit_name);
    if stem.is_empty() || stem.as_bytes()[0].is_ascii_digit() {
        contract_bail!(
            "WIT interface name `{wit_name}` produces an invalid Python module name (`{stem}`)"
        );
    }
    if is_python_keyword(&stem) || is_windows_reserved_stem(&stem) {
        stem.push('_');
    }
    Ok(stem)
}

pub(crate) fn safe_pkg_segment(namespace: &str, name: &str) -> Result<String> {
    let ns_snake = to_snake_case(namespace);
    let name_snake = to_snake_case(name);
    let seg = dep_package_segment(namespace, name);
    if ns_snake.is_empty()
        || name_snake.is_empty()
        || seg.is_empty()
        || seg == "_"
        || seg.as_bytes()[0].is_ascii_digit()
        || is_python_keyword(&seg)
        || is_windows_reserved_stem(&seg)
    {
        contract_bail!(
            "dependency package `{namespace}:{name}` produces invalid Python package directory `{seg}`"
        );
    }
    Ok(seg)
}

pub(crate) fn py_type_ident(wit_name: &str, what: &str) -> Result<String> {
    let pascal = to_pascal_case(wit_name);
    if pascal.is_empty() || pascal.as_bytes()[0].is_ascii_digit() {
        contract_bail!(
            "WIT {what} `{wit_name}` cannot be represented as a Python identifier (`{pascal}`)"
        );
    }
    Ok(escape_python_ident(&pascal))
}

pub(crate) fn py_field_ident(wit_name: &str, what: &str) -> Result<String> {
    let snake = to_snake_case(wit_name);
    if snake.is_empty() || snake.as_bytes()[0].is_ascii_digit() {
        contract_bail!(
            "WIT {what} `{wit_name}` cannot be represented as a Python identifier (`{snake}`)"
        );
    }
    if snake == "self" {
        return Ok("self_".to_string());
    }
    Ok(escape_python_ident(&snake))
}

pub(crate) fn py_method_ident(wit_name: &str) -> Result<String> {
    let snake = to_snake_case(wit_name);
    if snake.is_empty() || snake.as_bytes()[0].is_ascii_digit() {
        contract_bail!(
            "WIT function `{wit_name}` cannot be represented as a Python identifier (`{snake}`)"
        );
    }
    Ok(escape_python_ident(&snake))
}

pub(crate) fn py_enum_member_ident(wit_name: &str) -> Result<String> {
    let upper = to_upper_snake_case(wit_name);
    if upper.is_empty() || upper.as_bytes()[0].is_ascii_digit() {
        contract_bail!(
            "WIT enum case `{wit_name}` cannot be represented as a Python identifier (`{upper}`)"
        );
    }
    Ok(escape_python_ident(&upper))
}

fn is_python_type_builtin(ident: &str) -> bool {
    matches!(ident, "bool" | "int" | "float" | "str" | "bytes" | "tuple")
}

fn insert_name(
    names: &mut BTreeMap<String, String>,
    ident: &str,
    source: &str,
    scope: &str,
) -> Result<()> {
    if let Some(previous) = names.insert(ident.to_string(), source.to_string()) {
        contract_bail!(
            "{scope}: Python name `{ident}` is produced by both `{previous}` and `{source}`"
        );
    }
    Ok(())
}

fn combined_doc_lines(
    raw_docs: &str,
    stability: &Stability,
    fallback: Option<&str>,
) -> Vec<String> {
    let mut lines = polyglot::doc_lines(raw_docs);
    if lines.is_empty()
        && let Some(fb) = fallback
    {
        lines.push(fb.to_string());
    }
    if let Some(stab) = stability_doc(stability) {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(stab);
    }
    lines
}

fn write_hash_comments(out: &mut String, lines: &[String]) -> Result<()> {
    for line in lines {
        if line.is_empty() {
            writeln!(out, "#")?;
        } else {
            writeln!(out, "# {line}")?;
        }
    }
    Ok(())
}

fn escape_docstring_body(line: &str) -> String {
    let escaped = line.replace('\\', "\\\\");
    // Break every triple-quote run (`"""`, `""""`, ...) so it can never terminate a docstring.
    let mut out = String::with_capacity(escaped.len());
    let mut quotes = 0usize;
    for ch in escaped.chars() {
        if ch == '"' {
            quotes += 1;
            if quotes == 3 {
                out.push_str("\\\"");
                quotes = 0;
            } else {
                out.push('"');
            }
        } else {
            quotes = 0;
            out.push(ch);
        }
    }
    out
}

fn ends_with_escaped_quote(text: &str) -> bool {
    let Some(prefix) = text.strip_suffix('"') else {
        return false;
    };
    let backslashes = prefix.bytes().rev().take_while(|&b| b == b'\\').count();
    backslashes % 2 == 1
}

fn escape_docstring_edges(mut text: String, escape_leading: bool, escape_trailing: bool) -> String {
    if escape_leading && text.starts_with('"') {
        text.insert(0, '\\');
    }
    if escape_trailing && text.ends_with('"') && !ends_with_escaped_quote(&text) {
        let len = text.len();
        text.insert(len - 1, '\\');
    }
    text
}

fn write_docstring(out: &mut String, indent: usize, lines: &[String]) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let prefix = " ".repeat(indent);
    if lines.len() == 1 {
        let escaped = escape_docstring_edges(escape_docstring_body(&lines[0]), true, true);
        writeln!(out, "{prefix}\"\"\"{escaped}\"\"\"")?;
        return Ok(());
    }
    for (idx, line) in lines.iter().enumerate() {
        if idx == 0 {
            let escaped = escape_docstring_edges(escape_docstring_body(line), true, false);
            writeln!(out, "{prefix}\"\"\"{escaped}")?;
        } else if line.is_empty() {
            writeln!(out)?;
        } else {
            let escaped = escape_docstring_body(line);
            writeln!(out, "{prefix}{escaped}")?;
        }
    }
    writeln!(out, "{prefix}\"\"\"")?;
    Ok(())
}

pub(crate) fn escape_py_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7F => {
                let _ = write!(out, "\\x{:02x}", c as u32);
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

    #[test]
    fn reserved_windows_and_keyword_stems_are_escaped() {
        assert_eq!(safe_interface_file_stem("types").unwrap(), "types");
        assert_eq!(safe_interface_file_stem("my-api").unwrap(), "my_api");
        assert_eq!(safe_interface_file_stem("class").unwrap(), "class_");
        assert_eq!(safe_interface_file_stem("from").unwrap(), "from_");
        assert_eq!(safe_interface_file_stem("type").unwrap(), "type_");
        assert_eq!(safe_interface_file_stem("match").unwrap(), "match_");
        assert_eq!(safe_interface_file_stem("con").unwrap(), "con_");
        assert_eq!(safe_interface_file_stem("nul").unwrap(), "nul_");
        assert_eq!(safe_interface_file_stem("com1").unwrap(), "com1_");
        assert_eq!(safe_interface_file_stem("lpt9").unwrap(), "lpt9_");
    }

    #[test]
    fn python_ident_helpers_escape_keywords_and_self() {
        assert_eq!(py_type_ident("none", "type").unwrap(), "None_");
        assert_eq!(py_type_ident("true", "type").unwrap(), "True_");
        assert_eq!(py_type_ident("false", "type").unwrap(), "False_");
        assert_eq!(
            py_type_ident("kitchen-sink", "type").unwrap(),
            "KitchenSink"
        );
        assert_eq!(py_field_ident("self", "record field").unwrap(), "self_");
        assert_eq!(py_field_ident("class", "record field").unwrap(), "class_");
        assert_eq!(py_field_ident("match", "record field").unwrap(), "match_");
        assert_eq!(py_field_ident("type", "record field").unwrap(), "type_");
        assert_eq!(py_method_ident("from").unwrap(), "from_");
        assert_eq!(py_method_ident("self").unwrap(), "self");
        assert_eq!(py_enum_member_ident("in-progress").unwrap(), "IN_PROGRESS");
    }

    #[test]
    fn docstring_escaping_handles_triple_quotes_edges_and_backslashes() {
        let mut single = String::new();
        write_docstring(
            &mut single,
            4,
            &["\"quoted\" with \"\"\"triple\"\"\" and trailing \\\"".to_string()],
        )
        .unwrap();
        assert_eq!(
            single,
            "    \"\"\"\\\"quoted\" with \"\"\\\"triple\"\"\\\" and trailing \\\\\\\"\"\"\"\n"
        );

        let mut multi = String::new();
        write_docstring(
            &mut multi,
            4,
            &[
                "\"Leading quote.".to_string(),
                String::new(),
                "Body with \"\"\"\" and C:\\path\\".to_string(),
            ],
        )
        .unwrap();
        assert!(multi.starts_with("    \"\"\"\\\"Leading quote.\n\n"));
        assert!(multi.contains("Body with \"\"\\\"\" and C:\\\\path\\\\\n"));
        assert!(multi.ends_with("    \"\"\"\n"));
    }
}
