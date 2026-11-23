//! Generated conversions between consecutive contract versions.
//!
//! The plain Rust emitter generates one module per version. A host that supports several
//! versions also needs adapters between them, so this module writes a `convert` module for every
//! adjacent pair of version directories, including a major-version boundary. The conversions are
//! deliberately explicit and fallible: evolution can remove a field or a case, and a conversion
//! must report that it cannot reconstruct it instead of inventing a value.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use anyhow::{Context, Result, bail};

use crate::diff::ConversionOmission;
use crate::emit::defaults::default_expr;
use crate::emit::rust::{header_for, version_module_name};
use crate::emit::versions::{Version, same_effective_dependency};
use crate::emit::{
    TypeRenderStyle, model_has_deprecation, pascal_case, render_type, rust_ident, rustfmt,
    snake_case, type_kind_name,
};
use crate::model::{
    Case, Dependency, Field, Interface, Model, QualifiedName, ResultExpr, TypeDef, TypeDefKind,
    TypeExpr,
};

/// Return a [`crate::ContractError`] carrying [`crate::diagnostic::codes::CONVERSION_REFUSED`]
/// (`WF2002`) with its logical contract path.
macro_rules! convert_bail {
    ($path:expr, $($arg:tt)*) => {{
        let message = format!($($arg)*);
        return Err(anyhow::Error::new(
            $crate::ContractError::with_diagnostic(
                $crate::diagnostic::Diagnostic::error(
                    $crate::diagnostic::codes::CONVERSION_REFUSED,
                    message,
                )
                .with_path($path),
            ),
        ));
    }};
}

/// The shared error type imported into each interface conversion module.
const ERROR: &str = "ConversionError";

/// Plan the `convert.rs` file for `versions`, or `None` when there are fewer than two versions.
pub(crate) fn plan(versions: &[Version]) -> Result<Option<String>> {
    let pairs: Vec<(&Version, &Version)> = versions
        .windows(2)
        .map(|pair| (&pair[0], &pair[1]))
        .collect();
    if pairs.is_empty() {
        return Ok(None);
    }

    let mut all_omissions: Vec<ConversionOmission> = Vec::new();
    let mut omitted_by_dir: BTreeMap<
        (crate::model::Version, crate::model::Version),
        BTreeSet<String>,
    > = BTreeMap::new();
    for &(old, new) in &pairs {
        let omissions = analyze_cross_major_omissions(&old.model, &new.model)?;
        for omission in &omissions {
            omitted_by_dir
                .entry((omission.from_version, omission.to_version))
                .or_default()
                .insert(omission.type_path.clone());
        }
        all_omissions.extend(omissions);
    }
    all_omissions.sort_by(|left, right| {
        left.from_version
            .cmp(&right.from_version)
            .then_with(|| left.to_version.cmp(&right.to_version))
            .then_with(|| left.type_path.cmp(&right.type_path))
            .then_with(|| left.reason.cmp(&right.reason))
    });

    let package = &versions
        .first()
        .context("no version directories found under WIT root")?
        .model
        .package;
    let mut out = String::new();
    writeln!(out, "{}", header_for(&package.namespace, &package.name))?;
    writeln!(out, "// source: {}:{}", package.namespace, package.name)?;
    if versions
        .iter()
        .any(|version| model_has_deprecation(&version.model))
    {
        writeln!(out, "#![allow(deprecated)]")?;
    }
    if !all_omissions.is_empty() {
        writeln!(out, "//")?;
        writeln!(
            out,
            "// Omitted cross-major conversions (write manual adapters for these):"
        )?;
        for omission in &all_omissions {
            writeln!(
                out,
                "// - {} -> {} {}: {}",
                omission.from_version, omission.to_version, omission.type_path, omission.reason
            )?;
        }
    }
    writeln!(out)?;
    write_error(&mut out)?;
    let empty_omitted = BTreeSet::new();
    for &(old, new) in &pairs {
        let fwd_omitted = omitted_by_dir
            .get(&(old.model.package.version, new.model.package.version))
            .unwrap_or(&empty_omitted);
        let rev_omitted = omitted_by_dir
            .get(&(new.model.package.version, old.model.package.version))
            .unwrap_or(&empty_omitted);
        writeln!(out)?;
        emit_pair(&mut out, old, new, Direction::Forward, fwd_omitted)
            .map_err(|err| attach_pair_locations(err, new, old))?;
        writeln!(out)?;
        emit_pair(&mut out, new, old, Direction::Reverse, rev_omitted)
            .map_err(|err| attach_pair_locations(err, old, new))?;
    }
    Ok(Some(out))
}

/// Analyze which type conversions are omitted across a major-version boundary, per direction.
///
/// Within one major (`old.major == new.major`), conversion generation never omits and returns an
/// empty list (incompatible evolutions are refused instead). Across a major boundary, every type
/// without a namesake, every directly incompatible type, and every type that transitively
/// contains an omitted type is reported per direction in deterministic sorted order.
pub(crate) fn analyze_cross_major_omissions(
    old: &Model,
    new: &Model,
) -> Result<Vec<ConversionOmission>> {
    if old.package.version.major == new.package.version.major {
        return Ok(Vec::new());
    }
    let mut omissions = Vec::new();
    for (source, target, direction) in [
        (old, new, Direction::Forward),
        (new, old, Direction::Reverse),
    ] {
        let omitted = analyze_direction_omissions(source, target, direction)?;
        for (type_path, reason) in omitted {
            omissions.push(ConversionOmission {
                from_version: source.package.version,
                to_version: target.package.version,
                type_path,
                reason,
            });
        }
    }
    omissions.sort_by(|left, right| {
        left.from_version
            .cmp(&right.from_version)
            .then_with(|| left.to_version.cmp(&right.to_version))
            .then_with(|| left.type_path.cmp(&right.type_path))
            .then_with(|| left.reason.cmp(&right.reason))
    });
    Ok(omissions)
}

fn analyze_direction_omissions(
    source: &Model,
    target: &Model,
    direction: Direction,
) -> Result<BTreeMap<String, String>> {
    let mut omitted: BTreeMap<String, String> = BTreeMap::new();
    let mut shared: Vec<(&Interface, &TypeDef, &TypeDef)> = Vec::new();
    for source_interface in &source.interfaces {
        let target_interface = target.interface(&source_interface.name);
        for source_def in &source_interface.types {
            let type_path = format!("{}/{}", source_interface.name, source_def.name);
            let target_def = target_interface.and_then(|iface| {
                iface
                    .types
                    .iter()
                    .find(|candidate| candidate.name == source_def.name)
            });
            if let Some(target_def) = target_def {
                shared.push((source_interface, source_def, target_def));
            } else {
                omitted.insert(
                    type_path,
                    format!("no namesake in {}", target.package.version),
                );
            }
        }
    }

    loop {
        let mut grew = false;
        for &(source_interface, source_def, target_def) in &shared {
            let type_path = format!("{}/{}", source_interface.name, source_def.name);
            if omitted.contains_key(&type_path) {
                continue;
            }
            if let Some(reason) =
                check_type_convertible(source, target, source_def, target_def, direction, &omitted)?
            {
                omitted.insert(type_path, reason);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    Ok(omitted)
}

#[allow(clippy::too_many_lines)]
fn check_type_convertible(
    source: &Model,
    target: &Model,
    source_def: &TypeDef,
    target_def: &TypeDef,
    direction: Direction,
    omitted: &BTreeMap<String, String>,
) -> Result<Option<String>> {
    match (&source_def.kind, &target_def.kind) {
        (TypeDefKind::Record(source_fields), TypeDefKind::Record(target_fields)) => {
            for target_field in target_fields {
                if let Some(source_field) = source_fields
                    .iter()
                    .find(|field| field.name == target_field.name)
                    && let Some(reason) = check_type_expr_convertible(
                        source,
                        target,
                        &source_field.ty,
                        &target_field.ty,
                        &format!("field `{}`", source_field.name),
                        omitted,
                    )?
                {
                    return Ok(Some(reason));
                }
            }
            if matches!(direction, Direction::Forward) {
                for target_field in target_fields {
                    if source_fields
                        .iter()
                        .any(|field| field.name == target_field.name)
                    {
                        continue;
                    }
                    if default_expr(target, &target_field.ty)?.is_none() {
                        return Ok(Some(format!(
                            "field `{}` is required in {} and has no default",
                            target_field.name, target.package.version
                        )));
                    }
                }
            }
            Ok(None)
        }
        (TypeDefKind::Variant(source_cases), TypeDefKind::Variant(target_cases)) => {
            for source_case in source_cases {
                let Some(target_case) = target_cases
                    .iter()
                    .find(|case| case.name == source_case.name)
                else {
                    continue;
                };
                match (&source_case.payload, &target_case.payload) {
                    (Some(_), None) | (None, Some(_)) => {
                        return Ok(Some(format!(
                            "case `{}` changes payload presence",
                            source_case.name
                        )));
                    }
                    (Some(source_payload), Some(target_payload)) => {
                        if let Some(reason) = check_type_expr_convertible(
                            source,
                            target,
                            source_payload,
                            target_payload,
                            &format!("case `{}`", source_case.name),
                            omitted,
                        )? {
                            return Ok(Some(reason));
                        }
                    }
                    (None, None) => {}
                }
            }
            Ok(None)
        }
        (TypeDefKind::Enum(_), TypeDefKind::Enum(_))
        | (TypeDefKind::Flags(_), TypeDefKind::Flags(_)) => Ok(None),
        (TypeDefKind::Alias(source_target), TypeDefKind::Alias(target_target)) => {
            check_type_expr_convertible(
                source,
                target,
                source_target,
                target_target,
                "alias target",
                omitted,
            )
        }
        (TypeDefKind::List(source_target), TypeDefKind::List(target_target)) => {
            check_type_expr_convertible(
                source,
                target,
                &TypeExpr::List(Box::new(source_target.clone())),
                &TypeExpr::List(Box::new(target_target.clone())),
                "element",
                omitted,
            )
        }
        (TypeDefKind::Option(source_target), TypeDefKind::Option(target_target)) => {
            check_type_expr_convertible(
                source,
                target,
                &TypeExpr::Option(Box::new(source_target.clone())),
                &TypeExpr::Option(Box::new(target_target.clone())),
                "payload",
                omitted,
            )
        }
        (TypeDefKind::Result(source_result), TypeDefKind::Result(target_result)) => {
            check_type_expr_convertible(
                source,
                target,
                &TypeExpr::Result(source_result.clone()),
                &TypeExpr::Result(target_result.clone()),
                "result",
                omitted,
            )
        }
        _ => Ok(Some(format!(
            "changes kind from {} to {}",
            type_kind_name(&source_def.kind),
            type_kind_name(&target_def.kind)
        ))),
    }
}

#[allow(clippy::too_many_lines)]
fn check_type_expr_convertible(
    source: &Model,
    target: &Model,
    source_type: &TypeExpr,
    target_type: &TypeExpr,
    context_label: &str,
    omitted: &BTreeMap<String, String>,
) -> Result<Option<String>> {
    let resolved_source = resolve_expr(source, None, source_type, "source", &mut HashSet::new())?;
    let resolved_target = resolve_expr(target, None, target_type, "target", &mut HashSet::new())?;
    match (&resolved_source, &resolved_target) {
        (TypeExpr::Bool, TypeExpr::Bool)
        | (TypeExpr::U8, TypeExpr::U8)
        | (TypeExpr::U16, TypeExpr::U16)
        | (TypeExpr::U32, TypeExpr::U32)
        | (TypeExpr::U64, TypeExpr::U64)
        | (TypeExpr::S8, TypeExpr::S8)
        | (TypeExpr::S16, TypeExpr::S16)
        | (TypeExpr::S32, TypeExpr::S32)
        | (TypeExpr::S64, TypeExpr::S64)
        | (TypeExpr::F32, TypeExpr::F32)
        | (TypeExpr::F64, TypeExpr::F64)
        | (TypeExpr::String, TypeExpr::String) => Ok(None),
        (TypeExpr::Named(source_name), TypeExpr::Named(target_name))
            if source_name == target_name =>
        {
            if let Some((ns, pkg)) = source_name.package() {
                if !same_effective_dependency(source, target, ns, pkg) {
                    return Ok(Some(format!(
                        "{context_label} uses foreign type `{source_name}` from changed \
                         dependency `{ns}:{pkg}`"
                    )));
                }
                return Ok(None);
            }
            let ref_path = source_name.to_string();
            if omitted.contains_key(&ref_path) || target.type_def(target_name).is_none() {
                return Ok(Some(format!(
                    "{context_label} references omitted type `{ref_path}`"
                )));
            }
            Ok(None)
        }
        (TypeExpr::List(sa), TypeExpr::List(tb)) | (TypeExpr::Option(sa), TypeExpr::Option(tb)) => {
            check_type_expr_convertible(source, target, sa, tb, context_label, omitted)
        }
        (TypeExpr::Result(sa), TypeExpr::Result(tb)) => {
            match (&sa.ok, &tb.ok) {
                (Some(a), Some(b)) => {
                    if let Some(reason) =
                        check_type_expr_convertible(source, target, a, b, context_label, omitted)?
                    {
                        return Ok(Some(reason));
                    }
                }
                (None, None) => {}
                _ => {
                    return Ok(Some(format!(
                        "{context_label} changes result success shape"
                    )));
                }
            }
            match (&sa.err, &tb.err) {
                (Some(a), Some(b)) => {
                    if let Some(reason) =
                        check_type_expr_convertible(source, target, a, b, context_label, omitted)?
                    {
                        return Ok(Some(reason));
                    }
                }
                (None, None) => {}
                _ => {
                    return Ok(Some(format!("{context_label} changes result error shape")));
                }
            }
            Ok(None)
        }
        _ => Ok(Some(format!(
            "{context_label} changes type from `{}` to `{}`",
            render_type(&resolved_source, TypeRenderStyle::Spaced),
            render_type(&resolved_target, TypeRenderStyle::Spaced)
        ))),
    }
}

fn attach_pair_locations(
    err: anyhow::Error,
    primary: &Version,
    fallback: &Version,
) -> anyhow::Error {
    let Some(existing) = crate::error_located_diagnostics(&err) else {
        return err;
    };
    let located: Vec<crate::diagnostic::LocatedDiagnostic> = existing
        .iter()
        .map(|item| {
            let mut item = item.clone();
            if item.file.is_none()
                && let Some(path) = item.diagnostic.path.clone()
            {
                item.logical.get_or_insert_with(|| path.clone());
                if let Some((file, pos)) = primary
                    .locations
                    .get(&path)
                    .or_else(|| fallback.locations.get(&path))
                {
                    item.diagnostic.position = Some(*pos);
                    item.file = Some(file.clone());
                }
            }
            item
        })
        .collect();
    anyhow::Error::new(crate::ContractError::with_located_diagnostics(
        format!("{err:#}"),
        located,
    ))
}

#[allow(clippy::too_many_lines)]
fn write_error(out: &mut String) -> Result<()> {
    writeln!(
        out,
        "/// A version-to-version conversion that cannot reconstruct the target value."
    )?;
    writeln!(out, "///")?;
    writeln!(
        out,
        "/// `#[non_exhaustive]` prevents exhaustive matching in crates downstream of the crate"
    )?;
    writeln!(
        out,
        "/// that includes this generated module. Inside the including crate itself it has no"
    )?;
    writeln!(
        out,
        "/// effect, so a `match` on `ConversionError` there should include a wildcard arm and"
    )?;
    writeln!(
        out,
        "/// struct patterns should include `..` to stay compatible with future generator releases."
    )?;
    writeln!(out, "#[derive(Debug, Clone, PartialEq, Eq)]")?;
    writeln!(out, "#[non_exhaustive]")?;
    writeln!(out, "pub enum ConversionError {{")?;
    writeln!(
        out,
        "    /// The target version requires a member that the source version does not have."
    )?;
    writeln!(out, "    MissingMember {{")?;
    writeln!(
        out,
        "        /// The source contract version, such as `0.2.0`."
    )?;
    writeln!(out, "        from: &'static str,")?;
    writeln!(
        out,
        "        /// The target contract version, such as `0.1.0`."
    )?;
    writeln!(out, "        to: &'static str,")?;
    writeln!(
        out,
        "        /// The canonical WIT member path, such as `types/user.name`."
    )?;
    writeln!(out, "        path: &'static str,")?;
    writeln!(out, "    }},")?;
    writeln!(
        out,
        "    /// The source value has a case that the target version does not have."
    )?;
    writeln!(out, "    UnknownCase {{")?;
    writeln!(
        out,
        "        /// The source contract version, such as `0.2.0`."
    )?;
    writeln!(out, "        from: &'static str,")?;
    writeln!(
        out,
        "        /// The target contract version, such as `0.1.0`."
    )?;
    writeln!(out, "        to: &'static str,")?;
    writeln!(
        out,
        "        /// The canonical WIT type path, such as `types/event`."
    )?;
    writeln!(out, "        path: &'static str,")?;
    writeln!(out, "        /// The WIT case name.")?;
    writeln!(out, "        case: &'static str,")?;
    writeln!(out, "    }},")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl ::core::fmt::Display for ConversionError {{")?;
    writeln!(
        out,
        "    fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {{"
    )?;
    writeln!(out, "        match self {{")?;
    writeln!(
        out,
        "            Self::MissingMember {{ from, to, path }} => write!("
    )?;
    writeln!(out, "                formatter,")?;
    writeln!(
        out,
        "                \"conversion {{from}} -> {{to}}: cannot reconstruct `{{path}}` from the source version\""
    )?;
    writeln!(out, "            ),")?;
    writeln!(out, "            Self::UnknownCase {{")?;
    writeln!(out, "                from,")?;
    writeln!(out, "                to,")?;
    writeln!(out, "                path,")?;
    writeln!(out, "                case,")?;
    writeln!(out, "            }} => write!(")?;
    writeln!(out, "                formatter,")?;
    writeln!(
        out,
        "                \"conversion {{from}} -> {{to}}: `{{path}}` case `{{case}}` does not exist in the target version\""
    )?;
    writeln!(out, "            ),")?;
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl ::std::error::Error for ConversionError {{}}")?;
    Ok(())
}

fn emit_pair(
    out: &mut String,
    source: &Version,
    target: &Version,
    direction: Direction,
    omitted: &BTreeSet<String>,
) -> Result<()> {
    let source_module = version_module_name(&source.model);
    let target_module = version_module_name(&target.model);
    let mut shared_interfaces = Vec::new();
    for source_interface in &source.model.interfaces {
        let Some(target_interface) = target.model.interface(&source_interface.name) else {
            continue;
        };
        let functions =
            shared_functions(source, target, source_interface, target_interface, omitted)?;
        if !functions.is_empty() {
            shared_interfaces.push((source_interface, target_interface, functions));
        }
    }

    writeln!(
        out,
        "/// Conversions from `{source_module}` to `{target_module}`."
    )?;
    if matches!(direction, Direction::Reverse) {
        writeln!(out, "///")?;
        writeln!(
            out,
            "/// Fields and flags added after `{}` are dropped without reporting them.",
            target.model.package.version
        )?;
    }
    if shared_interfaces.is_empty() {
        writeln!(out, "pub mod {source_module}_to_{target_module} {{}}")?;
        return Ok(());
    }
    let pair_module = format!("{source_module}_to_{target_module}");
    writeln!(out, "pub mod {pair_module} {{")?;
    // Separate the two `use` items with a blank line so Rustfmt Edition 2021 and Edition 2024
    // do not disagree on sorting `v0_2` vs `v0_10`.
    writeln!(
        out,
        "    use super::super::{source_module} as __witforge_from;"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "    use super::super::{target_module} as __witforge_to;"
    )?;

    for (source_interface, target_interface, functions) in shared_interfaces {
        let interface_module = rust_ident(&snake_case(&source_interface.name), "interface module")?;
        writeln!(out)?;
        writeln!(
            out,
            "    /// Conversions for interface `{}`.",
            source_interface.name
        )?;
        if interface_module == pair_module {
            writeln!(out, "    #[allow(clippy::module_inception)]")?;
        }
        writeln!(out, "    pub mod {interface_module} {{")?;
        writeln!(out, "        use super::super::ConversionError;")?;
        for (source_def, target_def) in functions {
            writeln!(out)?;
            emit_function(
                out,
                source,
                target,
                source_interface,
                target_interface,
                source_def,
                target_def,
                direction,
            )?;
        }
        writeln!(out, "    }}")?;
    }
    writeln!(out, "}}")?;
    Ok(())
}

/// The types defined by both versions of one interface, in the old version's source order,
/// excluding any type omitted for this direction across a major boundary.
fn shared_functions<'a>(
    source: &Version,
    target: &Version,
    old: &'a Interface,
    new: &'a Interface,
    omitted: &BTreeSet<String>,
) -> Result<Vec<(&'a TypeDef, &'a TypeDef)>> {
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut functions = Vec::new();
    for old_def in &old.types {
        if omitted.contains(&format!("{}/{}", old.name, old_def.name)) {
            continue;
        }
        let Some(new_def) = new
            .types
            .iter()
            .find(|new_def| new_def.name == old_def.name)
        else {
            continue;
        };
        let name = rust_ident(&snake_case(&old_def.name), "conversion function")?;
        if let Some(previous) = names.insert(name.clone(), old_def.name.clone()) {
            contract_bail!(
                "conversion {} -> {}: interface `{}`: conversion functions `{previous}` and `{}` \
                 both map to Rust function `{name}`",
                source.model.package.version,
                target.model.package.version,
                old.name,
                old_def.name
            );
        }
        functions.push((old_def, new_def));
    }
    Ok(functions)
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    Forward,
    Reverse,
}

fn copied(expression: &str) -> String {
    expression
        .strip_prefix('&')
        .map_or_else(|| format!("*{expression}"), str::to_owned)
}

#[allow(clippy::too_many_arguments)]
fn emit_function(
    out: &mut String,
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_def: &TypeDef,
    target_def: &TypeDef,
    direction: Direction,
) -> Result<()> {
    let function = rust_ident(&snake_case(&source_def.name), "conversion function")?;
    let source_type = rust_type(source_interface, &source_def.name, "__witforge_from")?;
    let target_type = rust_type(target_interface, &target_def.name, "__witforge_to")?;
    writeln!(
        out,
        "        /// Convert `{}/{}` from {} to {}.",
        source_interface.name,
        source_def.name,
        source.model.package.version,
        target.model.package.version
    )?;
    if matches!(direction, Direction::Reverse) {
        writeln!(out, "        ///")?;
        writeln!(
            out,
            "        /// Fields and flags added after {} are dropped without reporting them.",
            target.model.package.version
        )?;
    }
    writeln!(out, "        ///")?;
    writeln!(out, "        /// # Errors")?;
    writeln!(out, "        ///")?;
    writeln!(
        out,
        "        /// Returns an error when the source value cannot be represented by the target version."
    )?;
    let sig = rustfmt::fmt_fn_sig(
        8,
        &format!("pub fn {function}"),
        &[format!("value: &{source_type}")],
        Some(&format!("::core::result::Result<{target_type}, {ERROR}>")),
        " {",
    );
    writeln!(out, "{sig}")?;
    let body = conversion_body(
        12,
        source,
        target,
        source_interface,
        target_interface,
        source_def,
        target_def,
        direction,
    )?;
    out.push_str(&body);
    writeln!(out, "        }}")?;
    Ok(())
}

/// The body of one conversion function, with every line already indented starting at `indent`.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
fn conversion_body(
    indent: usize,
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_def: &TypeDef,
    target_def: &TypeDef,
    direction: Direction,
) -> Result<String> {
    let mut next_id = 0usize;
    match (&source_def.kind, &target_def.kind) {
        (TypeDefKind::Record(source_fields), TypeDefKind::Record(target_fields)) => record_body(
            indent,
            &mut next_id,
            source,
            target,
            source_interface,
            target_interface,
            source_def,
            source_fields,
            target_fields,
            direction,
        ),
        (TypeDefKind::Variant(source_cases), TypeDefKind::Variant(target_cases)) => variant_body(
            indent,
            &mut next_id,
            source,
            target,
            source_interface,
            target_interface,
            source_def,
            source_cases,
            target_cases,
        ),
        (TypeDefKind::Enum(source_cases), TypeDefKind::Enum(target_cases)) => enum_body(
            indent,
            source,
            target,
            source_interface,
            target_interface,
            source_def,
            source_cases,
            target_cases,
        ),
        (TypeDefKind::Flags(source_flags), TypeDefKind::Flags(target_flags)) => flags_body(
            indent,
            target_interface,
            source_def,
            source_flags,
            target_flags,
        ),
        (TypeDefKind::Alias(source_target), TypeDefKind::Alias(target_target)) => {
            emit_direct_conversion(
                indent,
                &mut next_id,
                source,
                target,
                source_interface,
                target_interface,
                source_target,
                target_target,
                &format!("{}/{}", source_interface.name, source_def.name),
            )
        }
        (TypeDefKind::List(source_target), TypeDefKind::List(target_target)) => {
            emit_direct_conversion(
                indent,
                &mut next_id,
                source,
                target,
                source_interface,
                target_interface,
                &TypeExpr::List(Box::new(source_target.clone())),
                &TypeExpr::List(Box::new(target_target.clone())),
                &format!("{}/{}", source_interface.name, source_def.name),
            )
        }
        (TypeDefKind::Option(source_target), TypeDefKind::Option(target_target)) => {
            emit_direct_conversion(
                indent,
                &mut next_id,
                source,
                target,
                source_interface,
                target_interface,
                &TypeExpr::Option(Box::new(source_target.clone())),
                &TypeExpr::Option(Box::new(target_target.clone())),
                &format!("{}/{}", source_interface.name, source_def.name),
            )
        }
        (TypeDefKind::Result(source_result), TypeDefKind::Result(target_result)) => {
            emit_direct_conversion(
                indent,
                &mut next_id,
                source,
                target,
                source_interface,
                target_interface,
                &TypeExpr::Result(source_result.clone()),
                &TypeExpr::Result(target_result.clone()),
                &format!("{}/{}", source_interface.name, source_def.name),
            )
        }
        _ => convert_bail!(
            format!("{}/{}", source_interface.name, source_def.name),
            "conversion {} -> {}: {}/{} changes kind from {} to {}; evolution requires a new type",
            source.model.package.version,
            target.model.package.version,
            source_interface.name,
            source_def.name,
            type_kind_name(&source_def.kind),
            type_kind_name(&target_def.kind)
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_direct_conversion(
    indent: usize,
    next_id: &mut usize,
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_type: &TypeExpr,
    target_type: &TypeExpr,
    path: &str,
) -> Result<String> {
    let prefix = " ".repeat(indent);
    let mut stmts = Vec::new();
    let val = emit_convert_value(
        &mut stmts,
        indent,
        next_id,
        source,
        target,
        source_interface,
        target_interface,
        source_type,
        target_type,
        "value",
        path,
        true,
    )?;
    let mut out = String::new();
    for stmt in stmts {
        writeln!(out, "{stmt}")?;
    }
    if val.returns_result {
        writeln!(out, "{prefix}{}", val.expr)?;
    } else {
        let ok_call =
            rustfmt::fmt_call(indent, "::core::result::Result::Ok", &[val.expr], indent, 0);
        writeln!(out, "{prefix}{ok_call}")?;
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn record_body(
    indent: usize,
    next_id: &mut usize,
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_def: &TypeDef,
    source_fields: &[Field],
    target_fields: &[Field],
    direction: Direction,
) -> Result<String> {
    let prefix = " ".repeat(indent);
    let target_interface_module =
        rust_ident(&snake_case(&target_interface.name), "interface module")?;
    let target_type = rust_ident(&pascal_case(&source_def.name), "type")?;
    let target_path = format!("super::__witforge_to::{target_interface_module}::{target_type}");
    let type_path = format!("{}/{}", source_interface.name, source_def.name);
    let from_ver = source.model.package.version;
    let to_ver = target.model.package.version;

    // Validate shared fields even when a removed required field makes the reverse conversion
    // impossible: a retyped shared field is still a contract error of its own.
    for target_field in target_fields {
        if let Some(source_field) = source_fields
            .iter()
            .find(|source_field| source_field.name == target_field.name)
        {
            let source_name = rust_ident(&snake_case(&source_field.name), "record field")?;
            let mut scratch = Vec::new();
            let mut scratch_id = 0;
            emit_convert_value(
                &mut scratch,
                indent,
                &mut scratch_id,
                source,
                target,
                source_interface,
                target_interface,
                &source_field.ty,
                &target_field.ty,
                &format!("&value.{source_name}"),
                &format!("{type_path}.{}", source_field.name),
                false,
            )?;
        }
    }

    let mut missing = None;
    for target_field in target_fields {
        if source_fields
            .iter()
            .any(|source_field| source_field.name == target_field.name)
        {
            continue;
        }
        if default_expr(&target.model, &target_field.ty)?.is_none() {
            missing = Some(target_field);
            break;
        }
    }
    if let Some(missing) = missing {
        match direction {
            Direction::Forward => convert_bail!(
                format!("{type_path}.{}", missing.name),
                "conversion {from_ver} -> {to_ver}: {type_path}.{} is required in {to_ver} but \
                 absent from {from_ver}; make it `option<T>` or `list<T>` so an older value can \
                 be defaulted, or add a new member instead of changing this one",
                missing.name,
            ),
            Direction::Reverse => {
                let err = rustfmt::fmt_struct_lit(
                    indent,
                    &format!("{ERROR}::MissingMember"),
                    &[
                        ("from".to_string(), format!("{:?}", from_ver.to_string())),
                        ("to".to_string(), format!("{:?}", to_ver.to_string())),
                        (
                            "path".to_string(),
                            format!("{:?}", format!("{type_path}.{}", missing.name)),
                        ),
                    ],
                    indent + "::core::result::Result::Err(".len(),
                    1,
                );
                return Ok(format!(
                    "{prefix}let _ = value;\n{prefix}::core::result::Result::Err({err})\n"
                ));
            }
        }
    }

    let mut stmts = Vec::new();
    let mut fields = Vec::new();
    let mut used_value = false;
    for target_field in target_fields {
        let target_name = rust_ident(&snake_case(&target_field.name), "record field")?;
        if let Some(source_field) = source_fields
            .iter()
            .find(|source_field| source_field.name == target_field.name)
        {
            used_value = true;
            let source_name = rust_ident(&snake_case(&source_field.name), "record field")?;
            let val = emit_convert_value(
                &mut stmts,
                indent,
                next_id,
                source,
                target,
                source_interface,
                target_interface,
                &source_field.ty,
                &target_field.ty,
                &format!("&value.{source_name}"),
                &format!("{type_path}.{}", source_field.name),
                false,
            )?;
            let mut field_expr = val.expr;
            if indent + 4 + target_name.len() + 2 + field_expr.len() + 1 > rustfmt::MAX_WIDTH {
                let tmp = fresh(next_id);
                stmts.push(rustfmt::fmt_let_stmt(indent, &tmp, &field_expr));
                field_expr = tmp;
            }
            fields.push((target_name, field_expr));
        } else {
            let Some(default) = default_expr(&target.model, &target_field.ty)? else {
                bail!(
                    "no default for target field `{}` in {type_path}",
                    target_field.name
                );
            };
            fields.push((target_name, default.to_string()));
        }
    }
    let mut out = String::new();
    if !used_value {
        writeln!(out, "{prefix}let _ = value;")?;
    }
    for stmt in stmts {
        writeln!(out, "{stmt}")?;
    }
    let lit = rustfmt::fmt_struct_lit(
        indent,
        &target_path,
        &fields,
        indent + "::core::result::Result::Ok(".len(),
        1,
    );
    writeln!(out, "{prefix}::core::result::Result::Ok({lit})")?;
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
fn variant_body(
    indent: usize,
    next_id: &mut usize,
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_def: &TypeDef,
    source_cases: &[Case],
    target_cases: &[Case],
) -> Result<String> {
    let prefix = " ".repeat(indent);
    let arm_indent = indent + 4;
    let body_indent = indent + 8;
    let source_interface_module =
        rust_ident(&snake_case(&source_interface.name), "interface module")?;
    let target_interface_module =
        rust_ident(&snake_case(&target_interface.name), "interface module")?;
    let source_type = rust_ident(&pascal_case(&source_def.name), "type")?;
    let target_type = rust_ident(&pascal_case(&source_def.name), "type")?;
    let source_path = format!("super::__witforge_from::{source_interface_module}::{source_type}");
    let target_path = format!("super::__witforge_to::{target_interface_module}::{target_type}");
    let type_path = format!("{}/{}", source_interface.name, source_def.name);
    let from_ver = source.model.package.version;
    let to_ver = target.model.package.version;

    let mut out = String::new();
    writeln!(out, "{prefix}match value {{")?;
    for source_case in source_cases {
        let source_case_name = rust_ident(&pascal_case(&source_case.name), "variant case")?;
        let target_case = target_cases
            .iter()
            .find(|target_case| target_case.name == source_case.name);
        if let Some(target_case) = target_case {
            match (&source_case.payload, &target_case.payload) {
                (Some(_), None) | (None, Some(_)) => convert_bail!(
                    format!("{type_path}.{}", source_case.name),
                    "conversion {from_ver} -> {to_ver}: {type_path} case `{}` changes payload \
                     shape; evolution requires a new case",
                    source_case.name
                ),
                _ => {}
            }
        }
        let pat = match (target_case, &source_case.payload) {
            (Some(_), Some(_)) => format!("{source_path}::{source_case_name}(payload)"),
            (None, Some(_)) => format!("{source_path}::{source_case_name}(_)"),
            (_, None) => format!("{source_path}::{source_case_name}"),
        };
        let arm = match (target_case, &source_case.payload) {
            (None, _) => {
                let err = rustfmt::fmt_struct_lit(
                    body_indent,
                    &format!("{ERROR}::UnknownCase"),
                    &[
                        ("from".to_string(), format!("{:?}", from_ver.to_string())),
                        ("to".to_string(), format!("{:?}", to_ver.to_string())),
                        ("path".to_string(), format!("{type_path:?}")),
                        ("case".to_string(), format!("{:?}", source_case.name)),
                    ],
                    body_indent + "::core::result::Result::Err(".len(),
                    1,
                );
                rustfmt::fmt_match_arm(
                    arm_indent,
                    &pat,
                    &[],
                    &format!("::core::result::Result::Err({err})"),
                )
            }
            (Some(target_case), Some(source_payload)) => {
                let target_case_name = rust_ident(&pascal_case(&target_case.name), "variant case")?;
                let target_payload = target_case
                    .payload
                    .as_ref()
                    .expect("payload presence was checked above");
                let mut stmts = Vec::new();
                let val = emit_convert_value(
                    &mut stmts,
                    body_indent,
                    next_id,
                    source,
                    target,
                    source_interface,
                    target_interface,
                    source_payload,
                    target_payload,
                    "payload",
                    &format!("{type_path}.{}", source_case.name),
                    false,
                )?;
                let mut inner = val.expr;
                let ctor = format!("{target_path}::{target_case_name}");
                let ok_single = format!("::core::result::Result::Ok({ctor}({inner}))");
                if stmts.is_empty() && body_indent + ok_single.len() > rustfmt::MAX_WIDTH {
                    let tmp = fresh(next_id);
                    stmts.push(rustfmt::fmt_let_stmt(body_indent, &tmp, &inner));
                    inner = tmp;
                }
                let ctor_call = rustfmt::fmt_call(
                    body_indent,
                    &ctor,
                    &[inner],
                    body_indent + "::core::result::Result::Ok(".len(),
                    1,
                );
                let tail = format!("::core::result::Result::Ok({ctor_call})");
                rustfmt::fmt_match_arm(arm_indent, &pat, &stmts, &tail)
            }
            (Some(target_case), None) => {
                let target_case_name = rust_ident(&pascal_case(&target_case.name), "variant case")?;
                let tail = format!("::core::result::Result::Ok({target_path}::{target_case_name})");
                rustfmt::fmt_match_arm(arm_indent, &pat, &[], &tail)
            }
        };
        writeln!(out, "{arm}")?;
    }
    writeln!(out, "{prefix}}}")?;
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn enum_body(
    indent: usize,
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_def: &TypeDef,
    source_cases: &[Case],
    target_cases: &[Case],
) -> Result<String> {
    let prefix = " ".repeat(indent);
    let arm_indent = indent + 4;
    let body_indent = indent + 8;
    let source_interface_module =
        rust_ident(&snake_case(&source_interface.name), "interface module")?;
    let target_interface_module =
        rust_ident(&snake_case(&target_interface.name), "interface module")?;
    let source_type = rust_ident(&pascal_case(&source_def.name), "type")?;
    let target_type = rust_ident(&pascal_case(&source_def.name), "type")?;
    let source_path = format!("super::__witforge_from::{source_interface_module}::{source_type}");
    let target_path = format!("super::__witforge_to::{target_interface_module}::{target_type}");
    let type_path = format!("{}/{}", source_interface.name, source_def.name);
    let from_ver = source.model.package.version;
    let to_ver = target.model.package.version;

    let mut out = String::new();
    writeln!(out, "{prefix}match value {{")?;
    for source_case in source_cases {
        let source_case_name = rust_ident(&pascal_case(&source_case.name), "enum case")?;
        let target_case_name = target_cases
            .iter()
            .find(|target_case| target_case.name == source_case.name)
            .map(|case| rust_ident(&pascal_case(&case.name), "enum case"))
            .transpose()?;
        let pat = format!("{source_path}::{source_case_name}");
        let arm = if let Some(target_case_name) = target_case_name {
            let tail = format!("::core::result::Result::Ok({target_path}::{target_case_name})");
            rustfmt::fmt_match_arm(arm_indent, &pat, &[], &tail)
        } else {
            let err = rustfmt::fmt_struct_lit(
                body_indent,
                &format!("{ERROR}::UnknownCase"),
                &[
                    ("from".to_string(), format!("{:?}", from_ver.to_string())),
                    ("to".to_string(), format!("{:?}", to_ver.to_string())),
                    ("path".to_string(), format!("{type_path:?}")),
                    ("case".to_string(), format!("{:?}", source_case.name)),
                ],
                body_indent + "::core::result::Result::Err(".len(),
                1,
            );
            rustfmt::fmt_match_arm(
                arm_indent,
                &pat,
                &[],
                &format!("::core::result::Result::Err({err})"),
            )
        };
        writeln!(out, "{arm}")?;
    }
    writeln!(out, "{prefix}}}")?;
    Ok(out)
}

fn flags_body(
    indent: usize,
    target_interface: &Interface,
    source_def: &TypeDef,
    source_flags: &[crate::model::Flag],
    target_flags: &[crate::model::Flag],
) -> Result<String> {
    let prefix = " ".repeat(indent);
    let target_interface_module =
        rust_ident(&snake_case(&target_interface.name), "interface module")?;
    let target_type = rust_ident(&pascal_case(&source_def.name), "type")?;
    let target_path = format!("super::__witforge_to::{target_interface_module}::{target_type}");
    let mut fields = Vec::new();
    let mut used_value = false;
    for target_flag in target_flags {
        let target_name = rust_ident(&snake_case(&target_flag.name), "flag")?;
        let val = if let Some(source_flag) = source_flags
            .iter()
            .find(|source_flag| source_flag.name == target_flag.name)
        {
            used_value = true;
            let source_name = rust_ident(&snake_case(&source_flag.name), "flag")?;
            format!("value.{source_name}")
        } else {
            "false".to_string()
        };
        fields.push((target_name, val));
    }
    let mut out = String::new();
    if !used_value {
        writeln!(out, "{prefix}let _ = value;")?;
    }
    let lit = rustfmt::fmt_struct_lit(
        indent,
        &target_path,
        &fields,
        indent + "::core::result::Result::Ok(".len(),
        1,
    );
    writeln!(out, "{prefix}::core::result::Result::Ok({lit})")?;
    Ok(out)
}

fn fresh(next_id: &mut usize) -> String {
    let id = *next_id;
    *next_id += 1;
    format!("v{id}")
}

struct ValueExpr {
    expr: String,
    /// When `tail_result` is true and the conversion directly calls a function returning
    /// `Result<Target, ConversionError>`, we return that call directly without `Ok(...?)`.
    returns_result: bool,
}

fn is_infallible_pair(
    source: &Model,
    target: &Model,
    source_type: &TypeExpr,
    target_type: &TypeExpr,
) -> bool {
    match (source_type, target_type) {
        (TypeExpr::Bool, TypeExpr::Bool)
        | (TypeExpr::U8, TypeExpr::U8)
        | (TypeExpr::U16, TypeExpr::U16)
        | (TypeExpr::U32, TypeExpr::U32)
        | (TypeExpr::U64, TypeExpr::U64)
        | (TypeExpr::S8, TypeExpr::S8)
        | (TypeExpr::S16, TypeExpr::S16)
        | (TypeExpr::S32, TypeExpr::S32)
        | (TypeExpr::S64, TypeExpr::S64)
        | (TypeExpr::F32, TypeExpr::F32)
        | (TypeExpr::F64, TypeExpr::F64)
        | (TypeExpr::String, TypeExpr::String) => true,
        (TypeExpr::Named(a), TypeExpr::Named(b)) if a == b => a
            .package()
            .is_some_and(|(ns, pkg)| same_effective_dependency(source, target, ns, pkg)),
        (TypeExpr::List(a), TypeExpr::List(b)) | (TypeExpr::Option(a), TypeExpr::Option(b)) => {
            is_infallible_pair(source, target, a, b)
        }
        (TypeExpr::Result(a), TypeExpr::Result(b)) => {
            let ok = match (&a.ok, &b.ok) {
                (Some(sa), Some(sb)) => is_infallible_pair(source, target, sa, sb),
                (None, None) => true,
                _ => false,
            };
            let err = match (&a.err, &b.err) {
                (Some(sa), Some(sb)) => is_infallible_pair(source, target, sa, sb),
                (None, None) => true,
                _ => false,
            };
            ok && err
        }
        _ => false,
    }
}

/// Expand aliases and named structural definitions to the expression they project.
fn qualify_name(context: Option<&Dependency>, name: &QualifiedName) -> QualifiedName {
    if name.package().is_some() {
        return name.clone();
    }
    let Some(dependency) = context else {
        return name.clone();
    };
    QualifiedName {
        interface: format!(
            "{}:{}/{}",
            dependency.package.namespace, dependency.package.name, name.interface
        ),
        name: name.name.clone(),
    }
}

fn resolve_expr(
    model: &Model,
    context: Option<&Dependency>,
    ty: &TypeExpr,
    role: &str,
    visiting: &mut HashSet<String>,
) -> Result<TypeExpr> {
    match ty {
        TypeExpr::Named(name) => {
            let Some((resolved, def)) = model.type_def_in(context, name) else {
                if role == "source" {
                    bail!("type `{name}` is not in the source model");
                }
                bail!("type `{name}` is missing from the target version");
            };
            let key = resolved.map_or_else(
                || name.to_string(),
                |dependency| {
                    format!(
                        "{}:{}/{}",
                        dependency.package.namespace, dependency.package.name, name
                    )
                },
            );
            match &def.kind {
                TypeDefKind::Alias(target) => {
                    if !visiting.insert(key.clone()) {
                        bail!("alias cycle at `{name}`");
                    }
                    let resolved = resolve_expr(model, resolved, target, role, visiting);
                    visiting.remove(&key);
                    resolved
                }
                TypeDefKind::List(element) => Ok(TypeExpr::List(Box::new(resolve_expr(
                    model, resolved, element, role, visiting,
                )?))),
                TypeDefKind::Option(payload) => Ok(TypeExpr::Option(Box::new(resolve_expr(
                    model, resolved, payload, role, visiting,
                )?))),
                TypeDefKind::Result(result) => Ok(TypeExpr::Result(ResultExpr {
                    ok: result
                        .ok
                        .as_deref()
                        .map(|ty| resolve_expr(model, resolved, ty, role, visiting).map(Box::new))
                        .transpose()?,
                    err: result
                        .err
                        .as_deref()
                        .map(|ty| resolve_expr(model, resolved, ty, role, visiting).map(Box::new))
                        .transpose()?,
                })),
                TypeDefKind::Record(_)
                | TypeDefKind::Variant(_)
                | TypeDefKind::Enum(_)
                | TypeDefKind::Flags(_) => Ok(TypeExpr::Named(qualify_name(context, name))),
            }
        }
        TypeExpr::List(element) => Ok(TypeExpr::List(Box::new(resolve_expr(
            model, context, element, role, visiting,
        )?))),
        TypeExpr::Option(payload) => Ok(TypeExpr::Option(Box::new(resolve_expr(
            model, context, payload, role, visiting,
        )?))),
        TypeExpr::Result(result) => Ok(TypeExpr::Result(ResultExpr {
            ok: result
                .ok
                .as_deref()
                .map(|ty| resolve_expr(model, context, ty, role, visiting).map(Box::new))
                .transpose()?,
            err: result
                .err
                .as_deref()
                .map(|ty| resolve_expr(model, context, ty, role, visiting).map(Box::new))
                .transpose()?,
        })),
        _ => Ok(ty.clone()),
    }
}

/// Emit statements converting borrowed `expression` from `source_type` to `target_type` and
/// return a short expression holding the converted value.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn emit_convert_value(
    stmts: &mut Vec<String>,
    indent: usize,
    next_id: &mut usize,
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_type: &TypeExpr,
    target_type: &TypeExpr,
    expression: &str,
    path: &str,
    tail_result: bool,
) -> Result<ValueExpr> {
    let from_ver = source.model.package.version;
    let to_ver = target.model.package.version;
    let source_type = resolve_expr(
        &source.model,
        None,
        source_type,
        "source",
        &mut HashSet::new(),
    )
    .with_context(|| format!("conversion {from_ver} -> {to_ver}"))?;
    let target_type = resolve_expr(
        &target.model,
        None,
        target_type,
        "target",
        &mut HashSet::new(),
    )
    .with_context(|| format!("conversion {from_ver} -> {to_ver}"))?;
    match (&source_type, &target_type) {
        (TypeExpr::Bool, TypeExpr::Bool)
        | (TypeExpr::U8, TypeExpr::U8)
        | (TypeExpr::U16, TypeExpr::U16)
        | (TypeExpr::U32, TypeExpr::U32)
        | (TypeExpr::U64, TypeExpr::U64)
        | (TypeExpr::S8, TypeExpr::S8)
        | (TypeExpr::S16, TypeExpr::S16)
        | (TypeExpr::S32, TypeExpr::S32)
        | (TypeExpr::S64, TypeExpr::S64)
        | (TypeExpr::F32, TypeExpr::F32)
        | (TypeExpr::F64, TypeExpr::F64) => Ok(ValueExpr {
            expr: copied(expression),
            returns_result: false,
        }),
        (TypeExpr::String, TypeExpr::String) => Ok(ValueExpr {
            expr: format!("::core::clone::Clone::clone({expression})"),
            returns_result: false,
        }),
        (TypeExpr::Named(source_name), TypeExpr::Named(target_name))
            if source_name == target_name =>
        {
            if source.model.type_def(source_name).is_none() {
                bail!("type `{source_name}` is not in the source model");
            }
            if target.model.type_def(target_name).is_none() {
                convert_bail!(
                    path,
                    "conversion {from_ver} -> {to_ver}: {path} names `{target_name}`, which is \
                     missing from the target version; evolution requires a new type instead of \
                     removing a referenced type"
                );
            }
            if source_name.is_foreign() || target_name.is_foreign() {
                let same_dep = source_name.package().is_some_and(|(ns, pkg)| {
                    same_effective_dependency(&source.model, &target.model, ns, pkg)
                });
                if !same_dep {
                    convert_bail!(
                        path,
                        "conversion {from_ver} -> {to_ver}: {path} uses foreign type \
                         `{source_name}` from a dependency that differs between versions"
                    );
                }
                return Ok(ValueExpr {
                    expr: format!("::core::clone::Clone::clone({expression})"),
                    returns_result: false,
                });
            }
            let interface_module =
                rust_ident(&snake_case(&source_name.interface), "interface module")?;
            let function = rust_ident(&snake_case(&source_name.name), "conversion function")?;
            let callee = format!("super::{interface_module}::{function}");
            if tail_result {
                let call = rustfmt::fmt_call(indent, &callee, &[expression.to_string()], indent, 0);
                return Ok(ValueExpr {
                    expr: call,
                    returns_result: true,
                });
            }
            let tmp = fresh(next_id);
            stmts.push(rustfmt::fmt_let_call(
                indent,
                &tmp,
                &callee,
                &[expression.to_string()],
                true,
            ));
            Ok(ValueExpr {
                expr: tmp,
                returns_result: false,
            })
        }
        (TypeExpr::List(source_element), TypeExpr::List(target_element)) => {
            if is_infallible_pair(&source.model, &target.model, source_element, target_element) {
                return Ok(ValueExpr {
                    expr: format!("::core::clone::Clone::clone({expression})"),
                    returns_result: false,
                });
            }
            let prefix = " ".repeat(indent);
            let inner_indent = indent + 4;
            let inner_prefix = " ".repeat(inner_indent);
            let out_var = fresh(next_id);
            let item_var = fresh(next_id);
            let iter_src = expression.strip_prefix('&').unwrap_or(expression);
            stmts.push(rustfmt::fmt_let_stmt(
                indent,
                &format!("mut {out_var}"),
                &format!("::std::vec::Vec::with_capacity({iter_src}.len())"),
            ));
            let mut body_stmts = Vec::new();
            let elem = emit_convert_value(
                &mut body_stmts,
                inner_indent,
                next_id,
                source,
                target,
                source_interface,
                target_interface,
                source_element,
                target_element,
                &item_var,
                path,
                false,
            )?;
            let mut loop_code = format!("{prefix}for {item_var} in {expression} {{\n");
            for s in body_stmts {
                let _ = writeln!(loop_code, "{s}");
            }
            let push_call = rustfmt::fmt_call(
                inner_indent,
                &format!("{out_var}.push"),
                &[elem.expr],
                inner_indent,
                1,
            );
            let _ = writeln!(loop_code, "{inner_prefix}{push_call};");
            let _ = write!(loop_code, "{prefix}}}");
            stmts.push(loop_code);
            Ok(ValueExpr {
                expr: out_var,
                returns_result: false,
            })
        }
        (TypeExpr::Option(source_payload), TypeExpr::Option(target_payload)) => {
            if is_infallible_pair(&source.model, &target.model, source_payload, target_payload) {
                return Ok(ValueExpr {
                    expr: copy_or_clone(source_payload, expression),
                    returns_result: false,
                });
            }
            let prefix = " ".repeat(indent);
            let inner_indent = indent + 4;
            let inner_prefix = " ".repeat(inner_indent);
            let out_var = fresh(next_id);
            let item_var = fresh(next_id);
            stmts.push(format!(
                "{prefix}let mut {out_var} = ::core::option::Option::None;"
            ));
            let mut body_stmts = Vec::new();
            let elem = emit_convert_value(
                &mut body_stmts,
                inner_indent,
                next_id,
                source,
                target,
                source_interface,
                target_interface,
                source_payload,
                target_payload,
                &item_var,
                path,
                false,
            )?;
            let mut if_code = format!(
                "{prefix}if let ::core::option::Option::Some({item_var}) = {expression} {{\n"
            );
            for s in body_stmts {
                let _ = writeln!(if_code, "{s}");
            }
            let some_call = rustfmt::fmt_call(
                inner_indent,
                "::core::option::Option::Some",
                &[elem.expr],
                inner_indent + out_var.len() + 3,
                1,
            );
            let _ = writeln!(if_code, "{inner_prefix}{out_var} = {some_call};");
            let _ = write!(if_code, "{prefix}}}");
            stmts.push(if_code);
            Ok(ValueExpr {
                expr: out_var,
                returns_result: false,
            })
        }
        (TypeExpr::Result(source_result), TypeExpr::Result(target_result)) => emit_result_value(
            stmts,
            indent,
            next_id,
            source,
            target,
            source_interface,
            target_interface,
            source_result,
            target_result,
            expression,
            path,
        ),
        _ => convert_bail!(
            path,
            "conversion {from_ver} -> {to_ver}: {} changes type from `{}` to `{}`; evolution \
             requires a new member",
            path,
            render_type(&source_type, TypeRenderStyle::Spaced),
            render_type(&target_type, TypeRenderStyle::Spaced)
        ),
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
fn emit_result_value(
    stmts: &mut Vec<String>,
    indent: usize,
    next_id: &mut usize,
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_result: &ResultExpr,
    target_result: &ResultExpr,
    expression: &str,
    path: &str,
) -> Result<ValueExpr> {
    let from_ver = source.model.package.version;
    let to_ver = target.model.package.version;
    // Validate both slots before checking infallibility so shape/type mismatches are refused.
    let mut check_ok_infallible = true;
    match (&source_result.ok, &target_result.ok) {
        (Some(sa), Some(sb)) => {
            let ra = resolve_expr(&source.model, None, sa, "source", &mut HashSet::new())?;
            let rb = resolve_expr(&target.model, None, sb, "target", &mut HashSet::new())?;
            check_ok_infallible = is_infallible_pair(&source.model, &target.model, &ra, &rb);
            let mut scratch = Vec::new();
            let mut scratch_id = 0;
            emit_convert_value(
                &mut scratch,
                indent,
                &mut scratch_id,
                source,
                target,
                source_interface,
                target_interface,
                sa,
                sb,
                "ok",
                path,
                false,
            )?;
        }
        (None, None) => {}
        _ => {
            convert_bail!(
                path,
                "conversion {from_ver} -> {to_ver}: {path} changes its result success shape; \
                 evolution requires a new type"
            )
        }
    }
    let mut check_err_infallible = true;
    match (&source_result.err, &target_result.err) {
        (Some(sa), Some(sb)) => {
            let ra = resolve_expr(&source.model, None, sa, "source", &mut HashSet::new())?;
            let rb = resolve_expr(&target.model, None, sb, "target", &mut HashSet::new())?;
            check_err_infallible = is_infallible_pair(&source.model, &target.model, &ra, &rb);
            let mut scratch = Vec::new();
            let mut scratch_id = 0;
            emit_convert_value(
                &mut scratch,
                indent,
                &mut scratch_id,
                source,
                target,
                source_interface,
                target_interface,
                sa,
                sb,
                "err",
                path,
                false,
            )?;
        }
        (None, None) => {}
        _ => convert_bail!(
            path,
            "conversion {from_ver} -> {to_ver}: {path} changes its result error shape; evolution \
             requires a new type"
        ),
    }
    if check_ok_infallible && check_err_infallible {
        let resolved_result = TypeExpr::Result(ResultExpr {
            ok: source_result.ok.clone(),
            err: source_result.err.clone(),
        });
        return Ok(ValueExpr {
            expr: copy_or_clone(&resolved_result, expression),
            returns_result: false,
        });
    }

    let prefix = " ".repeat(indent);
    let arm_indent = indent + 4;
    let body_indent = indent + 8;
    let out_var = fresh(next_id);

    let ok_arm = if let (Some(sa), Some(sb)) = (&source_result.ok, &target_result.ok) {
        let ok_var = fresh(next_id);
        let mut arm_stmts = Vec::new();
        let val = emit_convert_value(
            &mut arm_stmts,
            body_indent,
            next_id,
            source,
            target,
            source_interface,
            target_interface,
            sa,
            sb,
            &ok_var,
            path,
            false,
        )?;
        rustfmt::fmt_match_arm(
            arm_indent,
            &format!("::core::result::Result::Ok({ok_var})"),
            &arm_stmts,
            &format!("::core::result::Result::Ok({})", val.expr),
        )
    } else {
        rustfmt::fmt_match_arm(
            arm_indent,
            "::core::result::Result::Ok(_)",
            &[],
            "::core::result::Result::Ok(())",
        )
    };

    let err_arm = if let (Some(sa), Some(sb)) = (&source_result.err, &target_result.err) {
        let err_var = fresh(next_id);
        let mut arm_stmts = Vec::new();
        let val = emit_convert_value(
            &mut arm_stmts,
            body_indent,
            next_id,
            source,
            target,
            source_interface,
            target_interface,
            sa,
            sb,
            &err_var,
            path,
            false,
        )?;
        rustfmt::fmt_match_arm(
            arm_indent,
            &format!("::core::result::Result::Err({err_var})"),
            &arm_stmts,
            &format!("::core::result::Result::Err({})", val.expr),
        )
    } else {
        rustfmt::fmt_match_arm(
            arm_indent,
            "::core::result::Result::Err(_)",
            &[],
            "::core::result::Result::Err(())",
        )
    };

    stmts.push(format!(
        "{prefix}let {out_var} = match {expression} {{\n{ok_arm}\n{err_arm}\n{prefix}}};"
    ));
    Ok(ValueExpr {
        expr: out_var,
        returns_result: false,
    })
}

/// Whether an already-resolved WIT type expression maps to a `Copy` Rust type.
///
/// Only primitives (except `string`), unit `()` (empty result sides), `Option<T>` where `T: Copy`,
/// and `Result<O, E>` where `O: Copy` and `E: Copy` implement `Copy`; generated records, variants,
/// enums, and flags derive `Clone` without `Copy`, and `list<T>` / `string` own heap allocations.
fn is_copy_resolved(ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Bool
        | TypeExpr::U8
        | TypeExpr::U16
        | TypeExpr::U32
        | TypeExpr::U64
        | TypeExpr::S8
        | TypeExpr::S16
        | TypeExpr::S32
        | TypeExpr::S64
        | TypeExpr::F32
        | TypeExpr::F64 => true,
        TypeExpr::String | TypeExpr::Named(_) | TypeExpr::List(_) => false,
        TypeExpr::Option(inner) => is_copy_resolved(inner),
        TypeExpr::Result(result) => {
            result.ok.as_deref().is_none_or(is_copy_resolved)
                && result.err.as_deref().is_none_or(is_copy_resolved)
        }
    }
}

fn copy_or_clone(resolved: &TypeExpr, expression: &str) -> String {
    if is_copy_resolved(resolved) {
        copied(expression)
    } else {
        format!("::core::clone::Clone::clone({expression})")
    }
}

/// A Rust type path for `ty` in `version`, relative to the pair module that imports that
/// version under `__witforge_from` or `__witforge_to`.
fn rust_type(interface: &Interface, name: &str, version_alias: &str) -> Result<String> {
    let interface_module = rust_ident(&snake_case(&interface.name), "interface module")?;
    let type_name = rust_ident(&pascal_case(name), "type")?;
    Ok(format!(
        "super::{version_alias}::{interface_module}::{type_name}"
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::emit::versions::Version;
    use crate::lock::WireLock;
    use crate::model::{
        Interface, Model, Package, QualifiedName, ResultExpr, Stability, TypeDef, TypeDefKind,
        TypeExpr,
    };

    fn package(minor: u64) -> Package {
        Package {
            namespace: "example".to_string(),
            name: "convert".to_string(),
            version: crate::model::Version {
                major: 0,
                minor,
                patch: 0,
            },
            docs: String::new(),
        }
    }

    fn type_def(name: &str, kind: TypeDefKind) -> TypeDef {
        TypeDef {
            name: name.to_string(),
            docs: String::new(),
            stability: Stability::Unknown,
            kind,
        }
    }

    fn version(minor: u64, types: Vec<TypeDef>) -> Version {
        Version {
            major: 0,
            directory: PathBuf::from(format!("v0.{minor}")),
            model: Model {
                package: package(minor),
                interfaces: vec![Interface {
                    name: "types".to_string(),
                    docs: String::new(),
                    stability: Stability::Unknown,
                    types,
                    functions: Vec::new(),
                }],
                worlds: Vec::new(),
                dependencies: Vec::new(),
            },
            lock: WireLock::default(),
            locations: BTreeMap::new(),
        }
    }

    fn one_type(minor: u64, kind: TypeDefKind) -> Version {
        version(minor, vec![type_def("thing", kind)])
    }

    #[test]
    fn conversion_refuses_a_kind_change() {
        let error = plan(&[
            one_type(1, TypeDefKind::List(TypeExpr::U8)),
            one_type(2, TypeDefKind::Option(TypeExpr::U8)),
        ])
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("0.1.0 -> 0.2.0"), "{message}");
        assert!(
            message.contains("changes kind from list to option"),
            "{message}"
        );
    }

    #[test]
    fn conversion_refuses_a_result_shape_change() {
        let error = plan(&[
            one_type(
                1,
                TypeDefKind::Result(ResultExpr {
                    ok: Some(Box::new(TypeExpr::U8)),
                    err: Some(Box::new(TypeExpr::String)),
                }),
            ),
            one_type(
                2,
                TypeDefKind::Result(ResultExpr {
                    ok: Some(Box::new(TypeExpr::U8)),
                    err: None,
                }),
            ),
        ])
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("0.1.0 -> 0.2.0"), "{message}");
        assert!(
            message.contains("changes its result error shape"),
            "{message}"
        );
    }

    #[test]
    fn conversion_refuses_missing_named_types() {
        let missing = QualifiedName {
            interface: "types".to_string(),
            name: "missing".to_string(),
        };
        let error = plan(&[
            one_type(1, TypeDefKind::Alias(TypeExpr::Named(missing.clone()))),
            one_type(2, TypeDefKind::Alias(TypeExpr::Named(missing))),
        ])
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("not in the source model"), "{message}");

        let leaf = QualifiedName {
            interface: "types".to_string(),
            name: "leaf".to_string(),
        };
        let error = plan(&[
            version(
                1,
                vec![
                    type_def("thing", TypeDefKind::Alias(TypeExpr::Named(leaf.clone()))),
                    type_def("leaf", TypeDefKind::Alias(TypeExpr::U8)),
                ],
            ),
            version(
                2,
                vec![type_def("thing", TypeDefKind::Alias(TypeExpr::Named(leaf)))],
            ),
        ])
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("0.1.0 -> 0.2.0"), "{message}");
        assert!(
            message.contains("missing from the target version"),
            "{message}"
        );
    }

    #[test]
    fn conversion_refuses_a_retyped_shared_field() {
        let field = |ty| Field {
            name: "value".to_string(),
            docs: String::new(),
            ty,
        };
        let error = plan(&[
            one_type(1, TypeDefKind::Record(vec![field(TypeExpr::U8)])),
            one_type(2, TypeDefKind::Record(vec![field(TypeExpr::U64)])),
        ])
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("0.1.0 -> 0.2.0"), "{message}");
        assert!(
            message.contains("changes type from `u8` to `u64`"),
            "{message}"
        );
    }
}
