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

use crate::emit::rust::{HEADER, pascal_case, rust_ident, snake_case, version_module_name};
use crate::emit::versions::Version;
use crate::model::{
    Case, Field, Interface, Model, QualifiedName, ResultExpr, TypeDef, TypeDefKind, TypeExpr,
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
    writeln!(out, "{HEADER}")?;
    writeln!(out, "// source: {}:{}", package.namespace, package.name)?;
    writeln!(out)?;
    write_error(&mut out)?;
    for (old, new) in pairs {
        emit_pair(&mut out, old, new, Direction::Forward)?;
        emit_pair(&mut out, new, old, Direction::Reverse)?;
    }
    Ok(Some(out))
}

fn write_error(out: &mut String) -> Result<()> {
    writeln!(
        out,
        "/// A version-to-version conversion that cannot reconstruct the target value."
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
        "            Self::MissingMember {{ path }} => write!(formatter, \"cannot reconstruct `{{path}}` from the source version\"),"
    )?;
    writeln!(
        out,
        "            Self::UnknownCase {{ path, case }} => write!(formatter, \"`{{path}}` case `{{case}}` does not exist in the target version\"),"
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
    writeln!(
        out,
        "/// Conversions from `{source_module}` to `{target_module}`."
    )?;
    writeln!(out, "pub mod {source_module}_to_{target_module} {{")?;
    // The aliases cannot collide with a WIT interface module: WIT identifiers start with a
    // letter, while these start with `_`.
    writeln!(out, "    #[allow(unused_imports)]")?;
    writeln!(
        out,
        "    use super::super::{{ {source_module} as __witforge_from, {target_module} as __witforge_to }};"
    )?;
    writeln!(out)?;

    for source_interface in &source.model.interfaces {
        let Some(target_interface) = target.model.interface(&source_interface.name) else {
            continue;
        };
        let functions = shared_functions(source_interface, target_interface)?;
        if functions.is_empty() {
            continue;
        }
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
    writeln!(out)?;
    Ok(())
}

/// The types defined by both versions of one interface, in the old version's source order.
fn shared_functions<'a>(
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
                "interface `{}`: conversion functions `{previous}` and `{}` both map to Rust \
                 function `{name}`",
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
            source_interface,
            target_interface,
            source_def,
            source_cases,
            target_cases,
        ),
        (TypeDefKind::Flags(source_flags), TypeDefKind::Flags(target_flags)) => {
            flags_body(target_interface, source_def, source_flags, target_flags)
        }
        (TypeDefKind::Alias(source_target), TypeDefKind::Alias(target_target)) => convert_expr(
            source,
            target,
            source_interface,
            target_interface,
            source_target,
            target_target,
            "value",
            &format!("{}/{}", source_interface.name, source_def.name),
        ),
        (TypeDefKind::List(source_target), TypeDefKind::List(target_target)) => convert_expr(
            source,
            target,
            source_interface,
            target_interface,
            &TypeExpr::List(Box::new(source_target.clone())),
            &TypeExpr::List(Box::new(target_target.clone())),
            "value",
            &format!("{}/{}", source_interface.name, source_def.name),
        ),
        (TypeDefKind::Option(source_target), TypeDefKind::Option(target_target)) => convert_expr(
            source,
            target,
            source_interface,
            target_interface,
            &TypeExpr::Option(Box::new(source_target.clone())),
            &TypeExpr::Option(Box::new(target_target.clone())),
            "value",
            &format!("{}/{}", source_interface.name, source_def.name),
        ),
        (TypeDefKind::Result(source_result), TypeDefKind::Result(target_result)) => result_expr(
            source,
            target,
            source_interface,
            target_interface,
            source_result,
            target_result,
            "value",
            &format!("{}/{}", source_interface.name, source_def.name),
        ),
        _ => contract_bail!(
            "{}/{} changes kind from {} to {}; evolution requires a new type",
            source_interface.name,
            source_def.name,
            kind_name(&source_def.kind),
            kind_name(&target_def.kind)
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
                "{type_path}.{} is required in {} but absent from {}; make it `option<T>` or \
                 `list<T>` so an older value can be defaulted, or add a new member instead of \
                 changing this one",
                missing.name,
                target.model.package.version,
                source.model.package.version
            ),
            Direction::Reverse => {
                return Ok(format!(
                    "::core::result::Result::Err({ERROR}::MissingMember {{ path: \"{type_path}.{}\" }})",
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
            let expression = convert_expr(
                source,
                target,
                source_interface,
                target_interface,
                &source_field.ty,
                &target_field.ty,
                &format!("&value.{source_name}"),
                &format!("{type_path}.{}", source_field.name),
            )?;
            writeln!(fields, "{target_name}: {expression}?,")?;
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
                    "{type_path} case `{}` changes payload shape; evolution requires a new case",
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
                "::core::result::Result::Err({ERROR}::UnknownCase {{ path: \"{type_path}\", case: \"{}\" }}),",
                source_case.name
            ),
            (Some(target_case), Some(source_payload)) => {
                let target_case_name = rust_ident(&pascal_case(&target_case.name), "variant case")?;
                let target_payload = target_case
                    .payload
                    .as_ref()
                    .expect("payload presence was checked above");
                let expression = convert_expr(
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
                    "::core::result::Result::Ok(super::__witforge_to::{target_interface_module}::{target_type}::{target_case_name}({expression}?)),"
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
                "::core::result::Result::Err({ERROR}::UnknownCase {{ path: \"{type_path}\", case: \"{}\" }}),",
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

#[allow(clippy::too_many_arguments)]
fn convert_expr(
    source: &Version,
    target: &Version,
    source_interface: &Interface,
    target_interface: &Interface,
    source_type: &TypeExpr,
    target_type: &TypeExpr,
    expression: &str,
    path: &str,
) -> Result<String> {
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
        | (TypeExpr::F64, TypeExpr::F64) => {
            Ok(format!("::core::result::Result::Ok(*({expression}))"))
        }
        (TypeExpr::String, TypeExpr::String) => Ok(format!(
            "::core::result::Result::Ok(({expression}).clone())"
        )),
        (TypeExpr::Named(source_name), TypeExpr::Named(target_name))
            if source_name == target_name =>
        {
            if source.model.type_def(source_name).is_none() {
                bail!("type `{source_name}` is not in the source model");
            }
            if target.model.type_def(target_name).is_none() {
                contract_bail!(
                    "{path} names `{target_name}`, which is missing from the target version; \
                     evolution requires a new type instead of removing a referenced type"
                );
            }
            let interface_module =
                rust_ident(&snake_case(&source_name.interface), "interface module")?;
            let function = rust_ident(&snake_case(&source_name.name), "conversion function")?;
            Ok(format!(
                "super::{interface_module}::{function}({expression})"
            ))
        }
        (TypeExpr::List(source_element), TypeExpr::List(target_element)) => {
            let target_element_type = rust_type_expr(target, target_element, "__witforge_to")?;
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
            Ok(format!(
                "({expression}).iter().map(|item| {inner}).collect::<::core::result::Result<::std::vec::Vec<{target_element_type}>, {ERROR}>>()"
            ))
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
            Ok(format!(
                "match {expression} {{ \
                 ::core::option::Option::Some(item) => ::core::result::Result::Ok(::core::option::Option::Some({inner}?)), \
                 ::core::option::Option::None => ::core::result::Result::Ok(::core::option::Option::None), \
                 }}"
            ))
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
            "{} changes type from `{}` to `{}`; evolution requires a new member",
            path,
            render_type(source_type),
            render_type(target_type)
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
) -> Result<String> {
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
        (None, None) => "::core::result::Result::Ok(())".to_string(),
        _ => {
            contract_bail!("{path} changes its result success shape; evolution requires a new type")
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
        (None, None) => "::core::result::Result::Ok(())".to_string(),
        _ => contract_bail!("{path} changes its result error shape; evolution requires a new type"),
    };
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
    Ok(format!(
        "match {expression} {{ \
         ::core::result::Result::Ok({ok_binding}) => ::core::result::Result::Ok(::core::result::Result::Ok({ok}?)), \
         ::core::result::Result::Err({err_binding}) => ::core::result::Result::Ok(::core::result::Result::Err({err}?)), \
         }}"
    ))
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

fn rust_type_expr(version: &Version, ty: &TypeExpr, version_alias: &str) -> Result<String> {
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
                    "type `{name}` is missing from the target version; evolution requires a new \
                     type instead of removing a referenced type"
                );
            }
            let interface_module = rust_ident(&snake_case(&name.interface), "interface module")?;
            let type_name = rust_ident(&pascal_case(&name.name), "type")?;
            Ok(format!(
                "super::{version_alias}::{interface_module}::{type_name}"
            ))
        }
        TypeExpr::List(element) => Ok(format!(
            "::std::vec::Vec<{}>",
            rust_type_expr(version, element, version_alias)?
        )),
        TypeExpr::Option(payload) => Ok(format!(
            "::core::option::Option<{}>",
            rust_type_expr(version, payload, version_alias)?
        )),
        TypeExpr::Result(result) => {
            let ok = match &result.ok {
                Some(ty) => rust_type_expr(version, ty, version_alias)?,
                None => "()".to_string(),
            };
            let err = match &result.err {
                Some(ty) => rust_type_expr(version, ty, version_alias)?,
                None => "()".to_string(),
            };
            Ok(format!("::core::result::Result<{ok}, {err}>"))
        }
    }
}

/// The default value for a defaultable target field, or `None` if the type has no default.
fn default_expr(model: &Model, ty: &TypeExpr) -> Result<Option<&'static str>> {
    default_expr_inner(model, ty, &mut HashSet::new())
}

fn default_expr_inner(
    model: &Model,
    ty: &TypeExpr,
    visiting: &mut HashSet<QualifiedName>,
) -> Result<Option<&'static str>> {
    match ty {
        TypeExpr::Option(_) => Ok(Some("::core::option::Option::None")),
        TypeExpr::List(_) => Ok(Some("::std::vec::Vec::new()")),
        TypeExpr::Named(name) => {
            let Some(def) = model.type_def(name) else {
                bail!("type `{name}` is not in the model");
            };
            if !visiting.insert(name.clone()) {
                return Ok(None);
            }
            let result = match &def.kind {
                TypeDefKind::Option(_) | TypeDefKind::Alias(TypeExpr::Option(_)) => {
                    Ok(Some("::core::option::Option::None"))
                }
                TypeDefKind::List(_) | TypeDefKind::Alias(TypeExpr::List(_)) => {
                    Ok(Some("::std::vec::Vec::new()"))
                }
                TypeDefKind::Alias(target) => default_expr_inner(model, target, visiting),
                _ => Ok(None),
            };
            visiting.remove(name);
            result
        }
        _ => Ok(None),
    }
}

fn kind_name(kind: &TypeDefKind) -> &'static str {
    match kind {
        TypeDefKind::Record(_) => "record",
        TypeDefKind::Variant(_) => "variant",
        TypeDefKind::Enum(_) => "enum",
        TypeDefKind::Flags(_) => "flags",
        TypeDefKind::Alias(_) => "alias",
        TypeDefKind::List(_) => "list",
        TypeDefKind::Option(_) => "option",
        TypeDefKind::Result(_) => "result",
    }
}

fn render_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Bool => "bool".to_string(),
        TypeExpr::U8 => "u8".to_string(),
        TypeExpr::U16 => "u16".to_string(),
        TypeExpr::U32 => "u32".to_string(),
        TypeExpr::U64 => "u64".to_string(),
        TypeExpr::S8 => "s8".to_string(),
        TypeExpr::S16 => "s16".to_string(),
        TypeExpr::S32 => "s32".to_string(),
        TypeExpr::S64 => "s64".to_string(),
        TypeExpr::F32 => "f32".to_string(),
        TypeExpr::F64 => "f64".to_string(),
        TypeExpr::String => "string".to_string(),
        TypeExpr::Named(name) => name.to_string(),
        TypeExpr::List(element) => format!("list<{}>", render_type(element)),
        TypeExpr::Option(payload) => format!("option<{}>", render_type(payload)),
        TypeExpr::Result(result) => {
            let ok = result
                .ok
                .as_ref()
                .map_or_else(|| "_".to_string(), |ty| render_type(ty));
            let err = result
                .err
                .as_ref()
                .map_or_else(|| "_".to_string(), |ty| render_type(ty));
            format!("result<{ok}, {err}>")
        }
    }
}
