//! Emitters for witforge output formats.
//!
//! Emitters consume the owned [`crate::model::Model`], never `wit-parser` types.

/// Return a [`crate::ContractError`] built from a format string: a contract the emitter cannot
/// represent, such as two WIT names that map to one generated name. Internal errors use plain
/// `bail!`, so the CLI exits with `2` for them rather than reporting a contract violation.
macro_rules! contract_bail {
    ($($arg:tt)*) => {
        return Err(anyhow::Error::new($crate::ContractError::new(format!($($arg)*))))
    };
}

mod convert;
pub(crate) mod defaults;
pub mod glue;
pub(crate) mod ident;
pub mod json_schema;
pub mod output;
pub mod proto;
pub mod rust;
pub(crate) mod versions;

use crate::model::{TypeDefKind, TypeExpr};

pub(crate) use ident::{
    RUST_KEYWORDS, plain_ident as rust_ident, plain_pascal as pascal_case,
    plain_snake as snake_case,
};

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
        TypeDefKind::Option(_) => "option",
        TypeDefKind::Result(_) => "result",
    }
}

/// Render a type definition's shape in the requested diagnostic style.
pub(crate) fn type_shape(kind: &TypeDefKind, style: TypeRenderStyle) -> String {
    match kind {
        TypeDefKind::Alias(target) => render_type(target, style),
        TypeDefKind::List(element) => format!("list<{}>", render_type(element, style)),
        TypeDefKind::Option(payload) => format!("option<{}>", render_type(payload, style)),
        TypeDefKind::Result(result) => {
            let separator = match style {
                TypeRenderStyle::Compact => ",",
                TypeRenderStyle::Spaced => ", ",
            };
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
        TypeDefKind::Record(_)
        | TypeDefKind::Variant(_)
        | TypeDefKind::Enum(_)
        | TypeDefKind::Flags(_) => type_kind_name(kind).to_string(),
    }
}

/// Render a type expression in the requested diagnostic style.
pub(crate) fn render_type(ty: &TypeExpr, style: TypeRenderStyle) -> String {
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
        TypeExpr::List(element) => format!("list<{}>", render_type(element, style)),
        TypeExpr::Option(payload) => format!("option<{}>", render_type(payload, style)),
        TypeExpr::Result(result) => {
            let separator = match style {
                TypeRenderStyle::Compact => ",",
                TypeRenderStyle::Spaced => ", ",
            };
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
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::versions::{Version, load_versions};
    use super::{glue, proto, rust};
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
            &glue::plan_versions(&dangling, "crate::plain", "crate::proto").unwrap_err(),
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
    }
}
