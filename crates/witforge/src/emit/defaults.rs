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
    default_expr_inner(model, None, ty, &mut HashSet::new())
}

fn default_expr_inner(
    model: &Model,
    context: Option<&Dependency>,
    ty: &TypeExpr,
    visiting: &mut HashSet<String>,
) -> Result<Option<&'static str>> {
    match ty {
        TypeExpr::Option(_) => Ok(Some("::core::option::Option::None")),
        TypeExpr::List(_) => Ok(Some("::std::vec::Vec::new()")),
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
                TypeDefKind::List(_) => Ok(Some("::std::vec::Vec::new()")),
                TypeDefKind::Alias(target) => default_expr_inner(model, resolved, target, visiting),
                _ => Ok(None),
            };
            visiting.remove(&key);
            result
        }
        _ => Ok(None),
    }
}
