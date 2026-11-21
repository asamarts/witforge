//! Generated conversions between consecutive contract versions.
//!
//! The plain Rust emitter generates one module per version. A host that supports several
//! versions also needs adapters between them, so this module writes a `convert` module for every
//! adjacent pair of version directories, including a major-version boundary. The conversions are
//! deliberately explicit and fallible: evolution can remove a field or a case, and a conversion
//! must report that it cannot reconstruct it instead of inventing a value.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;

use anyhow::{Context, Result, bail};

use crate::emit::defaults::default_expr;
use crate::emit::rust::{header_for, version_module_name};
use crate::emit::versions::Version;
use crate::emit::{
    TypeRenderStyle, pascal_case, render_type, rust_ident, snake_case, type_kind_name,
};
use crate::model::{
    Case, Dependency, Field, Interface, Model, QualifiedName, ResultExpr, TypeDef, TypeDefKind,
    TypeExpr,
};

/// The shared error type generated in `convert.rs`.
const ERROR: &str = "super::super::ConversionError";

/// Plan the `convert.rs` file for `versions`, or `None` when there are fewer than two versions.
pub(crate) fn plan(versions: &[Version]) -> Result<Option<String>> {
    let pairs: Vec<(&Version, &Version)> = versions
        .windows(2)
        .map(|pair| (&pair[0], &pair[1]))
        .collect();
    if pairs.is_empty() {
        return Ok(None);
    }

    let package = &versions
        .first()
        .context("no version directories found under WIT root")?
        .model
        .package;
    let mut out = String::new();
    writeln!(out, "{}", header_for(&package.namespace, &package.name))?;
    writeln!(out, "// source: {}:{}", package.namespace, package.name)?;
    writeln!(out)?;
    write_error(&mut out)?;
    for (index, &(old, new)) in pairs.iter().enumerate() {
        emit_pair(&mut out, old, new, Direction::Forward)?;
        writeln!(out)?;
        emit_pair(&mut out, new, old, Direction::Reverse)?;
        if index + 1 != pairs.len() {
            writeln!(out)?;
        }
    }
    Ok(Some(out))
}

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
        "            Self::MissingMember {{ from, to, path }} => write!(formatter, \"conversion {{from}} -> {{to}}: cannot reconstruct `{{path}}` from the source version\"),"
    )?;
    writeln!(
        out,
        "            Self::UnknownCase {{ from, to, path, case }} => write!(formatter, \"conversion {{from}} -> {{to}}: `{{path}}` case `{{case}}` does not exist in the target version\"),"
    )?;
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl ::std::error::Error for ConversionError {{}}")?;
    writeln!(out)?;
    Ok(())
}

fn emit_pair(
    out: &mut String,
    source: &Version,
    target: &Version,
    direction: Direction,
) -> Result<()> {
    let source_module = version_module_name(&source.model);
    let target_module = version_module_name(&target.model);
    let mut shared_interfaces = Vec::new();
    for source_interface in &source.model.interfaces {
        let Some(target_interface) = target.model.interface(&source_interface.name) else {
            continue;
        };
        let functions = shared_functions(source, target, source_interface, target_interface)?;
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
    writeln!(out, "pub mod {source_module}_to_{target_module} {{")?;
    // The aliases cannot collide with a WIT interface module: WIT identifiers start with a
    // letter, while these start with `_`. Both aliases are used by every generated conversion
    // function signature in this module.
    writeln!(
        out,
        "    use super::super::{{ {source_module} as __witforge_from, {target_module} as __witforge_to }};"
    )?;
    writeln!(out)?;

    for (source_interface, target_interface, functions) in shared_interfaces {
        let interface_module = rust_ident(&snake_case(&source_interface.name), "interface module")?;
        writeln!(
            out,
            "    /// Conversions for interface `{}`.",
            source_interface.name
        )?;
        writeln!(out, "    pub mod {interface_module} {{")?;
        for (source_def, target_def) in functions {
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
        writeln!(out)?;
    }
    writeln!(out, "}}")?;
    Ok(())
}

/// The types defined by both versions of one interface, in the old version's source order.
fn shared_functions<'a>(
    source: &Version,
    target: &Version,
    old: &'a Interface,
    new: &'a Interface,
) -> Result<Vec<(&'a TypeDef, &'a TypeDef)>> {
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut functions = Vec::new();
    for old_def in &old.types {
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

struct Converted {
    expr: String,
    fallible: bool,
    /// When `expr` is a direct call `<callee>(<arg>)`, the callee path so `.map(<callee>)` avoids
    /// `clippy::redundant_closure`.
    direct_callee: Option<String>,
}

impl Converted {
    fn infallible(expr: String) -> Self {
        Self {
            expr,
            fallible: false,
            direct_callee: None,
        }
    }

    fn fallible(expr: String) -> Self {
        Self {
            expr,
            fallible: true,
            direct_callee: None,
        }
    }

    fn direct_call(callee: String, arg: &str) -> Self {
        let expr = format!("{callee}({arg})");
        Self {
            expr,
            fallible: true,
            direct_callee: Some(callee),
        }
    }

    fn into_value_expr(self) -> String {
        if self.fallible {
            format!("{}?", self.expr)
        } else {
            self.expr
        }
    }

    fn into_result_expr(self) -> String {
        if self.fallible {
            self.expr
        } else {
            format!("::core::result::Result::<_, {ERROR}>::Ok({})", self.expr)
        }
    }
}

fn copied(expression: &str) -> String {
    expression
        .strip_prefix('&')
        .map_or_else(|| format!("*{expression}"), str::to_owned)
}

fn borrowed(expression: &str) -> &str {
    expression.strip_prefix('&').unwrap_or(expression)
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
    writeln!(
        out,
        "        pub fn {function}(value: &{source_type}) -> ::core::result::Result<{target_type}, {ERROR}> {{"
    )?;
    writeln!(out, "            let _ = value;")?;
    let body = conversion_body(
        source,
        target,
        source_interface,
        target_interface,
        source_def,
        target_def,
        direction,
    )?;
    for line in body.lines() {
        writeln!(out, "            {line}")?;
    }
    writeln!(out, "        }}")?;
    writeln!(out)?;
    Ok(())
}

/// The body of one conversion function, indented at the function's own level.
#[allow(clippy::too_many_arguments)]
fn conversion_body(
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_def: &TypeDef,
    target_def: &TypeDef,
    direction: Direction,
) -> Result<String> {
    match (&source_def.kind, &target_def.kind) {
        (TypeDefKind::Record(source_fields), TypeDefKind::Record(target_fields)) => record_body(
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
            source,
            target,
            source_interface,
            target_interface,
            source_def,
            source_cases,
            target_cases,
        ),
        (TypeDefKind::Enum(source_cases), TypeDefKind::Enum(target_cases)) => enum_body(
            source,
            target,
            source_interface,
            target_interface,
            source_def,
            source_cases,
            target_cases,
        ),
        (TypeDefKind::Flags(source_flags), TypeDefKind::Flags(target_flags)) => {
            flags_body(target_interface, source_def, source_flags, target_flags)
        }
        (TypeDefKind::Alias(source_target), TypeDefKind::Alias(target_target)) => Ok(convert_expr(
            source,
            target,
            source_interface,
            target_interface,
            source_target,
            target_target,
            "value",
            &format!("{}/{}", source_interface.name, source_def.name),
        )?
        .into_result_expr()),
        (TypeDefKind::List(source_target), TypeDefKind::List(target_target)) => Ok(convert_expr(
            source,
            target,
            source_interface,
            target_interface,
            &TypeExpr::List(Box::new(source_target.clone())),
            &TypeExpr::List(Box::new(target_target.clone())),
            "value",
            &format!("{}/{}", source_interface.name, source_def.name),
        )?
        .into_result_expr()),
        (TypeDefKind::Option(source_target), TypeDefKind::Option(target_target)) => {
            Ok(convert_expr(
                source,
                target,
                source_interface,
                target_interface,
                &TypeExpr::Option(Box::new(source_target.clone())),
                &TypeExpr::Option(Box::new(target_target.clone())),
                "value",
                &format!("{}/{}", source_interface.name, source_def.name),
            )?
            .into_result_expr())
        }
        (TypeDefKind::Result(source_result), TypeDefKind::Result(target_result)) => {
            Ok(result_expr(
                source,
                target,
                source_interface,
                target_interface,
                source_result,
                target_result,
                "value",
                &format!("{}/{}", source_interface.name, source_def.name),
            )?
            .into_result_expr())
        }
        _ => contract_bail!(
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
fn record_body(
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_def: &TypeDef,
    source_fields: &[Field],
    target_fields: &[Field],
    direction: Direction,
) -> Result<String> {
    let target_interface_module =
        rust_ident(&snake_case(&target_interface.name), "interface module")?;
    let target_type = rust_ident(&pascal_case(&source_def.name), "type")?;
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
            convert_expr(
                source,
                target,
                source_interface,
                target_interface,
                &source_field.ty,
                &target_field.ty,
                &format!("&value.{source_name}"),
                &format!("{type_path}.{}", source_field.name),
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
            Direction::Forward => contract_bail!(
                "conversion {from_ver} -> {to_ver}: {type_path}.{} is required in {to_ver} but \
                 absent from {from_ver}; make it `option<T>` or `list<T>` so an older value can \
                 be defaulted, or add a new member instead of changing this one",
                missing.name,
            ),
            Direction::Reverse => {
                return Ok(format!(
                    "::core::result::Result::Err({ERROR}::MissingMember {{ from: \"{from_ver}\", to: \"{to_ver}\", path: \"{type_path}.{}\" }})",
                    missing.name
                ));
            }
        }
    }

    let mut fields = String::new();
    for target_field in target_fields {
        let target_name = rust_ident(&snake_case(&target_field.name), "record field")?;
        let value = source_fields
            .iter()
            .find(|source_field| source_field.name == target_field.name);
        if let Some(source_field) = value {
            let source_name = rust_ident(&snake_case(&source_field.name), "record field")?;
            let converted = convert_expr(
                source,
                target,
                source_interface,
                target_interface,
                &source_field.ty,
                &target_field.ty,
                &format!("&value.{source_name}"),
                &format!("{type_path}.{}", source_field.name),
            )?;
            writeln!(fields, "{target_name}: {},", converted.into_value_expr())?;
        } else {
            let Some(default) = default_expr(&target.model, &target_field.ty)? else {
                // The required-field check above gives the better diagnostic; this arm is the
                // internal-consistency guard for a model we did not expect.
                bail!(
                    "no default for target field `{}` in {type_path}",
                    target_field.name
                );
            };
            writeln!(fields, "{target_name}: {default},")?;
        }
    }
    let mut out = String::new();
    writeln!(
        out,
        "::core::result::Result::Ok(super::__witforge_to::{target_interface_module}::{target_type} {{"
    )?;
    for line in fields.lines() {
        writeln!(out, "    {line}")?;
    }
    writeln!(out, "}})")?;
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn variant_body(
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_def: &TypeDef,
    source_cases: &[Case],
    target_cases: &[Case],
) -> Result<String> {
    let source_interface_module =
        rust_ident(&snake_case(&source_interface.name), "interface module")?;
    let target_interface_module =
        rust_ident(&snake_case(&target_interface.name), "interface module")?;
    let source_type = rust_ident(&pascal_case(&source_def.name), "type")?;
    let target_type = rust_ident(&pascal_case(&source_def.name), "type")?;
    let type_path = format!("{}/{}", source_interface.name, source_def.name);
    let from_ver = source.model.package.version;
    let to_ver = target.model.package.version;
    let mut out = String::new();
    writeln!(out, "match value {{")?;
    for source_case in source_cases {
        let source_case_name = rust_ident(&pascal_case(&source_case.name), "variant case")?;
        let target_case = target_cases
            .iter()
            .find(|target_case| target_case.name == source_case.name);
        if let Some(target_case) = target_case {
            match (&source_case.payload, &target_case.payload) {
                (Some(_), None) | (None, Some(_)) => contract_bail!(
                    "conversion {from_ver} -> {to_ver}: {type_path} case `{}` changes payload \
                     shape; evolution requires a new case",
                    source_case.name
                ),
                _ => {}
            }
        }
        let pattern = if source_case.payload.is_some() {
            format!(
                "super::__witforge_from::{source_interface_module}::{source_type}::{source_case_name}(_payload)"
            )
        } else {
            format!(
                "super::__witforge_from::{source_interface_module}::{source_type}::{source_case_name}"
            )
        };
        let arm = match (target_case, &source_case.payload) {
            (None, _) => format!(
                "::core::result::Result::Err({ERROR}::UnknownCase {{ from: \"{from_ver}\", to: \"{to_ver}\", path: \"{type_path}\", case: \"{}\" }}),",
                source_case.name
            ),
            (Some(target_case), Some(source_payload)) => {
                let target_case_name = rust_ident(&pascal_case(&target_case.name), "variant case")?;
                let target_payload = target_case
                    .payload
                    .as_ref()
                    .expect("payload presence was checked above");
                let converted = convert_expr(
                    source,
                    target,
                    source_interface,
                    target_interface,
                    source_payload,
                    target_payload,
                    "payload",
                    &format!("{type_path}.{}", source_case.name),
                )?;
                format!(
                    "::core::result::Result::Ok(super::__witforge_to::{target_interface_module}::{target_type}::{target_case_name}({})),",
                    converted.into_value_expr()
                )
            }
            (Some(target_case), None) => {
                let target_case_name = rust_ident(&pascal_case(&target_case.name), "variant case")?;
                format!(
                    "::core::result::Result::Ok(super::__witforge_to::{target_interface_module}::{target_type}::{target_case_name}),"
                )
            }
        };
        // Name the binding `payload` only when the conversion arm uses it; a removed case keeps
        // the underscore so the generated match stays warning-free.
        let pattern = if target_case.is_some() {
            pattern.replace("(_payload)", "(payload)")
        } else {
            pattern
        };
        writeln!(out, "    {pattern} => {arm}")?;
    }
    writeln!(out, "}}")?;
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn enum_body(
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_def: &TypeDef,
    source_cases: &[Case],
    target_cases: &[Case],
) -> Result<String> {
    let source_interface_module =
        rust_ident(&snake_case(&source_interface.name), "interface module")?;
    let target_interface_module =
        rust_ident(&snake_case(&target_interface.name), "interface module")?;
    let source_type = rust_ident(&pascal_case(&source_def.name), "type")?;
    let target_type = rust_ident(&pascal_case(&source_def.name), "type")?;
    let type_path = format!("{}/{}", source_interface.name, source_def.name);
    let from_ver = source.model.package.version;
    let to_ver = target.model.package.version;
    let mut out = String::new();
    writeln!(out, "match value {{")?;
    for source_case in source_cases {
        let source_case_name = rust_ident(&pascal_case(&source_case.name), "enum case")?;
        let target_case_name = target_cases
            .iter()
            .find(|target_case| target_case.name == source_case.name)
            .map(|case| rust_ident(&pascal_case(&case.name), "enum case"))
            .transpose()?;
        let arm = if let Some(target_case_name) = target_case_name {
            format!(
                "::core::result::Result::Ok(super::__witforge_to::{target_interface_module}::{target_type}::{target_case_name}),"
            )
        } else {
            format!(
                "::core::result::Result::Err({ERROR}::UnknownCase {{ from: \"{from_ver}\", to: \"{to_ver}\", path: \"{type_path}\", case: \"{}\" }}),",
                source_case.name
            )
        };
        writeln!(
            out,
            "    super::__witforge_from::{source_interface_module}::{source_type}::{source_case_name} => {arm}"
        )?;
    }
    writeln!(out, "}}")?;
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn flags_body(
    target_interface: &Interface,
    source_def: &TypeDef,
    source_flags: &[crate::model::Flag],
    target_flags: &[crate::model::Flag],
) -> Result<String> {
    let target_interface_module =
        rust_ident(&snake_case(&target_interface.name), "interface module")?;
    let target_type = rust_ident(&pascal_case(&source_def.name), "type")?;
    let mut out = String::new();
    writeln!(
        out,
        "::core::result::Result::Ok(super::__witforge_to::{target_interface_module}::{target_type} {{"
    )?;
    for target_flag in target_flags {
        let target_name = rust_ident(&snake_case(&target_flag.name), "flag")?;
        let value = source_flags
            .iter()
            .find(|source_flag| source_flag.name == target_flag.name)
            .map(|source_flag| {
                rust_ident(&snake_case(&source_flag.name), "flag")
                    .map(|source_name| format!("value.{source_name}"))
            })
            .transpose()?
            .unwrap_or_else(|| "false".to_string());
        writeln!(out, "    {target_name}: {value},")?;
    }
    writeln!(out, "}})")?;
    Ok(out)
}

/// Expand aliases and named structural definitions to the expression they project.
///
/// Conversion compares the types values actually have. An alias and its target are one Rust type
/// and one wire type, so `type id = u32` and `u32` must convert identically. Named records,
/// variants, enums, and flags stay named: their conversion function is the adapter.
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

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn convert_expr(
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_type: &TypeExpr,
    target_type: &TypeExpr,
    expression: &str,
    path: &str,
) -> Result<Converted> {
    let from_ver = source.model.package.version;
    let to_ver = target.model.package.version;
    let source_type = resolve_expr(
        &source.model,
        None,
        source_type,
        "source",
        &mut HashSet::new(),
    )?;
    let target_type = resolve_expr(
        &target.model,
        None,
        target_type,
        "target",
        &mut HashSet::new(),
    )?;
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
        | (TypeExpr::F64, TypeExpr::F64) => Ok(Converted::infallible(copied(expression))),
        (TypeExpr::String, TypeExpr::String) => Ok(Converted::infallible(format!(
            "::core::clone::Clone::clone({expression})"
        ))),
        (TypeExpr::Named(source_name), TypeExpr::Named(target_name))
            if source_name == target_name =>
        {
            if source.model.type_def(source_name).is_none() {
                bail!("type `{source_name}` is not in the source model");
            }
            if target.model.type_def(target_name).is_none() {
                contract_bail!(
                    "conversion {from_ver} -> {to_ver}: {path} names `{target_name}`, which is \
                     missing from the target version; evolution requires a new type instead of \
                     removing a referenced type"
                );
            }
            if source_name.is_foreign() || target_name.is_foreign() {
                // Dependency packages are pinned and frozen with the version directory; the
                // same package-qualified name is the same value across these versions.
                return Ok(Converted::infallible(format!(
                    "::core::clone::Clone::clone({expression})"
                )));
            }
            let interface_module =
                rust_ident(&snake_case(&source_name.interface), "interface module")?;
            let function = rust_ident(&snake_case(&source_name.name), "conversion function")?;
            Ok(Converted::direct_call(
                format!("super::{interface_module}::{function}"),
                expression,
            ))
        }
        (TypeExpr::List(source_element), TypeExpr::List(target_element)) => {
            let inner = convert_expr(
                source,
                target,
                source_interface,
                target_interface,
                source_element,
                target_element,
                "item",
                path,
            )?;
            if !inner.fallible {
                return Ok(Converted::infallible(format!(
                    "::core::clone::Clone::clone({expression})"
                )));
            }
            let target_element_type =
                rust_type_expr(source, target, target_element, "__witforge_to", path)?;
            if let Some(callee) = inner.direct_callee {
                Ok(Converted::fallible(format!(
                    "{}.iter().map({callee}).collect::<::core::result::Result<::std::vec::Vec<{target_element_type}>, {ERROR}>>()",
                    borrowed(expression)
                )))
            } else {
                Ok(Converted::fallible(format!(
                    "{}.iter().map(|item| {}).collect::<::core::result::Result<::std::vec::Vec<{target_element_type}>, {ERROR}>>()",
                    borrowed(expression),
                    inner.into_result_expr()
                )))
            }
        }
        (TypeExpr::Option(source_payload), TypeExpr::Option(target_payload)) => {
            let inner = convert_expr(
                source,
                target,
                source_interface,
                target_interface,
                source_payload,
                target_payload,
                "item",
                path,
            )?;
            if !inner.fallible {
                return Ok(Converted::infallible(format!(
                    "::core::clone::Clone::clone({expression})"
                )));
            }
            if let Some(callee) = inner.direct_callee {
                Ok(Converted::fallible(format!(
                    "{}.as_ref().map({callee}).transpose()",
                    borrowed(expression)
                )))
            } else {
                Ok(Converted::fallible(format!(
                    "match {expression} {{ \
                     ::core::option::Option::Some(item) => ::core::result::Result::<_, {ERROR}>::Ok(::core::option::Option::Some({})), \
                     ::core::option::Option::None => ::core::result::Result::<_, {ERROR}>::Ok(::core::option::Option::None), \
                     }}",
                    inner.into_value_expr()
                )))
            }
        }
        (TypeExpr::Result(source_result), TypeExpr::Result(target_result)) => result_expr(
            source,
            target,
            source_interface,
            target_interface,
            source_result,
            target_result,
            expression,
            path,
        ),
        _ => contract_bail!(
            "conversion {from_ver} -> {to_ver}: {} changes type from `{}` to `{}`; evolution \
             requires a new member",
            path,
            render_type(&source_type, TypeRenderStyle::Spaced),
            render_type(&target_type, TypeRenderStyle::Spaced)
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn result_expr(
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_result: &ResultExpr,
    target_result: &ResultExpr,
    expression: &str,
    path: &str,
) -> Result<Converted> {
    let from_ver = source.model.package.version;
    let to_ver = target.model.package.version;
    let ok = match (&source_result.ok, &target_result.ok) {
        (Some(source_ok), Some(target_ok)) => convert_expr(
            source,
            target,
            source_interface,
            target_interface,
            source_ok,
            target_ok,
            "ok",
            path,
        )?,
        (None, None) => Converted::infallible("()".to_string()),
        _ => {
            contract_bail!(
                "conversion {from_ver} -> {to_ver}: {path} changes its result success shape; \
                 evolution requires a new type"
            )
        }
    };
    let err = match (&source_result.err, &target_result.err) {
        (Some(source_err), Some(target_err)) => convert_expr(
            source,
            target,
            source_interface,
            target_interface,
            source_err,
            target_err,
            "err",
            path,
        )?,
        (None, None) => Converted::infallible("()".to_string()),
        _ => contract_bail!(
            "conversion {from_ver} -> {to_ver}: {path} changes its result error shape; evolution \
             requires a new type"
        ),
    };
    if !ok.fallible && !err.fallible {
        return Ok(Converted::infallible(format!(
            "::core::clone::Clone::clone({expression})"
        )));
    }
    let ok_binding = if source_result.ok.is_some() {
        "ok"
    } else {
        "_ok"
    };
    let err_binding = if source_result.err.is_some() {
        "err"
    } else {
        "_err"
    };
    Ok(Converted::fallible(format!(
        "match {expression} {{ \
         ::core::result::Result::Ok({ok_binding}) => ::core::result::Result::<_, {ERROR}>::Ok(::core::result::Result::Ok({})), \
         ::core::result::Result::Err({err_binding}) => ::core::result::Result::<_, {ERROR}>::Ok(::core::result::Result::Err({})), \
         }}",
        ok.into_value_expr(),
        err.into_value_expr()
    )))
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

fn rust_type_expr(
    source: &Version,
    version: &Version,
    ty: &TypeExpr,
    version_alias: &str,
    path: &str,
) -> Result<String> {
    match ty {
        TypeExpr::Bool => Ok("bool".to_string()),
        TypeExpr::U8 => Ok("u8".to_string()),
        TypeExpr::U16 => Ok("u16".to_string()),
        TypeExpr::U32 => Ok("u32".to_string()),
        TypeExpr::U64 => Ok("u64".to_string()),
        TypeExpr::S8 => Ok("i8".to_string()),
        TypeExpr::S16 => Ok("i16".to_string()),
        TypeExpr::S32 => Ok("i32".to_string()),
        TypeExpr::S64 => Ok("i64".to_string()),
        TypeExpr::F32 => Ok("f32".to_string()),
        TypeExpr::F64 => Ok("f64".to_string()),
        TypeExpr::String => Ok("::std::string::String".to_string()),
        TypeExpr::Named(name) => {
            if version.model.type_def(name).is_none() {
                contract_bail!(
                    "conversion {} -> {}: {path}: type `{name}` is missing from the target \
                     version; evolution requires a new type instead of removing a referenced type",
                    source.model.package.version,
                    version.model.package.version,
                );
            }
            if let Some((namespace, package)) = name.package() {
                let module = format!("{}_{}", snake_case(namespace), snake_case(package));
                let interface = rust_ident(
                    &snake_case(name.interface_name()),
                    "dependency interface module",
                )?;
                let type_name = rust_ident(&pascal_case(&name.name), "type")?;
                return Ok(format!(
                    "super::super::super::witforge_deps::{module}::{interface}::{type_name}"
                ));
            }
            let interface_module = rust_ident(&snake_case(&name.interface), "interface module")?;
            let type_name = rust_ident(&pascal_case(&name.name), "type")?;
            Ok(format!(
                "super::{version_alias}::{interface_module}::{type_name}"
            ))
        }
        TypeExpr::List(element) => Ok(format!(
            "::std::vec::Vec<{}>",
            rust_type_expr(source, version, element, version_alias, path)?
        )),
        TypeExpr::Option(payload) => Ok(format!(
            "::core::option::Option<{}>",
            rust_type_expr(source, version, payload, version_alias, path)?
        )),
        TypeExpr::Result(result) => {
            let ok = match &result.ok {
                Some(ty) => rust_type_expr(source, version, ty, version_alias, path)?,
                None => "()".to_string(),
            };
            let err = match &result.err {
                Some(ty) => rust_type_expr(source, version, ty, version_alias, path)?,
                None => "()".to_string(),
            };
            Ok(format!("::core::result::Result<{ok}, {err}>"))
        }
    }
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
        assert!(
            message.contains("changes type from `u8` to `u64`"),
            "{message}"
        );
    }
}
