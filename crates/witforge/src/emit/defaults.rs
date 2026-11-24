//! Defaultability rules shared by conversion generation and compatibility reporting.

use std::collections::HashSet;

use anyhow::{Result, bail};

use crate::model::{Dependency, Model, TypeDefKind, TypeExpr};

/// The default value for a defaultable target field, or `None` if the type has no default.
///
/// `option<T>` defaults to `None`, `list<T>` to an empty vector, and a named alias or named
/// structural type resolves to the same rule. Every other type has no default: a required
/// member of such a type cannot be reconstructed by a newer-to-older conversion.
pub(crate) fn default_expr(model: &Model, ty: &TypeExpr) -> Result<Option<&'static str>> {
    default_expr_with_no_std(model, ty, false)
}

/// Like [`default_expr`], selecting `::alloc::vec::Vec::new()` when `no_std` is true.
pub(crate) fn default_expr_with_no_std(
    model: &Model,
    ty: &TypeExpr,
    no_std: bool,
) -> Result<Option<&'static str>> {
    default_expr_inner(model, None, ty, no_std, &mut HashSet::new())
}

fn default_expr_inner(
    model: &Model,
    context: Option<&Dependency>,
    ty: &TypeExpr,
    no_std: bool,
    visiting: &mut HashSet<String>,
) -> Result<Option<&'static str>> {
    let empty_vec = if no_std {
        "::alloc::vec::Vec::new()"
    } else {
        "::std::vec::Vec::new()"
    };
    match ty {
        TypeExpr::Option(_) => Ok(Some("::core::option::Option::None")),
        TypeExpr::List(_) => Ok(Some(empty_vec)),
        TypeExpr::Named(name) => {
            let Some((resolved, def)) = model.type_def_in(context, name) else {
                bail!("type `{name}` is not in the model");
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
            if !visiting.insert(key.clone()) {
                return Ok(None);
            }
            let result = match &def.kind {
                TypeDefKind::Option(_) => Ok(Some("::core::option::Option::None")),
                TypeDefKind::List(_) => Ok(Some(empty_vec)),
                TypeDefKind::Alias(target) => {
                    default_expr_inner(model, resolved, target, no_std, visiting)
                }
                _ => Ok(None),
            };
            visiting.remove(&key);
            result
        }
        _ => Ok(None),
    }
}
