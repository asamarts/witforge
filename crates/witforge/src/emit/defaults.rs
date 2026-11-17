//! Defaultability rules shared by conversion generation and compatibility reporting.

use std::collections::HashSet;

use anyhow::{Result, bail};

use crate::model::{Model, QualifiedName, TypeDefKind, TypeExpr};

/// The default value for a defaultable target field, or `None` if the type has no default.
///
/// `option<T>` defaults to `None`, `list<T>` to an empty vector, and a named alias or named
/// structural type resolves to the same rule. Every other type has no default: a required
/// member of such a type cannot be reconstructed by a newer-to-older conversion.
pub(crate) fn default_expr(model: &Model, ty: &TypeExpr) -> Result<Option<&'static str>> {
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
