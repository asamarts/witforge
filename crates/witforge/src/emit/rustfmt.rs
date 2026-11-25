//! Deterministic Rust formatting helpers matching `rustfmt` default style in both
//! Edition 2021 and Edition 2024.
//!
//! witforge does not shell out to a `rustfmt` binary (`docs/design/emitters.md` section 1). These
//! helpers produce Rust source that passes `cargo fmt --check` under both style editions without
//! re-sorting version modules or trait re-exports.

use std::fmt::Write as _;

use anyhow::Result;

use crate::emit::docs;

/// Default `rustfmt` `max_width`.
pub(crate) const MAX_WIDTH: usize = 100;
/// Default `rustfmt` `struct_lit_width`.
const STRUCT_LIT_WIDTH: usize = 18;
/// Default `rustfmt` `fn_call_width`.
pub(crate) const FN_CALL_WIDTH: usize = 60;
/// Default `rustfmt` `array_width`.
const ARRAY_WIDTH: usize = 60;

/// Conservative check for whether a rendered Rust type expression may trigger
/// `clippy::type_complexity` (default threshold 250).
pub(crate) fn needs_allow_type_complexity(ty: &str) -> bool {
    let mut depth = usize::from(ty.trim_start().starts_with('&'));
    let mut max_depth = depth;
    let mut generic_count = 0usize;
    for ch in ty.chars() {
        match ch {
            '<' | '(' => {
                depth += 1;
                max_depth = max_depth.max(depth);
                if ch == '<' {
                    generic_count += 1;
                }
            }
            '>' | ')' => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    max_depth >= 4 || generic_count >= 4
}

/// Write outer Rust doc comments for `docs_text`, falling back to `fallback` so generated public
/// items always pass `-D missing_docs`.
pub(crate) fn write_docs_or(
    out: &mut String,
    indent: usize,
    docs_text: &str,
    fallback: &str,
) -> Result<()> {
    if !docs::write_rust_outer(out, indent, docs_text)? {
        let prefix = " ".repeat(indent);
        writeln!(out, "{prefix}/// {fallback}")?;
    }
    Ok(())
}

/// Format a Rust type expression so it stays within `MAX_WIDTH` in `rustfmt`'s default style.
///
/// `indent` is the base indentation of the enclosing line; `first_line_used` is the number of
/// columns already occupied before this type on its first line; `suffix_len` is the number of
/// characters immediately following the type on its last line (e.g. `1` for `,` or `;`, `2` for
/// ` {`).
pub(crate) fn fmt_type(
    ty: &str,
    indent: usize,
    first_line_used: usize,
    suffix_len: usize,
) -> String {
    if first_line_used + ty.len() + suffix_len <= MAX_WIDTH {
        return ty.to_string();
    }
    if let Some(rest) = ty.strip_prefix('&') {
        return format!(
            "&{}",
            fmt_type(rest, indent, first_line_used + 1, suffix_len)
        );
    }
    // Tuple type `(A, B, ...)`
    if ty.starts_with('(')
        && ty.ends_with(')')
        && let Some(elems) = split_top_level(&ty[1..ty.len() - 1], ',')
        && elems.len() >= 2
    {
        let inner_indent = indent + 4;
        let inner_prefix = " ".repeat(inner_indent);
        let close_prefix = " ".repeat(indent);
        let mut out = String::from("(\n");
        for elem in elems {
            let formatted = fmt_type(elem.trim(), inner_indent, inner_indent, 1);
            let _ = writeln!(out, "{inner_prefix}{formatted},");
        }
        let _ = write!(out, "{close_prefix})");
        return out;
    }
    // Generic path `Head<Arg1, Arg2>`
    if let Some(open) = find_top_level_char(ty, '<')
        && ty.ends_with('>')
    {
        let head = &ty[..open];
        let inner = &ty[open + 1..ty.len() - 1];
        if let Some(args) = split_top_level(inner, ',') {
            let inner_indent = indent + 4;
            let inner_prefix = " ".repeat(inner_indent);
            let close_prefix = " ".repeat(indent);
            let mut out = format!("{head}<\n");
            for arg in args {
                let formatted = fmt_type(arg.trim(), inner_indent, inner_indent, 1);
                let _ = writeln!(out, "{inner_prefix}{formatted},");
            }
            let _ = write!(out, "{close_prefix}>");
            return out;
        }
    }
    ty.to_string()
}

/// Format a `pub type Name = Ty;` or `type Name = Ty;` declaration.
pub(crate) fn fmt_type_alias(indent: usize, prefix_decl: &str, ty: &str) -> String {
    let spaces = " ".repeat(indent);
    let head = format!("{spaces}{prefix_decl} = ");
    if head.len() + ty.len() < MAX_WIDTH {
        return format!("{head}{ty};");
    }
    if !ty.starts_with('(') && indent + 4 + ty.len() < MAX_WIDTH {
        return format!("{spaces}{prefix_decl} =\n{spaces}    {ty};");
    }
    let formatted = fmt_type(ty, indent, head.len(), 1);
    format!("{head}{formatted};")
}

/// Format a function or trait-method signature.
///
/// `header` is e.g. `"pub fn to_proto"` or `"fn analyze"`. `params` are already formatted as
/// `"&self"` or `"name: Ty"`. `trailer` is `";"` or `" {"`.
pub(crate) fn fmt_fn_sig(
    indent: usize,
    header: &str,
    params: &[String],
    ret: Option<&str>,
    trailer: &str,
) -> String {
    let prefix = " ".repeat(indent);
    let joined = params.join(", ");
    let single = match ret {
        Some(ret) => format!("{prefix}{header}({joined}) -> {ret}{trailer}"),
        None => format!("{prefix}{header}({joined}){trailer}"),
    };
    if single.len() <= MAX_WIDTH {
        return single;
    }
    if params.is_empty() {
        // Keep 0-param signatures single-line with generic return wrapping if needed (callers use
        // local aliases so 0-param signatures stay under `MAX_WIDTH` in both 2021 and 2024).
        if let Some(ret) = ret {
            let head = format!("{prefix}{header}() -> ");
            let formatted_ret = fmt_type(ret, indent, head.len(), trailer.len());
            return format!("{head}{formatted_ret}{trailer}");
        }
        return single;
    }
    let param_indent = indent + 4;
    let param_prefix = " ".repeat(param_indent);
    let mut out = format!("{prefix}{header}(\n");
    for param in params {
        if let Some((name, ty)) = param.split_once(": ") {
            let head = format!("{param_prefix}{name}: ");
            let formatted = fmt_type(ty, param_indent, head.len(), 1);
            let _ = writeln!(out, "{head}{formatted},");
        } else {
            let _ = writeln!(out, "{param_prefix}{param},");
        }
    }
    match ret {
        Some(ret) => {
            let ret_head = format!("{prefix}) -> ");
            if trailer == " {" {
                let single_len = ret_head.len() + ret.len() + 2;
                if single_len <= MAX_WIDTH.saturating_sub(indent) {
                    let _ = write!(out, "{ret_head}{ret} {{");
                } else if ret_head.len() + ret.len() <= MAX_WIDTH + 2 {
                    let _ = write!(out, "{ret_head}{ret}\n{prefix}{{");
                } else {
                    let formatted_ret = fmt_type(ret, indent, ret_head.len(), trailer.len());
                    let _ = write!(out, "{ret_head}{formatted_ret}{trailer}");
                }
            } else {
                let formatted_ret = fmt_type(ret, indent, ret_head.len(), trailer.len());
                let _ = write!(out, "{ret_head}{formatted_ret}{trailer}");
            }
        }
        None => {
            let _ = write!(out, "{prefix}){trailer}");
        }
    }
    out
}

/// Format a struct field (`pub name: Ty,`) at `indent`.
pub(crate) fn fmt_struct_field(indent: usize, field_name: &str, ty: &str) -> String {
    let prefix = " ".repeat(indent);
    let head = format!("{prefix}pub {field_name}: ");
    if head.len() + ty.len() < MAX_WIDTH {
        return format!("{head}{ty},");
    }
    if indent + 4 + ty.len() < MAX_WIDTH {
        return format!("{prefix}pub {field_name}:\n{prefix}    {ty},");
    }
    let formatted = fmt_type(ty, indent, head.len(), 1);
    format!("{head}{formatted},")
}

/// Format a tuple enum variant (`Case(Ty),`) at `indent`.
pub(crate) fn fmt_tuple_variant(indent: usize, case_name: &str, ty: &str) -> String {
    let prefix = " ".repeat(indent);
    let single = format!("{prefix}{case_name}({ty}),");
    if single.len() <= MAX_WIDTH {
        return single;
    }
    let inner_indent = indent + 4;
    let inner_prefix = " ".repeat(inner_indent);
    let formatted = fmt_type(ty, inner_indent, inner_indent, 1);
    format!("{prefix}{case_name}(\n{inner_prefix}{formatted},\n{prefix}),")
}

/// Format a struct literal `Path { k: v, ... }` matching `rustfmt`'s `struct_lit_width` (18) and
/// `max_width` (100), using field init shorthand when `k == v`.
pub(crate) fn fmt_struct_lit(
    indent: usize,
    path: &str,
    fields: &[(String, String)],
    leading_col: usize,
    trailing_len: usize,
) -> String {
    if fields.is_empty() {
        return format!("{path} {{}}");
    }
    let all_single = fields.iter().all(|(_, v)| !v.contains('\n'));
    let rendered_fields: Vec<String> = fields
        .iter()
        .map(|(k, v)| {
            if k == v {
                k.clone()
            } else {
                format!("{k}: {v}")
            }
        })
        .collect();
    if all_single {
        let joined = rendered_fields.join(", ");
        if joined.len() <= STRUCT_LIT_WIDTH
            && leading_col + path.len() + 5 + joined.len() + trailing_len <= MAX_WIDTH
        {
            return format!("{path} {{ {joined} }}");
        }
    }
    let inner_indent = indent + 4;
    let inner_prefix = " ".repeat(inner_indent);
    let close_prefix = " ".repeat(indent);
    let mut out = format!("{path} {{\n");
    for (k, v) in fields {
        if k == v {
            let _ = writeln!(out, "{inner_prefix}{k},");
        } else if !v.contains('\n')
            && inner_indent + k.len() + 2 + v.len() >= MAX_WIDTH
            && inner_indent + 4 + v.len() < MAX_WIDTH
        {
            let _ = writeln!(out, "{inner_prefix}{k}:\n{inner_prefix}    {v},");
        } else {
            let _ = writeln!(out, "{inner_prefix}{k}: {v},");
        }
    }
    let _ = write!(out, "{close_prefix}}}");
    out
}

/// Wrap a single-dot argument expression (`&value.<field>` or `value.<field>`) on its own line
/// inside a multi-line call or tuple when `rustfmt` would break at `.`.
fn fmt_multiline_arg(inner_indent: usize, arg: &str) -> String {
    if !arg.contains('\n')
        && inner_indent + arg.len() + 1 > MAX_WIDTH
        && let Some((recv, field)) = arg.split_once('.')
        && !field.contains('.')
        && !recv.contains(' ')
        && inner_indent + 4 + 1 + field.len() < MAX_WIDTH
    {
        let child_prefix = " ".repeat(inner_indent + 4);
        return format!("{recv}\n{child_prefix}.{field}");
    }
    arg.to_string()
}

/// Format a function call `callee(arg0, arg1)` matching `rustfmt`'s `fn_call_width` (60) and
/// `max_width` (100).
pub(crate) fn fmt_call(
    indent: usize,
    callee: &str,
    args: &[String],
    leading_col: usize,
    trailing_len: usize,
) -> String {
    if args.is_empty() {
        return format!("{callee}()");
    }
    let all_single = args.iter().all(|a| !a.contains('\n'));
    if all_single {
        let joined = args.join(", ");
        if joined.len() <= FN_CALL_WIDTH
            && leading_col + callee.len() + 2 + joined.len() + trailing_len <= MAX_WIDTH
        {
            return format!("{callee}({joined})");
        }
    }
    let inner_indent = indent + 4;
    let inner_prefix = " ".repeat(inner_indent);
    let close_prefix = " ".repeat(indent);
    let mut out = format!("{callee}(\n");
    for arg in args {
        let rendered = fmt_multiline_arg(inner_indent, arg);
        let _ = writeln!(out, "{inner_prefix}{rendered},");
    }
    let _ = write!(out, "{close_prefix})");
    out
}

/// Format a tuple expression `(a, b, ...)` matching `rustfmt`'s `max_width` (100).
pub(crate) fn fmt_tuple_expr(
    indent: usize,
    elems: &[String],
    leading_col: usize,
    trailing_len: usize,
) -> String {
    let all_single = elems.iter().all(|e| !e.contains('\n'));
    if all_single {
        let joined = elems.join(", ");
        if leading_col + 2 + joined.len() + trailing_len <= MAX_WIDTH {
            return format!("({joined})");
        }
    }
    let inner_indent = indent + 4;
    let inner_prefix = " ".repeat(inner_indent);
    let close_prefix = " ".repeat(indent);
    let mut out = String::from("(\n");
    for elem in elems {
        let rendered = fmt_multiline_arg(inner_indent, elem);
        let _ = writeln!(out, "{inner_prefix}{rendered},");
    }
    let _ = write!(out, "{close_prefix})");
    out
}

/// Format a single-expression `pat => ::core::result::Result::Ok(inner),` match arm matching
/// `rustfmt`: single line when `<= MAX_WIDTH`, block `{ Ok(inner) }` when `Ok(inner)` fits on one
/// line inside the block (`inner.len() <= FN_CALL_WIDTH`), and block-free multi-line
/// `Ok(\n    inner,\n),` when `Ok(inner)` itself must wrap.
pub(crate) fn fmt_match_arm_ok(arm_indent: usize, pat: &str, inner: &str) -> String {
    let prefix = " ".repeat(arm_indent);
    let ok_single = format!("::core::result::Result::Ok({inner})");
    let single = format!("{prefix}{pat} => {ok_single},");
    if !pat.contains('\n') && !inner.contains('\n') && single.len() <= MAX_WIDTH {
        return single;
    }
    let body_indent = arm_indent + 4;
    let body_prefix = " ".repeat(body_indent);
    if !inner.contains('\n')
        && inner.len() <= FN_CALL_WIDTH
        && body_indent + ok_single.len() <= MAX_WIDTH
    {
        return format!("{prefix}{pat} => {{\n{body_prefix}{ok_single}\n{prefix}}}");
    }
    let ok_header = format!("{prefix}{pat} => ::core::result::Result::Ok(");
    if !pat.contains('\n') && ok_header.len() <= MAX_WIDTH {
        let inner_prefix = " ".repeat(arm_indent + 4);
        return format!("{ok_header}\n{inner_prefix}{inner},\n{prefix}),");
    }
    let inner_prefix = " ".repeat(body_indent + 4);
    format!(
        "{prefix}{pat} => {{\n{body_prefix}::core::result::Result::Ok(\n{inner_prefix}{inner},\n{body_prefix})\n{prefix}}}"
    )
}

/// Format a `let` statement at `indent`, wrapping the RHS onto the next line (and wrapping call
/// arguments if needed) when single-line exceeds `MAX_WIDTH`.
pub(crate) fn fmt_let_stmt(indent: usize, binding: &str, expr: &str) -> String {
    let prefix = " ".repeat(indent);
    if !expr.contains('\n') {
        let single = format!("{prefix}let {binding} = {expr};");
        if single.len() <= MAX_WIDTH {
            return single;
        }
        if indent + 4 + expr.len() < MAX_WIDTH {
            return format!("{prefix}let {binding} =\n{prefix}    {expr};");
        }
        let (call_core, call_suffix) = if let Some(stripped) = expr.strip_suffix(")?") {
            (Some(format!("{stripped})")), "?")
        } else if expr.ends_with(')') {
            (Some(expr.to_string()), "")
        } else {
            (None, "")
        };
        if let Some(core) = call_core
            && let Some(open) = find_top_level_char(&core, '(')
            && let Some(args) = split_top_level(&core[open + 1..core.len() - 1], ',')
        {
            let callee = &core[..open];
            let trimmed: Vec<String> = args
                .into_iter()
                .map(|arg| arg.trim().to_string())
                .filter(|arg| !arg.is_empty())
                .collect();
            let head = format!("{prefix}let {binding} = ");
            let rhs_indent = indent + 4;
            if head.len() + callee.len() + call_suffix.len() >= MAX_WIDTH
                && rhs_indent + callee.len() + call_suffix.len() < MAX_WIDTH
            {
                let rhs_prefix = " ".repeat(rhs_indent);
                let wrapped = fmt_call(
                    rhs_indent,
                    callee,
                    &trimmed,
                    rhs_indent,
                    call_suffix.len() + 1,
                );
                return format!("{prefix}let {binding} =\n{rhs_prefix}{wrapped}{call_suffix};");
            }
            let wrapped = fmt_call(indent, callee, &trimmed, head.len(), call_suffix.len() + 1);
            return format!("{head}{wrapped}{call_suffix};");
        }
        // Single-dot field access (`&value.<long_field>` or `value.<long_field>`): when breaking
        // after `=` still exceeds `MAX_WIDTH`, `rustfmt` breaks at `.` if that fits and otherwise
        // keeps the assignment on a single line.
        if let Some((recv, field)) = expr.split_once('.')
            && !field.contains('.')
            && !recv.contains(' ')
        {
            if indent + 4 + 1 + field.len() < MAX_WIDTH {
                return format!("{prefix}let {binding} = {recv}\n{prefix}    .{field};");
            }
            return format!("{prefix}let {binding} = {expr};");
        }
        return format!("{prefix}let {binding} =\n{prefix}    {expr};");
    }
    format!("{prefix}let {binding} = {expr};")
}

/// Format a `pub const` item (`header = value;`) at `indent`.
pub(crate) fn fmt_const_item(indent: usize, header: &str, value: &str) -> String {
    let prefix = " ".repeat(indent);
    let single = format!("{prefix}{header} = {value};");
    if single.len() <= MAX_WIDTH {
        single
    } else {
        format!("{prefix}{header} =\n{prefix}    {value};")
    }
}

/// Format a `match` arm at `indent`.
pub(crate) fn fmt_match_arm(indent: usize, pat: &str, stmts: &[String], tail: &str) -> String {
    let prefix = " ".repeat(indent);
    if stmts.is_empty() && !tail.contains('\n') && !pat.contains('\n') {
        let single = format!("{prefix}{pat} => {tail},");
        if single.len() <= MAX_WIDTH {
            return single;
        }
    }
    let inner_prefix = " ".repeat(indent + 4);
    let mut out = format!("{prefix}{pat} => {{\n");
    for stmt in stmts {
        let _ = writeln!(out, "{stmt}");
    }
    if !tail.is_empty() {
        let _ = writeln!(out, "{inner_prefix}{tail}");
    }
    let _ = write!(out, "{prefix}}}");
    out
}

/// Format a slice literal `&["a", "b"]` for `worlds.rs`.
pub(crate) fn fmt_string_slice(
    indent: usize,
    items: &[String],
    leading_col: usize,
    trailing_len: usize,
) -> String {
    if items.is_empty() {
        return "&[]".to_string();
    }
    let joined = items.join(", ");
    if joined.len() <= ARRAY_WIDTH && leading_col + 3 + joined.len() + trailing_len <= MAX_WIDTH {
        return format!("&[{joined}]");
    }
    let inner_prefix = " ".repeat(indent + 4);
    let close_prefix = " ".repeat(indent);
    let mut out = String::from("&[\n");
    for item in items {
        let _ = writeln!(out, "{inner_prefix}{item},");
    }
    let _ = write!(out, "{close_prefix}]");
    out
}

/// Format a `let <binding> = <callee>(<args>)[?];` statement cleanly within `MAX_WIDTH`.
pub(crate) fn fmt_let_call(
    indent: usize,
    binding: &str,
    callee: &str,
    args: &[String],
    try_op: bool,
) -> String {
    let prefix = " ".repeat(indent);
    let q = if try_op { "?" } else { "" };
    let head = format!("{prefix}let {binding} = ");
    let single_call = format!("{callee}({})", args.join(", "));
    let single = format!("{head}{single_call}{q};");
    if !single_call.contains('\n') && single.len() <= MAX_WIDTH {
        return single;
    }
    if !single_call.contains('\n') && indent + 4 + single_call.len() + q.len() < MAX_WIDTH {
        return format!("{prefix}let {binding} =\n{prefix}    {single_call}{q};");
    }
    let rhs_indent = indent + 4;
    if head.len() + callee.len() + q.len() >= MAX_WIDTH
        && rhs_indent + callee.len() + q.len() < MAX_WIDTH
    {
        let rhs_prefix = " ".repeat(rhs_indent);
        let wrapped = fmt_call(rhs_indent, callee, args, rhs_indent, q.len() + 1);
        return format!("{prefix}let {binding} =\n{rhs_prefix}{wrapped}{q};");
    }
    let wrapped = fmt_call(indent, callee, args, head.len(), q.len() + 1);
    format!("{head}{wrapped}{q};")
}

fn find_top_level_char(text: &str, needle: char) -> Option<usize> {
    let mut angle = 0usize;
    let mut paren = 0usize;
    for (idx, ch) in text.char_indices() {
        if angle == 0 && paren == 0 && ch == needle {
            return Some(idx);
        }
        match ch {
            '<' => angle += 1,
            '>' => angle = angle.saturating_sub(1),
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            _ => {}
        }
    }
    None
}

fn split_top_level(text: &str, sep: char) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let mut angle = 0usize;
    let mut paren = 0usize;
    let mut start = 0;
    for (idx, ch) in text.char_indices() {
        match ch {
            '<' => angle += 1,
            '>' => angle = angle.saturating_sub(1),
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            c if c == sep && angle == 0 && paren == 0 => {
                parts.push(&text[start..idx]);
                start = idx + c.len_utf8();
            }
            _ => {}
        }
    }
    if angle != 0 || paren != 0 {
        return None;
    }
    parts.push(&text[start..]);
    Some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_let_stmt_and_let_call_break_at_exact_rustfmt_boundaries() {
        // `head = "    let v0 = "` (len 13). With `callee` len 86, `head + callee == 99`:
        // without `?` it stays on the same line, with `?` (`99 + 1 >= 100`) it breaks after `=`.
        let callee_86 = format!("a::{}", "b".repeat(83));
        assert_eq!(callee_86.len(), 86);
        let args = vec![
            "arg_one_long_name".to_string(),
            "arg_two_long_name".to_string(),
        ];

        let no_try_call = fmt_let_call(4, "v0", &callee_86, &args, false);
        assert!(
            no_try_call.starts_with(&format!("    let v0 = {callee_86}(\n")),
            "expected no break after `=` for len 99 without `?`: {no_try_call}"
        );
        let try_call = fmt_let_call(4, "v0", &callee_86, &args, true);
        assert!(
            try_call.starts_with(&format!("    let v0 =\n        {callee_86}(\n")),
            "expected break after `=` for len 99 with `?`: {try_call}"
        );

        let expr_try = format!("{callee_86}(arg_one_long_name, arg_two_long_name)?");
        let stmt_try = fmt_let_stmt(4, "v0", &expr_try);
        assert_eq!(stmt_try, try_call);

        // Single-dot field access longer than `MAX_WIDTH` after `=`: breaks at `.` when that fits,
        // otherwise keeps single line matching rustfmt.
        let field_90 = "f".repeat(90);
        let dot_break = fmt_let_stmt(4, "v0", &format!("&value.{field_90}"));
        assert_eq!(
            dot_break,
            format!("    let v0 = &value\n        .{field_90};")
        );
        let field_95 = "f".repeat(95);
        let no_break = fmt_let_stmt(4, "v0", &format!("&value.{field_95}"));
        assert_eq!(no_break, format!("    let v0 = &value.{field_95};"));
    }
}
