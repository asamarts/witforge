//! Unified identifier conversion for plain Rust, protobuf, `prost-build`, and glue.
//!
//! `.proto` names stay stable across releases. Plain Rust, protobuf, and glue all derive their
//! identifiers from this module so that every generator agrees on the spelling of one-letter
//! words, upper-case acronyms, digits, enum values, oneof modules, and escaped keywords.
//!
//! The `prost-build` conversions reproduce `prost-build` 0.14's default naming (`heck`'s
//! `ToSnakeCase` / `ToUpperCamelCase`, `sanitize_identifier`, and `strip_enum_prefix`) and are
//! tested directly against `prost-build` output.

use anyhow::Result;

/// The Rust keyword and reserved-word table used by plain Rust and glue module-root validation.
pub(crate) const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
    "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "try", "typeof", "union", "unsized", "virtual", "yield",
];

/// Split `s` into words using the exact word-boundary rule of `heck` 0.5 (used by `prost-build`
/// 0.14): non-alphanumeric separators split words, a lowercase-to-uppercase transition splits
/// before the uppercase letter, and an uppercase run followed by a lowercase letter splits before
/// the final uppercase letter (`XMLHttpRequest` -> `XML`, `Http`, `Request`).
fn split_words(s: &str) -> Vec<&str> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum WordMode {
        Boundary,
        Lowercase,
        Uppercase,
    }

    let mut words = Vec::new();
    for chunk in s.split(|c: char| !c.is_alphanumeric()) {
        let mut char_indices = chunk.char_indices().peekable();
        let mut init = 0;
        let mut mode = WordMode::Boundary;

        while let Some((i, c)) = char_indices.next() {
            if let Some(&(next_i, next)) = char_indices.peek() {
                let next_mode = if c.is_lowercase() {
                    WordMode::Lowercase
                } else if c.is_uppercase() {
                    WordMode::Uppercase
                } else {
                    mode
                };

                if next_mode == WordMode::Lowercase && next.is_uppercase() {
                    words.push(&chunk[init..next_i]);
                    init = next_i;
                    mode = WordMode::Boundary;
                } else if mode == WordMode::Uppercase && c.is_uppercase() && next.is_lowercase() {
                    words.push(&chunk[init..i]);
                    init = i;
                    mode = WordMode::Boundary;
                } else {
                    mode = next_mode;
                }
            } else {
                words.push(&chunk[init..]);
                break;
            }
        }
    }
    words
}

/// Convert `s` to `lower_snake_case` using `heck`'s word-boundary rules.
pub(crate) fn to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (idx, word) in split_words(s).into_iter().enumerate() {
        if idx > 0 {
            out.push('_');
        }
        for ch in word.chars() {
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}

/// Convert `s` to `UpperCamelCase` using `heck`'s word-boundary rules.
pub(crate) fn to_upper_camel_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in split_words(s) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            for ch in chars {
                out.push(ch.to_ascii_lowercase());
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Plain Rust identifiers
// ---------------------------------------------------------------------------

/// Convert a WIT name to its plain Rust `snake_case` spelling (`URL` -> `url`,
/// `HTTP-api` -> `http_api`, `foo-BAR` -> `foo_bar`, `foo2-bar` -> `foo2_bar`).
pub(crate) fn plain_snake(wit_name: &str) -> String {
    to_snake_case(wit_name)
}

/// Convert a WIT name to its plain Rust `UpperCamelCase` spelling (`HTTP` -> `Http`,
/// `HTTP-api` -> `HttpApi`, `foo-BAR` -> `FooBar`, `x-kind` -> `XKind`, `foo2-bar` -> `Foo2Bar`).
pub(crate) fn plain_pascal(wit_name: &str) -> String {
    to_upper_camel_case(wit_name)
}

/// Escape a plain Rust identifier, rejecting names that cannot be represented.
///
/// Raw identifiers (`r#...`) are used for every keyword in [`RUST_KEYWORDS`] except `crate`,
/// `self`, `super`, and `Self`, which cannot be raw identifiers and receive a trailing `_`. A
/// trailing `_` cannot collide with another WIT name because WIT identifiers cannot contain `_`.
pub(crate) fn plain_ident(name: &str, what: &str) -> Result<String> {
    if name.is_empty() || name == "_" {
        contract_bail!("{what} has no Rust-compatible name");
    }
    if name.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        contract_bail!("{what} `{name}` starts with a digit");
    }
    let suffix = ["crate", "self", "super", "Self"];
    if suffix.contains(&name) {
        Ok(format!("{name}_"))
    } else if RUST_KEYWORDS.contains(&name) {
        Ok(format!("r#{name}"))
    } else {
        Ok(name.to_string())
    }
}

// ---------------------------------------------------------------------------
// Protobuf (.proto) identifiers — stable wire/JSON names
// ---------------------------------------------------------------------------

/// Convert one WIT name segment to its `.proto` `PascalCase` spelling.
///
/// Splits on `-`, upper-cases the first byte of each segment, and preserves the rest verbatim so
/// existing `.proto` message, enum, and wrapper names never move.
pub(crate) fn proto_pascal(wit_name: &str) -> String {
    let mut out = String::new();
    for part in wit_name.split('-') {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    out
}

/// The `.proto` top-level type name for `interface/ty`.
pub(crate) fn proto_type_name(interface: &str, ty: &str) -> String {
    format!("{}{}", proto_pascal(interface), proto_pascal(ty))
}

/// The `.proto` request or response message name for `interface#function`.
pub(crate) fn proto_function_message_name(interface: &str, function: &str, suffix: &str) -> String {
    format!(
        "{}{}{suffix}",
        proto_pascal(interface),
        proto_pascal(function)
    )
}

/// The `.proto` method-id enum name for `interface`.
pub(crate) fn proto_method_enum_name(interface: &str) -> String {
    format!("{}Method", proto_pascal(interface))
}

/// The `.proto` field or oneof-case spelling of a WIT member name.
pub(crate) fn proto_field_name(wit_name: &str) -> String {
    wit_name.replace('-', "_")
}

/// The `.proto` package/filename segment for a WIT namespace or package name.
pub(crate) fn proto_package_segment(wit_name: &str) -> String {
    wit_name.replace('-', "_")
}

/// The `.proto` `UPPER_SNAKE` spelling used for enum prefixes, enum values, and method values.
pub(crate) fn proto_upper_snake(name: &str) -> String {
    let mut out = String::new();
    let mut previous_lower = false;
    for ch in name.chars() {
        if ch == '-' || ch == '_' {
            out.push('_');
            previous_lower = false;
        } else if ch.is_ascii_uppercase() {
            if previous_lower {
                out.push('_');
            }
            out.push(ch);
            previous_lower = false;
        } else {
            out.push(ch.to_ascii_uppercase());
            previous_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        }
    }
    out
}

/// The `.proto` zero (`= 0`) value name for a user enum whose `.proto` name is `proto_enum_name`.
pub(crate) fn proto_enum_zero_value(proto_enum_name: &str) -> String {
    format!("{}_UNSPECIFIED", proto_upper_snake(proto_enum_name))
}

/// The `.proto` value name for one case of an enum whose `.proto` name is `proto_enum_name`.
pub(crate) fn proto_enum_value_name(proto_enum_name: &str, wit_case: &str) -> String {
    format!(
        "{}_{}",
        proto_upper_snake(proto_enum_name),
        proto_upper_snake(wit_case)
    )
}

/// The `.proto` zero (`= 0`) value name for `interface`'s method enum.
pub(crate) fn proto_method_enum_zero_value(interface: &str) -> String {
    proto_enum_zero_value(&proto_method_enum_name(interface))
}

/// The `.proto` value name for `interface#function` in `interface`'s method enum.
pub(crate) fn proto_method_enum_value(interface: &str, wit_function: &str) -> String {
    format!(
        "{}_METHOD_{}",
        proto_upper_snake(&proto_pascal(interface)),
        proto_upper_snake(wit_function)
    )
}

/// The `.proto` structural shape segment for a foreign named type inside a synthesized wrapper.
pub(crate) fn proto_dependency_shape_name(
    namespace: &str,
    package: &str,
    interface: &str,
    ty: &str,
) -> String {
    format!(
        "{}{}{}{}",
        proto_pascal(namespace),
        proto_pascal(package),
        proto_pascal(interface),
        proto_pascal(ty)
    )
}

// ---------------------------------------------------------------------------
// prost-build 0.14 default identifiers (derived from `.proto` names)
// ---------------------------------------------------------------------------

/// Escape a Rust identifier using the exact table from `prost-build` 0.14 (`src/ident.rs`).
///
/// A trailing underscore (`_`, `super_`, `self_`, `Self_`, `extern_`, `crate_`) cannot collide
/// with another WIT name because WIT identifiers cannot contain underscores.
pub(crate) fn prost_sanitize_ident(ident: &str) -> String {
    match ident {
        // 2015 strict keywords.
        "as" | "break" | "const" | "continue" | "else" | "enum" | "false" | "fn" | "for"
        | "if" | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut" | "pub"
        | "ref" | "return" | "static" | "struct" | "trait" | "true" | "type" | "unsafe" | "use"
        | "where" | "while"
        // 2018 strict keywords.
        | "dyn"
        // 2015 reserved keywords.
        | "abstract" | "become" | "box" | "do" | "final" | "macro" | "override" | "priv"
        | "typeof" | "unsized" | "virtual" | "yield"
        // 2018 reserved keywords.
        | "async" | "await" | "try"
        // 2024 reserved keywords.
        | "gen" => format!("r#{ident}"),
        // Keywords prost-build suffixes with `_` instead of `r#`.
        "_" | "super" | "self" | "Self" | "extern" | "crate" => format!("{ident}_"),
        s if s.starts_with(|c: char| c.is_numeric()) => format!("_{ident}"),
        _ => ident.to_string(),
    }
}

/// Convert a `.proto` identifier to a `prost-build` `lower_snake_case` Rust identifier.
pub(crate) fn prost_to_snake(proto_name: &str) -> String {
    prost_sanitize_ident(&to_snake_case(proto_name))
}

/// Convert a `.proto` identifier to a `prost-build` `UpperCamelCase` Rust identifier.
pub(crate) fn prost_to_upper_camel(proto_name: &str) -> String {
    prost_sanitize_ident(&to_upper_camel_case(proto_name))
}

/// Reproduce `prost-build`'s `strip_enum_prefix(prefix, name)`.
///
/// Both `enum_ident` and `value_ident` must already be converted with [`prost_to_upper_camel`].
pub(crate) fn prost_strip_enum_prefix(enum_ident: &str, value_ident: &str) -> String {
    let stripped = value_ident.strip_prefix(enum_ident).unwrap_or(value_ident);
    let stripped = if stripped.chars().next().is_some_and(char::is_uppercase) {
        stripped
    } else {
        value_ident
    };
    prost_sanitize_ident(stripped)
}

/// The Rust struct or enum identifier `prost-build` generates for a `.proto` message or enum.
pub(crate) fn prost_type_ident(proto_name: &str) -> String {
    prost_to_upper_camel(proto_name)
}

/// The Rust struct field identifier `prost-build` generates for a `.proto` field name.
pub(crate) fn prost_field_ident(proto_field: &str) -> String {
    prost_to_snake(proto_field)
}

/// The Rust module identifier `prost-build` generates for nested types/oneofs of a `.proto`
/// message.
pub(crate) fn prost_oneof_module(proto_message: &str) -> String {
    prost_to_snake(proto_message)
}

/// The Rust enum identifier `prost-build` generates for a `.proto` `oneof` declaration.
pub(crate) fn prost_oneof_enum(proto_oneof: &str) -> String {
    prost_to_upper_camel(proto_oneof)
}

/// The Rust enum variant identifier `prost-build` generates for a `.proto` `oneof` case field.
pub(crate) fn prost_oneof_variant(proto_field: &str) -> String {
    prost_to_upper_camel(proto_field)
}

/// The Rust enum variant identifier `prost-build` generates for a `.proto` enum value.
pub(crate) fn prost_enum_variant(proto_enum_name: &str, proto_value_name: &str) -> String {
    let enum_ident = prost_to_upper_camel(proto_enum_name);
    let value_ident = prost_to_upper_camel(proto_value_name);
    prost_strip_enum_prefix(&enum_ident, &value_ident)
}

/// The Rust module segment `prost-build` generates for one protobuf package segment.
pub(crate) fn prost_package_segment(wit_segment: &str) -> String {
    prost_to_snake(&proto_package_segment(wit_segment))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prost_conversions_match_documented_reproductions() {
        // enum x-kind { foo } -> proto TYPES_XKIND_FOO -> prost TypesXKind::TypesXkindFoo
        let proto_enum = proto_type_name("types", "x-kind");
        assert_eq!(proto_enum, "TypesXKind");
        let proto_val = proto_enum_value_name(&proto_enum, "foo");
        assert_eq!(proto_val, "TYPES_XKIND_FOO");
        assert_eq!(prost_type_ident(&proto_enum), "TypesXKind");
        assert_eq!(prost_enum_variant(&proto_enum, &proto_val), "TypesXkindFoo");
        assert_eq!(prost_oneof_module(&proto_enum), "types_x_kind");

        // record x-y, get-x-y: func -> proto ApiXY, ApiGetXYRequest -> prost ApiXy, ApiGetXyRequest
        assert_eq!(proto_type_name("api", "x-y"), "ApiXY");
        assert_eq!(prost_type_ident("ApiXY"), "ApiXy");
        assert_eq!(
            proto_function_message_name("api", "get-x-y", "Request"),
            "ApiGetXYRequest"
        );
        assert_eq!(prost_type_ident("ApiGetXYRequest"), "ApiGetXyRequest");
        assert_eq!(
            prost_enum_variant(
                &proto_method_enum_name("api"),
                &proto_method_enum_value("api", "get-x-y")
            ),
            "GetXY"
        );

        // record r { foo-BAR: u32 } -> proto foo_BAR -> prost foo_bar
        assert_eq!(proto_field_name("foo-BAR"), "foo_BAR");
        assert_eq!(prost_field_ident("foo_BAR"), "foo_bar");
        assert_eq!(plain_snake("foo-BAR"), "foo_bar");

        // enum e { HTTP, other } -> proto TYPES_E_HTTP -> prost TypesE::Http
        let e = proto_type_name("types", "e");
        assert_eq!(e, "TypesE");
        assert_eq!(proto_enum_value_name(&e, "HTTP"), "TYPES_E_HTTP");
        assert_eq!(prost_enum_variant(&e, "TYPES_E_HTTP"), "Http");
        assert_eq!(plain_pascal("HTTP"), "Http");

        // One-letter interface method enum zero value vs method value
        assert_eq!(proto_method_enum_name("a"), "AMethod");
        assert_eq!(proto_method_enum_zero_value("a"), "AMETHOD_UNSPECIFIED");
        assert_eq!(proto_method_enum_value("a", "ping"), "A_METHOD_PING");
        assert_eq!(
            prost_enum_variant("AMethod", "AMETHOD_UNSPECIFIED"),
            "AmethodUnspecified"
        );
        assert_eq!(prost_enum_variant("AMethod", "A_METHOD_PING"), "Ping");

        // Digits inside or at the end of a word
        assert_eq!(plain_snake("foo2-bar"), "foo2_bar");
        assert_eq!(plain_pascal("foo2-bar"), "Foo2Bar");
        assert_eq!(prost_field_ident(&proto_field_name("foo2-bar")), "foo2_bar");
        assert_eq!(prost_type_ident(&proto_type_name("api", "a1")), "ApiA1");
        assert_eq!(prost_oneof_module("ApiA1"), "api_a1");

        // api-x/r and api/xr map to one prost struct name
        assert_eq!(prost_type_ident(&proto_type_name("api-x", "r")), "ApiXr");
        assert_eq!(prost_type_ident(&proto_type_name("api", "xr")), "ApiXr");

        // interface API -> mod api + trait Api (no collision in plain Rust)
        assert_eq!(plain_snake("API"), "api");
        assert_eq!(plain_pascal("API"), "Api");

        // prost vs plain escaping differences
        assert_eq!(prost_sanitize_ident("extern"), "extern_");
        assert_eq!(plain_ident("extern", "field").unwrap(), "r#extern");
        assert_eq!(prost_sanitize_ident("gen"), "r#gen");
        assert_eq!(prost_sanitize_ident("union"), "union");
        assert_eq!(plain_ident("union", "field").unwrap(), "r#union");
    }
}
