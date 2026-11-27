//! Deterministic strict ESM TypeScript plain types and interface emitter (`W13-002`).
//!
//! Emits zero-dependency TypeScript (`5.0+` / `6.0+`, `strict: true`, `verbatimModuleSyntax: true`,
//! explicit `.js` relative ESM specifiers) directly from the owned [`Model`] per
//! `docs/design/polyglot-emitters.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::emit::output::{
    GeneratedFileFinding, GeneratedFiles, ensure_outside_wit_root, is_windows_reserved_stem,
};
use crate::emit::polyglot::{
    self, TargetLanguage, dep_dir_path, dep_package_segment, escape_typescript_ident,
    to_camel_case, to_pascal_case, to_snake_case,
};
use crate::emit::stability_doc;
use crate::emit::versions::{DepLayout, Version, load_versions_with_released};
use crate::model::{
    Dependency, Interface, Model, QualifiedName, Stability, TypeDef, TypeDefKind, TypeExpr,
};

/// Options controlling plain TypeScript code generation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TypeScriptOptions {}

impl TypeScriptOptions {
    /// Create default TypeScript emission options.
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

pub(crate) fn header_for(namespace: &str, name: &str) -> String {
    TargetLanguage::TypeScript.plain_header(namespace, name)
}

/// The TypeScript directory name for one contract version: `v<major>_<minor>`.
#[must_use]
pub fn version_dir_name(model: &Model) -> String {
    polyglot::version_dir_name(model.package.version.major, model.package.version.minor)
}

/// Plan the complete nested TypeScript output for a WIT root without writing anything.
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
    plan_with_options(wit_root, None, &TypeScriptOptions::new())
}

/// Plan the TypeScript output for `wit_root`, using an optional explicit released-version manifest
/// path (`--released`) when determining retired versions.
///
/// # Errors
///
/// Returns the same errors as [`plan`].
pub fn plan_with_released(wit_root: &Path, released: Option<&Path>) -> Result<GeneratedFiles> {
    plan_with_options(wit_root, released, &TypeScriptOptions::new())
}

/// Plan the TypeScript output for `wit_root` with an optional explicit released-version manifest
/// path and explicit [`TypeScriptOptions`].
///
/// # Errors
///
/// Returns the same errors as [`plan`].
pub fn plan_with_options(
    wit_root: &Path,
    released: Option<&Path>,
    options: &TypeScriptOptions,
) -> Result<GeneratedFiles> {
    plan_versions_with_options(&load_versions_with_released(wit_root, released)?, options)
}

/// Generate TypeScript output under `out`.
///
/// # Errors
///
/// Returns an error for invalid input, an output root inside the WIT tree, a foreign or unowned
/// file collision, or an I/O failure.
pub fn generate(wit_root: &Path, out: &Path) -> Result<()> {
    generate_with_options(wit_root, out, &TypeScriptOptions::new())
}

/// Generate TypeScript output under `out` with explicit [`TypeScriptOptions`].
///
/// # Errors
///
/// Returns the same errors as [`generate`].
pub fn generate_with_options(
    wit_root: &Path,
    out: &Path,
    options: &TypeScriptOptions,
) -> Result<()> {
    ensure_outside_wit_root(wit_root, out)?;
    plan_with_options(wit_root, None, options)?.write(out)
}

/// Check generated TypeScript output under `out` without writing.
///
/// Returns one structured finding per missing, changed, extra, symlinked, or unowned file.
///
/// # Errors
///
/// Returns an error for invalid input, an output root inside the WIT tree, a foreign-owned file,
/// or an I/O failure.
pub fn check(wit_root: &Path, out: &Path) -> Result<Vec<GeneratedFileFinding>> {
    check_with_options(wit_root, out, &TypeScriptOptions::new())
}

/// Check generated TypeScript output under `out` with explicit [`TypeScriptOptions`].
///
/// # Errors
///
/// Returns the same errors as [`check`].
pub fn check_with_options(
    wit_root: &Path,
    out: &Path,
    options: &TypeScriptOptions,
) -> Result<Vec<GeneratedFileFinding>> {
    ensure_outside_wit_root(wit_root, out)?;
    plan_with_options(wit_root, None, options)?.check(out)
}

/// Emit the per-interface TypeScript files and barrel `index.ts` for one single contract version.
///
/// Keys are relative to the version directory (for example `types.ts` and `index.ts`). Emitted
/// interface files import shared `Option` helpers from `../witforge_types.js` and pinned
/// dependencies from `../witforge_deps/...`; use [`plan`] (or [`generate`]) for the complete
/// self-contained TypeScript tree including `witforge_types.ts`, `witforge_deps/`, `worlds.ts`,
/// and the root `index.ts`.
///
/// # Errors
///
/// Returns a [`crate::ContractError`] when a WIT name cannot be represented in TypeScript or two
/// generated names collide, and a plain error when a named type is missing from `model`.
pub fn emit_version(model: &Model) -> Result<BTreeMap<String, String>> {
    emit_version_with_options(model, &TypeScriptOptions::new())
}

/// Emit the per-interface TypeScript files and barrel `index.ts` for one single contract version
/// with explicit [`TypeScriptOptions`].
///
/// # Errors
///
/// Returns the same errors as [`emit_version`].
pub fn emit_version_with_options(
    model: &Model,
    options: &TypeScriptOptions,
) -> Result<BTreeMap<String, String>> {
    let header = header_for(&model.package.namespace, &model.package.name);
    let resolver = |ns: &str, pkg: &str| -> Result<String> {
        let seg = safe_pkg_segment(ns, pkg)?;
        Ok(format!("../witforge_deps/{seg}"))
    };
    emit_version_files(&header, model, options, &resolver)
}

#[allow(clippy::too_many_lines)]
pub(crate) fn plan_versions_with_options(
    versions: &[Version],
    options: &TypeScriptOptions,
) -> Result<GeneratedFiles> {
    let dep_layout = DepLayout::from_versions(versions)?;
    let package = &versions
        .first()
        .context("no version directories found under WIT root")?
        .model
        .package;
    let header = header_for(&package.namespace, &package.name);
    let mut files = GeneratedFiles::new_nested(&header);

    files.insert_relative(
        "witforge_types.ts",
        emit_shared_types(&header, &package.namespace, &package.name)?,
    )?;

    let mut version_dirs: BTreeMap<String, &std::path::Path> = BTreeMap::new();
    let mut ordered_versions: Vec<(String, String)> = Vec::new();
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
            let _ = safe_pkg_segment(ns, pkg)?;
            let root_major = dep_layout.is_scoped(ns, pkg).then_some(major);
            Ok(format!("../{}", dep_dir_path(root_major, ns, pkg)))
        };
        let emitted = emit_version_files(&header, &version.model, options, &resolver)
            .with_context(|| version.directory.display().to_string())?;
        for (rel_file, contents) in emitted {
            files.insert_relative(format!("{dir_name}/{rel_file}"), contents)?;
        }
        ordered_versions.push((
            dir_name,
            format!("Contract version `{}`.", version.model.package.version),
        ));
    }

    if !dep_layout.is_empty() {
        emit_dependency_tree(&mut files, &header, &dep_layout, options)?;
    }

    let has_worlds = versions.iter().any(|v| !v.model.worlds.is_empty());
    if has_worlds {
        files.insert_relative("worlds.ts", emit_worlds(&header, versions)?)?;
    }

    // Root barrel `index.ts`
    let mut root_index = String::new();
    writeln!(root_index, "{header}")?;
    writeln!(
        root_index,
        "// source: {}:{}",
        package.namespace, package.name
    )?;
    writeln!(root_index)?;
    writeln!(
        root_index,
        "export type {{ Option }} from \"./witforge_types.js\";"
    )?;
    writeln!(
        root_index,
        "export {{ NONE, some }} from \"./witforge_types.js\";"
    )?;
    for (dir_name, doc) in &ordered_versions {
        writeln!(root_index)?;
        writeln!(root_index, "/** {doc} */")?;
        writeln!(
            root_index,
            "export * as {dir_name} from \"./{dir_name}/index.js\";"
        )?;
    }
    if !dep_layout.is_empty() {
        writeln!(root_index)?;
        writeln!(root_index, "/** Pinned dependency types. */")?;
        writeln!(
            root_index,
            "export * as witforge_deps from \"./witforge_deps/index.js\";"
        )?;
    }
    if has_worlds {
        writeln!(root_index)?;
        writeln!(root_index, "/** World metadata. */")?;
        writeln!(
            root_index,
            "export type {{ WorldMeta }} from \"./worlds.js\";"
        )?;
        writeln!(root_index, "export {{ WORLDS }} from \"./worlds.js\";")?;
    }
    files.insert_relative("index.ts", root_index)?;

    Ok(files)
}

fn emit_shared_types(header: &str, namespace: &str, name: &str) -> Result<String> {
    let mut out = String::new();
    writeln!(out, "{header}")?;
    writeln!(out, "// source: {namespace}:{name}")?;
    writeln!(out)?;
    writeln!(
        out,
        "/** Tagged optional value preserving the distinction between `none` and `some(none)`. */"
    )?;
    writeln!(out, "export type Option<T> =")?;
    writeln!(out, "  | {{ readonly tag: \"none\" }}")?;
    writeln!(out, "  | {{ readonly tag: \"some\"; readonly value: T }};")?;
    writeln!(out)?;
    writeln!(out, "/** Shared `none` Option sentinel. */")?;
    writeln!(
        out,
        "export const NONE: {{ readonly tag: \"none\" }} = Object.freeze({{ tag: \"none\" }});"
    )?;
    writeln!(out)?;
    writeln!(out, "/** Construct a `some` Option value. */")?;
    writeln!(
        out,
        "export function some<T>(value: T): {{ readonly tag: \"some\"; readonly value: T }} {{"
    )?;
    writeln!(out, "  return {{ tag: \"some\", value }};")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "/** Internal alias used when a WIT interface defines a type named `Uint8Array`. */"
    )?;
    writeln!(out, "export type _WitforgeBytes = Uint8Array;")?;
    writeln!(out)?;
    writeln!(
        out,
        "/** Internal alias used when a WIT interface defines a type named `ReadonlyMap`. */"
    )?;
    writeln!(
        out,
        "export type _WitforgeReadonlyMap<K, V> = ReadonlyMap<K, V>;"
    )?;
    Ok(out)
}

#[allow(clippy::too_many_lines)]
fn emit_dependency_tree(
    files: &mut GeneratedFiles,
    header: &str,
    layout: &DepLayout,
    options: &TypeScriptOptions,
) -> Result<()> {
    let mut top_entries: Vec<(String, String)> = Vec::new();
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
                "dependency packages `{previous}` and `{full_pkg}` both map to TypeScript \
                 dependency directory `{seg}`"
            );
        }
        let dep_model = dependency_model(dependency, &shared_slice);
        let shared_resolver = |ns: &str, pkg: &str| -> Result<String> {
            let other = safe_pkg_segment(ns, pkg)?;
            Ok(format!("../{other}"))
        };
        let emitted = emit_package_interfaces(
            header,
            &dep_model,
            Some(dependency),
            "../../witforge_types.js",
            options,
            &shared_resolver,
        )?;
        for (rel_file, contents) in emitted {
            files.insert_relative(format!("witforge_deps/{seg}/{rel_file}"), contents)?;
        }
        top_entries.push((
            seg.clone(),
            format!(
                "Dependency package `{}:{}`.",
                dependency.package.namespace, dependency.package.name
            ),
        ));
    }

    for (&major, scoped_deps) in &layout.per_major {
        let major_dir = polyglot::major_dir_name(major);
        let major_all = layout
            .by_major
            .get(&major)
            .map_or(scoped_deps.as_slice(), Vec::as_slice);
        let mut major_segs: BTreeMap<String, String> = BTreeMap::new();
        let mut major_entries: Vec<(String, String)> = Vec::new();

        for dependency in scoped_deps {
            let seg = safe_pkg_segment(&dependency.package.namespace, &dependency.package.name)?;
            let full_pkg = format!(
                "{}:{}",
                dependency.package.namespace, dependency.package.name
            );
            if let Some(previous) = major_segs.insert(seg.clone(), full_pkg.clone()) {
                contract_bail!(
                    "dependency packages `{previous}` and `{full_pkg}` in `{major_dir}` both map \
                     to TypeScript dependency directory `{seg}`"
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
                for (_, stem, _) in &iface_plan {
                    let mut reexport = String::new();
                    writeln!(reexport, "{header}")?;
                    writeln!(
                        reexport,
                        "// source: {full_pkg}@{} (re-exported from {owner_dir})",
                        dependency.package.version
                    )?;
                    writeln!(reexport)?;
                    writeln!(
                        reexport,
                        "export * from \"../../{owner_dir}/{seg}/{stem}.js\";"
                    )?;
                    files.insert_relative(
                        format!("witforge_deps/{major_dir}/{seg}/{stem}.ts"),
                        reexport,
                    )?;
                }
                let mut pkg_index = String::new();
                writeln!(pkg_index, "{header}")?;
                writeln!(
                    pkg_index,
                    "// source: {full_pkg}@{} (re-exported from {owner_dir})",
                    dependency.package.version
                )?;
                writeln!(pkg_index)?;
                writeln!(
                    pkg_index,
                    "export * from \"../../{owner_dir}/{seg}/index.js\";"
                )?;
                files.insert_relative(
                    format!("witforge_deps/{major_dir}/{seg}/index.ts"),
                    pkg_index,
                )?;
            } else {
                let dep_model = dependency_model(dependency, major_all);
                let major_resolver = |ns: &str, pkg: &str| -> Result<String> {
                    let other = safe_pkg_segment(ns, pkg)?;
                    if layout.is_scoped(ns, pkg) {
                        Ok(format!("../{other}"))
                    } else {
                        Ok(format!("../../{other}"))
                    }
                };
                let emitted = emit_package_interfaces(
                    header,
                    &dep_model,
                    Some(dependency),
                    "../../../witforge_types.js",
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
            major_entries.push((
                seg,
                format!(
                    "Dependency package `{}:{}`.",
                    dependency.package.namespace, dependency.package.name
                ),
            ));
        }

        let mut major_index = String::new();
        writeln!(major_index, "{header}")?;
        writeln!(
            major_index,
            "// source: wit/deps ({major_dir} scoped dependencies)"
        )?;
        if major_entries.is_empty() {
            writeln!(major_index)?;
            writeln!(major_index, "export {{}};")?;
        } else {
            for (seg, doc) in &major_entries {
                writeln!(major_index)?;
                writeln!(major_index, "/** {doc} */")?;
                writeln!(major_index, "export * as {seg} from \"./{seg}/index.js\";")?;
            }
        }
        files.insert_relative(format!("witforge_deps/{major_dir}/index.ts"), major_index)?;
        top_entries.push((
            major_dir.clone(),
            format!("Pinned dependencies for major version `{major_dir}`."),
        ));
    }

    let mut deps_index = String::new();
    writeln!(deps_index, "{header}")?;
    writeln!(deps_index, "// source: wit/deps")?;
    if top_entries.is_empty() {
        writeln!(deps_index)?;
        writeln!(deps_index, "export {{}};")?;
    } else {
        for (entry, doc) in &top_entries {
            writeln!(deps_index)?;
            writeln!(deps_index, "/** {doc} */")?;
            writeln!(
                deps_index,
                "export * as {entry} from \"./{entry}/index.js\";"
            )?;
        }
    }
    files.insert_relative("witforge_deps/index.ts", deps_index)?;
    Ok(())
}

pub(crate) fn dependency_model(dependency: &Dependency, dependencies: &[Dependency]) -> Model {
    Model {
        package: dependency.package.clone(),
        interfaces: dependency.interfaces.clone(),
        worlds: Vec::new(),
        dependencies: dependencies
            .iter()
            .filter(|other| {
                other.package.namespace != dependency.package.namespace
                    || other.package.name != dependency.package.name
            })
            .cloned()
            .collect(),
    }
}

fn emit_version_files(
    header: &str,
    model: &Model,
    options: &TypeScriptOptions,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
) -> Result<BTreeMap<String, String>> {
    emit_package_interfaces(
        header,
        model,
        None,
        "../witforge_types.js",
        options,
        foreign_resolver,
    )
}

fn emit_package_interfaces(
    header: &str,
    model: &Model,
    dep_context: Option<&Dependency>,
    shared_types_import: &str,
    _options: &TypeScriptOptions,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
) -> Result<BTreeMap<String, String>> {
    let pkg_label = format!(
        "{}:{}@{}",
        model.package.namespace, model.package.name, model.package.version
    );
    let iface_plan = plan_interface_names(&model.interfaces, &pkg_label)?;
    let stem_by_name: BTreeMap<&str, &str> = iface_plan
        .iter()
        .map(|(iface, stem, _)| (iface.name.as_str(), stem.as_str()))
        .collect();

    let mut out_files = BTreeMap::new();
    for (interface, stem, _) in &iface_plan {
        let source = emit_interface_file(
            header,
            model,
            dep_context,
            interface,
            &stem_by_name,
            shared_types_import,
            foreign_resolver,
        )?;
        out_files.insert(format!("{stem}.ts"), source);
    }

    let mut barrel = String::new();
    writeln!(barrel, "{header}")?;
    writeln!(barrel, "// source: {pkg_label}")?;
    if !polyglot::doc_lines(&model.package.docs).is_empty() {
        writeln!(barrel)?;
        write_jsdoc(
            &mut barrel,
            0,
            &model.package.docs,
            &Stability::Unknown,
            None,
        )?;
    }
    if iface_plan.is_empty() {
        writeln!(barrel)?;
        writeln!(barrel, "export {{}};")?;
    } else {
        for (interface, stem, export_ident) in &iface_plan {
            writeln!(barrel)?;
            let fallback = format!("Interface `{}`.", interface.name);
            write_jsdoc(
                &mut barrel,
                0,
                &interface.docs,
                &interface.stability,
                Some(&fallback),
            )?;
            writeln!(barrel, "export * as {export_ident} from \"./{stem}.js\";")?;
        }
    }
    out_files.insert("index.ts".to_string(), barrel);
    Ok(out_files)
}

fn plan_interface_names<'a>(
    interfaces: &'a [Interface],
    scope: &str,
) -> Result<Vec<(&'a Interface, String, String)>> {
    let mut stems: BTreeMap<String, String> = BTreeMap::new();
    let mut idents: BTreeMap<String, String> = BTreeMap::new();
    let mut planned = Vec::with_capacity(interfaces.len());

    for interface in interfaces {
        let stem = safe_interface_file_stem(&interface.name)?;
        let stem_lower = stem.to_ascii_lowercase();
        if let Some(previous) = stems.insert(stem_lower, interface.name.clone()) {
            contract_bail!(
                "{scope}: interfaces `{previous}` and `{}` both map to TypeScript file `{stem}.ts`",
                interface.name
            );
        }
        let export_ident = ts_value_ident(&interface.name, "interface namespace")?;
        if let Some(previous) = idents.insert(export_ident.clone(), interface.name.clone()) {
            contract_bail!(
                "{scope}: interfaces `{previous}` and `{}` both map to TypeScript namespace \
                 `{export_ident}`",
                interface.name
            );
        }
        planned.push((interface, stem, export_ident));
    }
    Ok(planned)
}

#[derive(Default)]
struct BuiltinSlot {
    shadowed: bool,
    used: bool,
}

/// External module namespace imports collected while rendering one interface file.
struct InterfaceImports {
    /// Target import specifier (`./types.js`, `../witforge_deps/.../shared.js`) -> local alias.
    by_specifier: BTreeMap<String, String>,
    used_aliases: BTreeSet<String>,
    option: BuiltinSlot,
    bytes: BuiltinSlot,
    map: BuiltinSlot,
}

impl InterfaceImports {
    fn new(local_type_names: &BTreeMap<String, String>) -> Self {
        Self {
            by_specifier: BTreeMap::new(),
            used_aliases: BTreeSet::new(),
            option: BuiltinSlot {
                shadowed: local_type_names.contains_key("Option"),
                used: false,
            },
            bytes: BuiltinSlot {
                shadowed: local_type_names.contains_key("Uint8Array"),
                used: false,
            },
            map: BuiltinSlot {
                shadowed: local_type_names.contains_key("ReadonlyMap"),
                used: false,
            },
        }
    }

    fn alias_for(&mut self, specifier: String, preferred_base: &str) -> String {
        if let Some(existing) = self.by_specifier.get(&specifier) {
            return existing.clone();
        }
        let mut candidate = preferred_base.to_string();
        let mut suffix = 2usize;
        while self.used_aliases.contains(&candidate) {
            candidate = format!("{preferred_base}_{suffix}");
            suffix += 1;
        }
        self.used_aliases.insert(candidate.clone());
        self.by_specifier.insert(specifier, candidate.clone());
        candidate
    }

    fn option_type_name(&mut self) -> &'static str {
        self.option.used = true;
        if self.option.shadowed {
            "_WitforgeOption"
        } else {
            "Option"
        }
    }

    fn bytes_type_name(&mut self) -> &'static str {
        if self.bytes.shadowed {
            self.bytes.used = true;
            "_WitforgeBytes"
        } else {
            "Uint8Array"
        }
    }

    fn map_type_name(&mut self) -> &'static str {
        if self.map.shadowed {
            self.map.used = true;
            "_WitforgeReadonlyMap"
        } else {
            "ReadonlyMap"
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
    let mut local_type_names: BTreeMap<String, String> = BTreeMap::new();
    let service_iface_name = if interface.functions.is_empty() {
        None
    } else {
        let name = ts_type_ident(&interface.name, "interface")?;
        local_type_names.insert(name.clone(), format!("interface {}", interface.name));
        Some(name)
    };
    for def in &interface.types {
        let name = ts_type_ident(&def.name, "type")?;
        insert_name(
            &mut local_type_names,
            &name,
            &format!("type {}/{}", interface.name, def.name),
            &format!("interface `{}`", interface.name),
        )?;
    }

    let mut imports = InterfaceImports::new(&local_type_names);
    let mut body = String::new();
    let mut first = true;

    for def in &interface.types {
        if !first {
            writeln!(body)?;
        }
        first = false;
        emit_type_def(
            &mut body,
            model,
            dep_context,
            interface,
            def,
            local_stems,
            foreign_resolver,
            &mut imports,
        )?;
    }

    if let Some(service_name) = service_iface_name {
        if !first {
            writeln!(body)?;
        }
        first = false;
        emit_service_interface(
            &mut body,
            model,
            dep_context,
            interface,
            &service_name,
            local_stems,
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

    let mut shared_specifiers = Vec::new();
    if imports.option.used {
        if imports.option.shadowed {
            shared_specifiers.push("Option as _WitforgeOption");
        } else {
            shared_specifiers.push("Option");
        }
    }
    if imports.bytes.used {
        shared_specifiers.push("_WitforgeBytes");
    }
    if imports.map.used {
        shared_specifiers.push("_WitforgeReadonlyMap");
    }

    let has_imports = !shared_specifiers.is_empty() || !imports.by_specifier.is_empty();
    if has_imports {
        writeln!(out)?;
        if !shared_specifiers.is_empty() {
            writeln!(
                out,
                "import type {{ {} }} from \"{}\";",
                shared_specifiers.join(", "),
                shared_types_import
            )?;
        }
        for (specifier, alias) in &imports.by_specifier {
            writeln!(out, "import type * as {alias} from \"{specifier}\";")?;
        }
    }

    writeln!(out)?;
    if first {
        writeln!(out, "export {{}};")?;
    } else {
        out.push_str(&body);
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn emit_type_def(
    out: &mut String,
    model: &Model,
    dep_context: Option<&Dependency>,
    interface: &Interface,
    def: &TypeDef,
    local_stems: &BTreeMap<&str, &str>,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut InterfaceImports,
) -> Result<()> {
    let name = ts_type_ident(&def.name, "type")?;
    let scope = format!("{}/{}", interface.name, def.name);

    match &def.kind {
        TypeDefKind::Record(fields) => {
            write_jsdoc(out, 0, &def.docs, &def.stability, None)?;
            if fields.is_empty() {
                writeln!(out, "export interface {name} {{}}")?;
                return Ok(());
            }
            let mut field_names = BTreeMap::new();
            writeln!(out, "export interface {name} {{")?;
            for field in fields {
                let ts_field = ts_value_ident(&field.name, "record field")?;
                insert_name(&mut field_names, &ts_field, &field.name, &scope)?;
                write_jsdoc(out, 2, &field.docs, &Stability::Unknown, None)?;
                let ty = map_type(
                    model,
                    dep_context,
                    &interface.name,
                    &field.ty,
                    local_stems,
                    foreign_resolver,
                    imports,
                )?;
                writeln!(out, "  readonly {ts_field}: {ty};")?;
            }
            writeln!(out, "}}")?;
        }
        TypeDefKind::Variant(cases) => {
            write_jsdoc(out, 0, &def.docs, &def.stability, None)?;
            if cases.is_empty() {
                writeln!(out, "export type {name} = never;")?;
                return Ok(());
            }
            let mut case_tags = BTreeMap::new();
            writeln!(out, "export type {name} =")?;
            for (idx, case) in cases.iter().enumerate() {
                insert_name(&mut case_tags, &case.name, &case.name, &scope)?;
                write_jsdoc(out, 2, &case.docs, &Stability::Unknown, None)?;
                let tag = escape_ts_string(&case.name);
                let semi = if idx + 1 == cases.len() { ";" } else { "" };
                if let Some(payload) = &case.payload {
                    let ty = map_type(
                        model,
                        dep_context,
                        &interface.name,
                        payload,
                        local_stems,
                        foreign_resolver,
                        imports,
                    )?;
                    writeln!(
                        out,
                        "  | {{ readonly tag: {tag}; readonly value: {ty} }}{semi}"
                    )?;
                } else {
                    writeln!(out, "  | {{ readonly tag: {tag} }}{semi}")?;
                }
            }
        }
        TypeDefKind::Enum(cases) => {
            write_jsdoc(out, 0, &def.docs, &def.stability, None)?;
            let mut case_names = BTreeMap::new();
            writeln!(out, "export const {name} = {{")?;
            for case in cases {
                let ts_case = ts_type_ident(&case.name, "enum case")?;
                insert_name(&mut case_names, &ts_case, &case.name, &scope)?;
                write_jsdoc(out, 2, &case.docs, &Stability::Unknown, None)?;
                let literal = escape_ts_string(&case.name);
                writeln!(out, "  {ts_case}: {literal},")?;
            }
            writeln!(out, "}} as const;")?;
            writeln!(out)?;
            write_jsdoc(out, 0, &def.docs, &def.stability, None)?;
            writeln!(
                out,
                "export type {name} = (typeof {name})[keyof typeof {name}];"
            )?;
        }
        TypeDefKind::Flags(flags) => {
            write_jsdoc(out, 0, &def.docs, &def.stability, None)?;
            if flags.is_empty() {
                writeln!(out, "export interface {name} {{}}")?;
                return Ok(());
            }
            let mut flag_names = BTreeMap::new();
            writeln!(out, "export interface {name} {{")?;
            for flag in flags {
                let ts_flag = ts_value_ident(&flag.name, "flag")?;
                insert_name(&mut flag_names, &ts_flag, &flag.name, &scope)?;
                write_jsdoc(out, 2, &flag.docs, &Stability::Unknown, None)?;
                writeln!(out, "  readonly {ts_flag}: boolean;")?;
            }
            writeln!(out, "}}")?;
        }
        TypeDefKind::Alias(target) => {
            write_jsdoc(out, 0, &def.docs, &def.stability, None)?;
            let ty = map_type(
                model,
                dep_context,
                &interface.name,
                target,
                local_stems,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "export type {name} = {ty};")?;
        }
        TypeDefKind::List(elem) => {
            write_jsdoc(out, 0, &def.docs, &def.stability, None)?;
            let ty = map_list_type(
                model,
                dep_context,
                &interface.name,
                elem,
                local_stems,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "export type {name} = {ty};")?;
        }
        TypeDefKind::FixedLengthList(elem, len) => {
            let note = format!("Fixed-length list of exactly {len} elements.");
            let combined_docs = if polyglot::doc_lines(&def.docs).is_empty() {
                note
            } else {
                format!("{}\n\n{note}", def.docs)
            };
            write_jsdoc(out, 0, &combined_docs, &def.stability, None)?;
            let ty = map_list_type(
                model,
                dep_context,
                &interface.name,
                elem,
                local_stems,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "export type {name} = {ty};")?;
        }
        TypeDefKind::Option(inner) => {
            write_jsdoc(out, 0, &def.docs, &def.stability, None)?;
            let opt = imports.option_type_name();
            let inner_ty = map_type(
                model,
                dep_context,
                &interface.name,
                inner,
                local_stems,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "export type {name} = {opt}<{inner_ty}>;")?;
        }
        TypeDefKind::Result(res) => {
            write_jsdoc(out, 0, &def.docs, &def.stability, None)?;
            let ty = map_result_type(
                model,
                dep_context,
                &interface.name,
                res,
                local_stems,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "export type {name} = {ty};")?;
        }
        TypeDefKind::Map(key, value) => {
            write_jsdoc(out, 0, &def.docs, &def.stability, None)?;
            let map_name = imports.map_type_name();
            let key_ty = map_type(
                model,
                dep_context,
                &interface.name,
                key,
                local_stems,
                foreign_resolver,
                imports,
            )?;
            let val_ty = map_type(
                model,
                dep_context,
                &interface.name,
                value,
                local_stems,
                foreign_resolver,
                imports,
            )?;
            writeln!(out, "export type {name} = {map_name}<{key_ty}, {val_ty}>;")?;
        }
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
    local_stems: &BTreeMap<&str, &str>,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut InterfaceImports,
) -> Result<()> {
    let fallback = format!("Interface `{}`.", interface.name);
    write_jsdoc(
        out,
        0,
        &interface.docs,
        &interface.stability,
        Some(&fallback),
    )?;
    writeln!(out, "export interface {service_name} {{")?;
    let mut method_names = BTreeMap::new();
    for func in &interface.functions {
        let method = ts_value_ident(&func.name, "function")?;
        insert_name(
            &mut method_names,
            &method,
            &func.name,
            &format!("interface `{}`", interface.name),
        )?;
        write_jsdoc(out, 2, &func.docs, &func.stability, None)?;
        let mut param_names = BTreeMap::new();
        let mut rendered_params = Vec::with_capacity(func.params.len());
        let fn_scope = format!("{}#{}", interface.name, func.name);
        for param in &func.params {
            let param_ident = ts_value_ident(&param.name, "parameter")?;
            insert_name(&mut param_names, &param_ident, &param.name, &fn_scope)?;
            let param_ty = map_type(
                model,
                dep_context,
                &interface.name,
                &param.ty,
                local_stems,
                foreign_resolver,
                imports,
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
            )?,
            None => "void".to_string(),
        };
        writeln!(out, "  {method}({}): {ret_ty};", rendered_params.join(", "))?;
    }
    writeln!(out, "}}")?;
    Ok(())
}

fn map_type(
    model: &Model,
    dep_context: Option<&Dependency>,
    current_interface: &str,
    ty: &TypeExpr,
    local_stems: &BTreeMap<&str, &str>,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut InterfaceImports,
) -> Result<String> {
    match ty {
        TypeExpr::Bool => Ok("boolean".to_string()),
        TypeExpr::U8
        | TypeExpr::U16
        | TypeExpr::U32
        | TypeExpr::S8
        | TypeExpr::S16
        | TypeExpr::S32
        | TypeExpr::F32
        | TypeExpr::F64 => Ok("number".to_string()),
        TypeExpr::U64 | TypeExpr::S64 => Ok("bigint".to_string()),
        TypeExpr::Char | TypeExpr::String => Ok("string".to_string()),
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
        ),
        TypeExpr::Option(inner) => {
            let opt = imports.option_type_name();
            let inner_ty = map_type(
                model,
                dep_context,
                current_interface,
                inner,
                local_stems,
                foreign_resolver,
                imports,
            )?;
            Ok(format!("{opt}<{inner_ty}>"))
        }
        TypeExpr::Result(res) => map_result_type(
            model,
            dep_context,
            current_interface,
            res,
            local_stems,
            foreign_resolver,
            imports,
        ),
        TypeExpr::Map(key, value) => {
            let map_name = imports.map_type_name();
            let key_ty = map_type(
                model,
                dep_context,
                current_interface,
                key,
                local_stems,
                foreign_resolver,
                imports,
            )?;
            let val_ty = map_type(
                model,
                dep_context,
                current_interface,
                value,
                local_stems,
                foreign_resolver,
                imports,
            )?;
            Ok(format!("{map_name}<{key_ty}, {val_ty}>"))
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
        let pkg_dir = foreign_resolver(namespace, package)?;
        let iface_stem = safe_interface_file_stem(name.interface_name())?;
        let specifier = format!("{pkg_dir}/{iface_stem}.js");
        let base_alias = format!(
            "_d_{}_{}",
            dep_package_segment(namespace, package),
            iface_stem
        );
        let alias = imports.alias_for(specifier, &base_alias);
        let type_ident = ts_type_ident(&name.name, "type")?;
        return Ok(format!("{alias}.{type_ident}"));
    }

    model.type_def_in(dep_context, name).with_context(|| {
        format!(
            "named type `{}/{}` is not in the model",
            name.interface, name.name
        )
    })?;
    let type_ident = ts_type_ident(&name.name, "type")?;
    if name.interface == current_interface {
        Ok(type_ident)
    } else {
        let target_stem = local_stems
            .get(name.interface.as_str())
            .copied()
            .map(str::to_string)
            .map_or_else(|| safe_interface_file_stem(&name.interface), Ok)?;
        let specifier = format!("./{target_stem}.js");
        let base_alias = format!("_i_{target_stem}");
        let alias = imports.alias_for(specifier, &base_alias);
        Ok(format!("{alias}.{type_ident}"))
    }
}

fn map_list_type(
    model: &Model,
    dep_context: Option<&Dependency>,
    current_interface: &str,
    elem: &TypeExpr,
    local_stems: &BTreeMap<&str, &str>,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut InterfaceImports,
) -> Result<String> {
    if model.resolves_to_u8_in(dep_context, elem) {
        return Ok(imports.bytes_type_name().to_string());
    }
    let inner = map_type(
        model,
        dep_context,
        current_interface,
        elem,
        local_stems,
        foreign_resolver,
        imports,
    )?;
    // Parenthesize union types (`result<...>`) and nested `readonly T[]` arrays so `readonly (...)[]`
    // parses unambiguously in strict TypeScript.
    let needs_parens = matches!(
        elem,
        TypeExpr::Result(_) | TypeExpr::List(_) | TypeExpr::FixedLengthList(_, _)
    ) && inner != "Uint8Array"
        && inner != "_WitforgeBytes";
    if needs_parens {
        Ok(format!("readonly ({inner})[]"))
    } else {
        Ok(format!("readonly {inner}[]"))
    }
}

fn map_result_type(
    model: &Model,
    dep_context: Option<&Dependency>,
    current_interface: &str,
    res: &crate::model::ResultExpr,
    local_stems: &BTreeMap<&str, &str>,
    foreign_resolver: &dyn Fn(&str, &str) -> Result<String>,
    imports: &mut InterfaceImports,
) -> Result<String> {
    let ok_branch = match &res.ok {
        Some(ok_ty) => {
            let rendered = map_type(
                model,
                dep_context,
                current_interface,
                ok_ty,
                local_stems,
                foreign_resolver,
                imports,
            )?;
            format!("{{ readonly tag: \"ok\"; readonly value: {rendered} }}")
        }
        None => "{ readonly tag: \"ok\" }".to_string(),
    };
    let err_branch = match &res.err {
        Some(err_ty) => {
            let rendered = map_type(
                model,
                dep_context,
                current_interface,
                err_ty,
                local_stems,
                foreign_resolver,
                imports,
            )?;
            format!("{{ readonly tag: \"err\"; readonly error: {rendered} }}")
        }
        None => "{ readonly tag: \"err\" }".to_string(),
    };
    Ok(format!("{ok_branch} | {err_branch}"))
}

fn emit_worlds(header: &str, versions: &[Version]) -> Result<String> {
    let Some(first) = versions.first() else {
        bail!("no version directories found under WIT root");
    };
    let mut out = String::new();
    writeln!(out, "{header}")?;
    writeln!(
        out,
        "// source: {}:{}",
        first.model.package.namespace, first.model.package.name
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "/** One contract world and the interfaces it imports and exports. */"
    )?;
    writeln!(out, "export interface WorldMeta {{")?;
    writeln!(out, "  /** The WIT world name. */")?;
    writeln!(out, "  readonly name: string;")?;
    writeln!(out, "  /** Imported interface names, in source order. */")?;
    writeln!(out, "  readonly imports: readonly string[];")?;
    writeln!(out, "  /** Exported interface names, in source order. */")?;
    writeln!(out, "  readonly exports: readonly string[];")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "/** World metadata per live contract version. */")?;
    writeln!(out, "export const WORLDS = {{")?;
    for version in versions {
        let dir_name = version_dir_name(&version.model);
        write_version_worlds_jsdoc(&mut out, version)?;
        if version.model.worlds.is_empty() {
            writeln!(out, "  {dir_name}: [],")?;
            continue;
        }
        writeln!(out, "  {dir_name}: [")?;
        for world in &version.model.worlds {
            writeln!(out, "    {{")?;
            writeln!(out, "      name: {},", escape_ts_string(&world.name))?;
            write_world_string_array(&mut out, "imports", &world.imports)?;
            write_world_string_array(&mut out, "exports", &world.exports)?;
            writeln!(out, "    }},")?;
        }
        writeln!(out, "  ],")?;
    }
    writeln!(
        out,
        "}} as const satisfies Readonly<Record<string, readonly WorldMeta[]>>;"
    )?;
    Ok(out)
}

fn write_version_worlds_jsdoc(out: &mut String, version: &Version) -> Result<()> {
    let mut lines: Vec<String> = vec![format!(
        "The worlds of {}.",
        escape_jsdoc_line(&version.model.package.version.to_string())
    )];
    for world in &version.model.worlds {
        let world_name = escape_jsdoc_line(&world.name);
        let world_lines = polyglot::doc_lines(&world.docs);
        if !world_lines.is_empty() {
            lines.push(String::new());
            lines.push(format!("World `{world_name}`:"));
            lines.push(String::new());
            for line in world_lines {
                lines.push(escape_jsdoc_line(&line));
            }
        }
        if let Some(stability) = stability_doc(&world.stability) {
            lines.push(String::new());
            lines.push(format!(
                "World `{world_name}` stability: {}",
                escape_jsdoc_line(&stability)
            ));
        }
        for (side, interfaces) in [("import", &world.imports), ("export", &world.exports)] {
            for iface in interfaces {
                let qname = escape_jsdoc_line(&iface.qualified_name());
                let iface_docs = polyglot::doc_lines(&iface.docs);
                if !iface_docs.is_empty() {
                    lines.push(String::new());
                    lines.push(format!(
                        "World `{world_name}` {side} `{qname}`: {}",
                        iface_docs
                            .into_iter()
                            .map(|l| escape_jsdoc_line(&l))
                            .collect::<Vec<_>>()
                            .join(" ")
                    ));
                }
                if let Some(stability) = stability_doc(&iface.stability) {
                    lines.push(String::new());
                    lines.push(format!(
                        "World `{world_name}` {side} `{qname}` stability: {}",
                        escape_jsdoc_line(&stability)
                    ));
                }
            }
        }
    }
    if lines.len() == 1 {
        writeln!(out, "  /** {} */", lines[0])?;
        return Ok(());
    }
    writeln!(out, "  /**")?;
    for line in &lines {
        if line.is_empty() {
            writeln!(out, "   *")?;
        } else {
            writeln!(out, "   * {line}")?;
        }
    }
    writeln!(out, "   */")?;
    Ok(())
}

fn write_world_string_array(
    out: &mut String,
    field: &str,
    items: &[crate::model::WorldInterface],
) -> Result<()> {
    if items.is_empty() {
        writeln!(out, "      {field}: [],")?;
        return Ok(());
    }
    let rendered: Vec<String> = items
        .iter()
        .map(|iface| escape_ts_string(&iface.qualified_name()))
        .collect();
    writeln!(out, "      {field}: [{}],", rendered.join(", "))?;
    Ok(())
}

pub(crate) fn safe_interface_file_stem(wit_name: &str) -> Result<String> {
    let mut stem = to_snake_case(wit_name);
    if stem.is_empty() {
        contract_bail!("WIT interface name `{wit_name}` produces an empty TypeScript filename");
    }
    if stem == "index" || is_windows_reserved_stem(&stem) {
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
        || is_windows_reserved_stem(&seg)
    {
        contract_bail!(
            "dependency package `{namespace}:{name}` produces invalid TypeScript directory `{seg}`"
        );
    }
    Ok(seg)
}

pub(crate) fn ts_type_ident(wit_name: &str, what: &str) -> Result<String> {
    let pascal = to_pascal_case(wit_name);
    if pascal.is_empty() || pascal.as_bytes()[0].is_ascii_digit() {
        contract_bail!(
            "WIT {what} `{wit_name}` cannot be represented as a TypeScript identifier (`{pascal}`)"
        );
    }
    Ok(escape_typescript_ident(&pascal))
}

pub(crate) fn ts_value_ident(wit_name: &str, what: &str) -> Result<String> {
    let camel = to_camel_case(wit_name);
    if camel.is_empty() || camel.as_bytes()[0].is_ascii_digit() {
        contract_bail!(
            "WIT {what} `{wit_name}` cannot be represented as a TypeScript identifier (`{camel}`)"
        );
    }
    Ok(escape_typescript_ident(&camel))
}

pub(crate) fn insert_name(
    names: &mut BTreeMap<String, String>,
    ident: &str,
    source: &str,
    scope: &str,
) -> Result<()> {
    if let Some(previous) = names.insert(ident.to_string(), source.to_string()) {
        contract_bail!(
            "{scope}: TypeScript name `{ident}` is produced by both `{previous}` and `{source}`"
        );
    }
    Ok(())
}

fn escape_jsdoc_line(line: &str) -> String {
    line.replace("*/", "*\\/")
}

pub(crate) fn write_jsdoc(
    out: &mut String,
    indent: usize,
    raw_docs: &str,
    stability: &Stability,
    fallback: Option<&str>,
) -> Result<()> {
    let mut lines: Vec<String> = polyglot::doc_lines(raw_docs)
        .into_iter()
        .map(|l| escape_jsdoc_line(&l))
        .collect();
    if lines.is_empty()
        && let Some(fb) = fallback
    {
        lines.push(escape_jsdoc_line(fb));
    }

    let mut tags: Vec<String> = Vec::new();
    if let Some(note) = stability.deprecation_note() {
        if let Some(since) = stability.since() {
            lines.push(format!("Stable since WIT version {since}."));
        } else if let Some(feature) = stability.unstable_feature() {
            lines.push(format!(
                "Unstable WIT feature `{}`.",
                escape_jsdoc_line(feature)
            ));
        }
        tags.push(format!("@deprecated {}", escape_jsdoc_line(note)));
    } else if let Some(stab) = stability_doc(stability) {
        lines.push(escape_jsdoc_line(&stab));
    }

    if lines.is_empty() && tags.is_empty() {
        return Ok(());
    }
    let prefix = " ".repeat(indent);
    if lines.len() == 1 && tags.is_empty() && !lines[0].starts_with('@') {
        writeln!(out, "{prefix}/** {} */", lines[0])?;
        return Ok(());
    }
    writeln!(out, "{prefix}/**")?;
    for line in &lines {
        if line.is_empty() {
            writeln!(out, "{prefix} *")?;
        } else {
            writeln!(out, "{prefix} * {line}")?;
        }
    }
    if !lines.is_empty() && !tags.is_empty() {
        writeln!(out, "{prefix} *")?;
    }
    for tag in &tags {
        writeln!(out, "{prefix} * {tag}")?;
    }
    writeln!(out, "{prefix} */")?;
    Ok(())
}

pub(crate) fn escape_ts_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 || c as u32 == 0x7F => {
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

    #[test]
    fn reserved_windows_and_index_interface_stems_are_escaped() {
        assert_eq!(safe_interface_file_stem("types").unwrap(), "types");
        assert_eq!(safe_interface_file_stem("my-api").unwrap(), "my_api");
        assert_eq!(safe_interface_file_stem("index").unwrap(), "index_");
        assert_eq!(safe_interface_file_stem("con").unwrap(), "con_");
        assert_eq!(safe_interface_file_stem("nul").unwrap(), "nul_");
        assert_eq!(safe_interface_file_stem("com1").unwrap(), "com1_");
        assert_eq!(safe_interface_file_stem("lpt9").unwrap(), "lpt9_");
    }

    #[test]
    fn jsdoc_escapes_closing_comment_delimiters_and_emits_deprecated_tag() {
        let mut out = String::new();
        let stab = Stability::Deprecated {
            since: Some(crate::model::Version::new(0, 1, 0)),
            feature: None,
            deprecated: crate::model::Version::new(0, 2, 0),
            note: "Deprecated in WIT version 0.2.0.".to_string(),
        };
        write_jsdoc(&mut out, 0, "Handles `a */ b` safely.", &stab, None).unwrap();
        assert!(out.contains("Handles `a *\\/ b` safely."), "{out}");
        assert!(
            out.contains("@deprecated Deprecated in WIT version 0.2.0."),
            "{out}"
        );
        assert_eq!(
            out.matches("*/").count(),
            1,
            "only the closing delimiter may be `*/`:\n{out}"
        );
    }
}
