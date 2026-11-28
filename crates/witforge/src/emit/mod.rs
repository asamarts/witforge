//! Emitters for witforge output formats.
//!
//! Emitters consume the owned [`crate::model::Model`], never `wit-parser` types.

/// Return a [`crate::ContractError`] built from a format string: a contract the emitter cannot
/// represent, such as two WIT names that map to one generated name. Internal errors use plain
/// `bail!`, so the CLI exits with `2` for them rather than reporting a contract violation.
macro_rules! contract_bail {
    ($($arg:tt)*) => {{
        let message = format!($($arg)*);
        let code = if message.contains("-byte limit") {
            $crate::diagnostic::codes::SUBSET_TYPE_LIMIT
        } else {
            $crate::diagnostic::codes::IDENTIFIER_COLLISION
        };
        return Err(anyhow::Error::new(
            $crate::ContractError::with_diagnostic(
                $crate::diagnostic::Diagnostic::error(code, message),
            ),
        ));
    }};
}

pub(crate) mod convert;
pub(crate) mod defaults;
pub mod doc;
pub(crate) mod docs;
pub mod glue;
pub mod go;
pub mod go_glue;
pub(crate) mod ident;
pub mod json_schema;
pub mod output;
pub mod polyglot;
pub mod proto;
pub mod python;
pub mod python_glue;
pub mod rust;
pub(crate) mod rustfmt;
pub mod typescript;
pub mod typescript_glue;
pub(crate) mod versions;

use crate::model::{Model, Stability, TypeDefKind, TypeExpr};

pub(crate) use ident::{
    RUST_KEYWORDS, plain_ident as rust_ident, plain_pascal as pascal_case,
    plain_snake as snake_case,
};

pub(crate) fn stability_doc(stability: &Stability) -> Option<String> {
    let state = if let Some(since) = stability.since() {
        Some(format!("Stable since WIT version {since}."))
    } else {
        stability
            .unstable_feature()
            .map(|feature| format!("Unstable WIT feature `{feature}`."))
    };
    match (state, stability.deprecation_note()) {
        (Some(state), Some(note)) => Some(format!("{state} {note}")),
        (Some(state), None) => Some(state),
        (None, Some(note)) => Some(note.to_string()),
        (None, None) => None,
    }
}

pub(crate) fn rust_deprecated_attribute(stability: &Stability) -> Option<String> {
    Some(format!(
        "#[deprecated(since = \"{}\", note = {:?})]",
        stability.deprecated()?,
        stability.deprecation_note()?
    ))
}
pub(crate) fn model_has_deprecation(model: &Model) -> bool {
    model.interfaces.iter().any(interface_has_deprecation)
        || model.dependencies.iter().any(dependency_has_deprecation)
}

pub(crate) fn dependency_has_deprecation(dependency: &crate::model::Dependency) -> bool {
    dependency.interfaces.iter().any(interface_has_deprecation)
}

fn interface_has_deprecation(interface: &crate::model::Interface) -> bool {
    interface.stability.deprecated().is_some()
        || interface
            .types
            .iter()
            .any(|def| def.stability.deprecated().is_some())
        || interface
            .functions
            .iter()
            .any(|function| function.stability.deprecated().is_some())
}

/// How a type expression is rendered in diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeRenderStyle {
    /// No space after result commas, for stable diff shapes.
    Compact,
    /// A space after result commas, for conversion diagnostics.
    Spaced,
}

/// The WIT spelling of a type-definition kind.
pub(crate) fn type_kind_name(kind: &TypeDefKind) -> &'static str {
    match kind {
        TypeDefKind::Record(_) => "record",
        TypeDefKind::Variant(_) => "variant",
        TypeDefKind::Enum(_) => "enum",
        TypeDefKind::Flags(_) => "flags",
        TypeDefKind::Alias(_) => "alias",
        TypeDefKind::List(_) => "list",
        TypeDefKind::FixedLengthList(..) => "fixed-length list",
        TypeDefKind::Option(_) => "option",
        TypeDefKind::Result(_) => "result",
        TypeDefKind::Map(..) => "map",
    }
}

/// Render a type definition's shape in the requested diagnostic style.
pub(crate) fn type_shape(kind: &TypeDefKind, style: TypeRenderStyle) -> String {
    let separator = match style {
        TypeRenderStyle::Compact => ",",
        TypeRenderStyle::Spaced => ", ",
    };
    match kind {
        TypeDefKind::Alias(target) => render_type(target, style),
        TypeDefKind::List(element) => format!("list<{}>", render_type(element, style)),
        TypeDefKind::FixedLengthList(element, len) => {
            format!("list<{}{separator}{len}>", render_type(element, style))
        }
        TypeDefKind::Option(payload) => format!("option<{}>", render_type(payload, style)),
        TypeDefKind::Result(result) => {
            format!(
                "result<{}{separator}{}>",
                result
                    .ok
                    .as_deref()
                    .map_or_else(|| "_".to_string(), |ty| render_type(ty, style)),
                result
                    .err
                    .as_deref()
                    .map_or_else(|| "_".to_string(), |ty| render_type(ty, style)),
            )
        }
        TypeDefKind::Map(key, value) => format!(
            "map<{}{separator}{}>",
            render_type(key, style),
            render_type(value, style)
        ),
        TypeDefKind::Record(_)
        | TypeDefKind::Variant(_)
        | TypeDefKind::Enum(_)
        | TypeDefKind::Flags(_) => type_kind_name(kind).to_string(),
    }
}

/// Render a type expression in the requested diagnostic style.
pub(crate) fn render_type(ty: &TypeExpr, style: TypeRenderStyle) -> String {
    let separator = match style {
        TypeRenderStyle::Compact => ",",
        TypeRenderStyle::Spaced => ", ",
    };
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
        TypeExpr::Char => "char".to_string(),
        TypeExpr::String => "string".to_string(),
        TypeExpr::Named(name) => name.to_string(),
        TypeExpr::List(element) => format!("list<{}>", render_type(element, style)),
        TypeExpr::FixedLengthList(element, len) => {
            format!("list<{}{separator}{len}>", render_type(element, style))
        }
        TypeExpr::Option(payload) => format!("option<{}>", render_type(payload, style)),
        TypeExpr::Result(result) => {
            format!(
                "result<{}{separator}{}>",
                result
                    .ok
                    .as_deref()
                    .map_or_else(|| "_".to_string(), |ty| render_type(ty, style)),
                result
                    .err
                    .as_deref()
                    .map_or_else(|| "_".to_string(), |ty| render_type(ty, style)),
            )
        }
        TypeExpr::Map(key, value) => format!(
            "map<{}{separator}{}>",
            render_type(key, style),
            render_type(value, style)
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::versions::{Version, load_versions};
    use super::{glue, go, go_glue, proto, python, python_glue, rust, typescript, typescript_glue};
    use crate::is_contract_error;
    use crate::lock::WireLock;
    use crate::model::{QualifiedName, TypeDefKind, TypeExpr};

    fn versions() -> Vec<Version> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/versions/wit");
        load_versions(&root).unwrap()
    }

    fn assert_internal(error: &anyhow::Error, expected: &str) {
        let message = format!("{error:#}");
        assert!(message.contains(expected), "{message}");
        assert!(!is_contract_error(error), "{message}");
    }

    /// Validated input never makes an emitter fail internally, so the versions are edited after
    /// validation; each plan must pass the emitter's plain error on rather than call it a
    /// contract violation, which the CLI would report with exit `1` instead of `2`.
    #[allow(clippy::too_many_lines)]
    #[test]
    fn plans_pass_on_an_internal_emitter_error_as_internal() {
        // A record field naming a type the model does not have.
        let mut dangling = versions();
        for version in &mut dangling {
            for interface in &mut version.model.interfaces {
                for def in &mut interface.types {
                    if let TypeDefKind::Record(fields) = &mut def.kind {
                        fields[0].ty = TypeExpr::Named(QualifiedName {
                            interface: interface.name.clone(),
                            name: "missing".to_string(),
                        });
                    }
                }
            }
        }
        assert_internal(
            &rust::plan_versions_with_serde(&dangling, false).unwrap_err(),
            "is not in the model",
        );
        assert_internal(
            &typescript::plan_versions_with_options(
                &dangling,
                &typescript::TypeScriptOptions::new(),
            )
            .unwrap_err(),
            "is not in the model",
        );
        assert_internal(
            &python::plan_versions_with_options(&dangling, &python::PythonOptions::new())
                .unwrap_err(),
            "is not in the model",
        );
        assert_internal(
            &go::plan_versions_with_options(
                &dangling,
                "example.com/witforge/test",
                &go::GoOptions::new(),
            )
            .unwrap_err(),
            "is not in the model",
        );
        assert_internal(
            &glue::plan_versions(&dangling, "crate::plain", "crate::proto").unwrap_err(),
            "is not in the model",
        );
        assert_internal(
            &typescript_glue::plan_versions_with_options(
                &dangling,
                "../plain",
                "../proto",
                &typescript_glue::TypeScriptGlueOptions::new(),
            )
            .unwrap_err(),
            "is not in the model",
        );
        assert_internal(
            &go_glue::plan_versions_with_options(
                &dangling,
                "example.com/witforge/glue",
                "example.com/witforge/plain",
                "example.com/witforge/proto",
                &go_glue::GoGlueOptions::new(),
            )
            .unwrap_err(),
            "is not in the model",
        );
        assert_internal(
            &python_glue::plan_versions_with_options(
                &dangling,
                "plain",
                "proto",
                &python_glue::PythonGlueOptions::new(),
            )
            .unwrap_err(),
            "is not in the model",
        );

        // A lockfile without the model's members.
        let mut unlocked = versions();
        for version in &mut unlocked {
            version.lock = WireLock::default();
        }
        assert_internal(
            &proto::plan_versions(&unlocked).unwrap_err(),
            "lockfile has no",
        );

        // A live member whose entry is reserved: glue converts it, and the protobuf schema glue
        // checks the contract against refuses it.
        let mut reserved = versions();
        for version in &mut reserved {
            let text = version.lock.render().replace(
                "user.name = 1 field string\n",
                "user.name = 1 field string reserved\n",
            );
            assert_ne!(text, version.lock.render());
            version.lock = WireLock::parse(&text).unwrap();
        }
        assert_internal(
            &glue::plan_versions(&reserved, "crate::plain", "crate::proto").unwrap_err(),
            "is live in the model",
        );
        assert_internal(
            &typescript_glue::plan_versions_with_options(
                &reserved,
                "../plain",
                "../proto",
                &typescript_glue::TypeScriptGlueOptions::new(),
            )
            .unwrap_err(),
            "is live in the model",
        );
        assert_internal(
            &go_glue::plan_versions_with_options(
                &reserved,
                "example.com/witforge/glue",
                "example.com/witforge/plain",
                "example.com/witforge/proto",
                &go_glue::GoGlueOptions::new(),
            )
            .unwrap_err(),
            "is live in the model",
        );
        assert_internal(
            &python_glue::plan_versions_with_options(
                &reserved,
                "plain",
                "proto",
                &python_glue::PythonGlueOptions::new(),
            )
            .unwrap_err(),
            "is live in the model",
        );
    }
}
