//! Shared conventions, identifier helpers, keyword escaping, and module-path helpers for
//! built-in polyglot emitters (TypeScript, Python, Go, Java, and C#).
//!
//! All helpers in this module are pure and deterministic, add no external dependencies, and never
//! reject valid WIT identifiers for target-language keywords: every target language escapes its
//! own reserved words deterministically so `witforge check` stays target-agnostic.

use crate::emit::docs;
use crate::emit::ident;
use crate::emit::output::HeaderStyle;

/// Built-in polyglot emitter target language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum TargetLanguage {
    /// TypeScript 5+ / 6+ strict ESM (plain) and `@bufbuild/protobuf` (glue).
    TypeScript,
    /// Python 3.11+ stdlib dataclasses (plain) and `google.protobuf` (glue).
    Python,
    /// Go 1.22+ stdlib (plain) and `google.golang.org/protobuf` (glue).
    Go,
    /// Java 21 LTS+ records and sealed interfaces (plain) and `com.google.protobuf` (glue).
    Java,
    /// C# 12+ / .NET 8 LTS+ sealed records (plain) and `Google.Protobuf` (glue).
    CSharp,
}

impl TargetLanguage {
    /// CLI subcommand name for the plain emitter (`ts`, `py`, `go`, `java`, `csharp`).
    #[must_use]
    pub const fn cli_name(self) -> &'static str {
        match self {
            Self::TypeScript => "ts",
            Self::Python => "py",
            Self::Go => "go",
            Self::Java => "java",
            Self::CSharp => "csharp",
        }
    }

    /// CLI subcommand name for the checked protobuf glue emitter (`ts-glue`, `py-glue`,
    /// `go-glue`, `java-glue`, `csharp-glue`).
    #[must_use]
    pub const fn glue_cli_name(self) -> &'static str {
        match self {
            Self::TypeScript => "ts-glue",
            Self::Python => "py-glue",
            Self::Go => "go-glue",
            Self::Java => "java-glue",
            Self::CSharp => "csharp-glue",
        }
    }

    /// `witforge.toml` plain emitter table key (`typescript`, `python`, `go`, `java`, `csharp`).
    #[must_use]
    pub const fn config_key(self) -> &'static str {
        match self {
            Self::TypeScript => "typescript",
            Self::Python => "python",
            Self::Go => "go",
            Self::Java => "java",
            Self::CSharp => "csharp",
        }
    }

    /// `witforge.toml` glue emitter table key (`typescript-glue`, `python-glue`, `go-glue`,
    /// `java-glue`, `csharp-glue`).
    #[must_use]
    pub const fn glue_config_key(self) -> &'static str {
        match self {
            Self::TypeScript => "typescript-glue",
            Self::Python => "python-glue",
            Self::Go => "go-glue",
            Self::Java => "java-glue",
            Self::CSharp => "csharp-glue",
        }
    }

    /// Ownership comment header syntax used for generated files of this language.
    #[must_use]
    pub const fn header_style(self) -> HeaderStyle {
        match self {
            Self::TypeScript | Self::Java | Self::CSharp => HeaderStyle::LineComment,
            Self::Python => HeaderStyle::HashComment,
            Self::Go => HeaderStyle::GoGenerated,
        }
    }

    /// Canonical source file extension without leading dot (`ts`, `py`, `go`, `java`, `cs`).
    #[must_use]
    pub const fn file_extension(self) -> &'static str {
        match self {
            Self::TypeScript => "ts",
            Self::Python => "py",
            Self::Go => "go",
            Self::Java => "java",
            Self::CSharp => "cs",
        }
    }

    /// Build the canonical ownership header line for this language's plain emitter.
    #[must_use]
    pub fn plain_header(self, namespace: &str, name: &str) -> String {
        self.header_style()
            .format_header(self.cli_name(), namespace, name)
    }

    /// Build the canonical ownership header line for this language's checked protobuf glue emitter.
    #[must_use]
    pub fn glue_header(self, namespace: &str, name: &str) -> String {
        self.header_style()
            .format_header(self.glue_cli_name(), namespace, name)
    }
}

// ---------------------------------------------------------------------------
// Case converters
// ---------------------------------------------------------------------------

/// Convert a WIT identifier to `lower_snake_case` (`start-byte` -> `start_byte`,
/// `HTTP-api` -> `http_api`, `URL` -> `url`).
#[must_use]
pub fn to_snake_case(wit_name: &str) -> String {
    ident::to_snake_case(wit_name)
}

/// Convert a WIT identifier to `UPPER_SNAKE_CASE` (`start-byte` -> `START_BYTE`,
/// `HTTP-api` -> `HTTP_API`, `URL` -> `URL`).
#[must_use]
pub fn to_upper_snake_case(wit_name: &str) -> String {
    ident::to_snake_case(wit_name).to_ascii_uppercase()
}

/// Convert a WIT identifier to `PascalCase` (`start-byte` -> `StartByte`,
/// `HTTP-api` -> `HttpApi`, `x-kind` -> `XKind`).
#[must_use]
pub fn to_pascal_case(wit_name: &str) -> String {
    ident::to_upper_camel_case(wit_name)
}

/// Convert a WIT identifier to `lowerCamelCase` (`start-byte` -> `startByte`,
/// `HTTP-api` -> `httpApi`, `URL` -> `url`, `x-kind` -> `xKind`).
#[must_use]
pub fn to_camel_case(wit_name: &str) -> String {
    let mut out = String::with_capacity(wit_name.len());
    for (idx, word) in ident::to_snake_case(wit_name).split('_').enumerate() {
        if word.is_empty() {
            continue;
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            if idx == 0 {
                out.push(first.to_ascii_lowercase());
            } else {
                out.push(first.to_ascii_uppercase());
            }
            for ch in chars {
                out.push(ch.to_ascii_lowercase());
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Keyword tables & deterministic escaping
// ---------------------------------------------------------------------------

/// ECMAScript / TypeScript reserved, strict-mode, and contextual keywords escaped by witforge.
pub(crate) const TYPESCRIPT_KEYWORDS: &[&str] = &[
    "abstract",
    "accessor",
    "any",
    "arguments",
    "as",
    "asserts",
    "async",
    "await",
    "bigint",
    "boolean",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "constructor",
    "continue",
    "debugger",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "eval",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "global",
    "if",
    "implements",
    "import",
    "in",
    "infer",
    "instanceof",
    "interface",
    "intrinsic",
    "is",
    "keyof",
    "let",
    "module",
    "namespace",
    "never",
    "new",
    "null",
    "number",
    "object",
    "of",
    "out",
    "override",
    "package",
    "private",
    "protected",
    "public",
    "readonly",
    "require",
    "return",
    "satisfies",
    "set",
    "static",
    "string",
    "super",
    "switch",
    "symbol",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "unique",
    "unknown",
    "using",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// Python 3.11+ hard and soft keywords (`keyword.kwlist` + `keyword.softkwlist`).
pub(crate) const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "_", "and", "as", "assert", "async", "await", "break", "case",
    "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from",
    "global", "if", "import", "in", "is", "lambda", "match", "nonlocal", "not", "or", "pass",
    "raise", "return", "try", "type", "while", "with", "yield",
];

/// Go 1.22+ language keywords.
pub(crate) const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];

/// Go 1.22+ predeclared universe identifiers that must not be shadowed in generated parameters,
/// local variables, or package names.
pub(crate) const GO_PREDECLARED: &[&str] = &[
    "any",
    "append",
    "bool",
    "byte",
    "cap",
    "clear",
    "close",
    "comparable",
    "complex",
    "complex128",
    "complex64",
    "copy",
    "delete",
    "error",
    "false",
    "float32",
    "float64",
    "imag",
    "int",
    "int16",
    "int32",
    "int64",
    "int8",
    "iota",
    "len",
    "make",
    "max",
    "min",
    "new",
    "nil",
    "panic",
    "print",
    "println",
    "real",
    "recover",
    "rune",
    "string",
    "true",
    "uint",
    "uint16",
    "uint32",
    "uint64",
    "uint8",
    "uintptr",
];

/// Java 21+ reserved keywords and boolean/null literals (`JLS §3.9`), plus `_`.
pub(crate) const JAVA_KEYWORDS: &[&str] = &[
    "_",
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "true",
    "try",
    "void",
    "volatile",
    "while",
];

/// Java 21+ restricted `TypeIdentifier`s (`JLS §3.8`) disallowed as class/record/interface/enum
/// names while still valid as package or variable identifiers.
pub(crate) const JAVA_RESTRICTED_TYPE_IDENTIFIERS: &[&str] =
    &["permits", "record", "sealed", "var", "yield"];

/// `java.lang.Object` methods that cannot be declared as Java record component names or
/// zero-arg interface methods without conflicting with `Object`.
pub(crate) const JAVA_OBJECT_METHODS: &[&str] = &[
    "clone",
    "equals",
    "finalize",
    "getClass",
    "hashCode",
    "notify",
    "notifyAll",
    "toString",
    "wait",
];

/// C# 12 / .NET 8 reserved keywords.
pub(crate) const CSHARP_KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "base",
    "bool",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "checked",
    "class",
    "const",
    "continue",
    "decimal",
    "default",
    "delegate",
    "do",
    "double",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "fixed",
    "float",
    "for",
    "foreach",
    "goto",
    "if",
    "implicit",
    "in",
    "int",
    "interface",
    "internal",
    "is",
    "lock",
    "long",
    "namespace",
    "new",
    "null",
    "object",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "ref",
    "return",
    "sbyte",
    "sealed",
    "short",
    "sizeof",
    "stackalloc",
    "static",
    "string",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "uint",
    "ulong",
    "unchecked",
    "unsafe",
    "ushort",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
];

/// `System.Object` / C# `record` synthesized member names that clash with record property names.
pub(crate) const CSHARP_OBJECT_MEMBERS: &[&str] = &[
    "Clone",
    "Deconstruct",
    "EqualityContract",
    "Equals",
    "Finalize",
    "GetHashCode",
    "GetType",
    "MemberwiseClone",
    "PrintMembers",
    "ToString",
];

/// Whether `ident` is a TypeScript/ECMAScript keyword or contextual reserved identifier.
#[must_use]
pub fn is_typescript_keyword(ident: &str) -> bool {
    TYPESCRIPT_KEYWORDS.binary_search(&ident).is_ok()
}

/// Deterministically escape a TypeScript identifier by appending `_` when reserved.
#[must_use]
pub fn escape_typescript_ident(ident: &str) -> String {
    if is_typescript_keyword(ident) {
        format!("{ident}_")
    } else {
        ident.to_string()
    }
}

/// Whether `ident` is a Python 3.11+ hard or soft keyword.
#[must_use]
pub fn is_python_keyword(ident: &str) -> bool {
    PYTHON_KEYWORDS.binary_search(&ident).is_ok()
}

/// Deterministically escape a Python identifier by appending `_` (PEP 8 convention) when reserved.
#[must_use]
pub fn escape_python_ident(ident: &str) -> String {
    if is_python_keyword(ident) {
        format!("{ident}_")
    } else {
        ident.to_string()
    }
}

/// Whether `ident` is a Go keyword or the blank identifier `_`.
#[must_use]
pub fn is_go_keyword(ident: &str) -> bool {
    ident == "_" || GO_KEYWORDS.binary_search(&ident).is_ok()
}

/// Whether `ident` is a Go predeclared universe identifier (`string`, `error`, `len`, etc.).
#[must_use]
pub fn is_go_predeclared(ident: &str) -> bool {
    GO_PREDECLARED.binary_search(&ident).is_ok()
}

/// Deterministically escape a Go identifier (parameter, local variable, or package identifier) by
/// appending `_` when it is a Go keyword, `_`, or predeclared universe identifier.
#[must_use]
pub fn escape_go_ident(ident: &str) -> String {
    if is_go_keyword(ident) || is_go_predeclared(ident) {
        format!("{ident}_")
    } else {
        ident.to_string()
    }
}

/// Derive a valid Go package identifier from a WIT name (`my-pkg` -> `my_pkg`, `type` -> `type_`).
#[must_use]
pub fn go_package_ident(wit_name: &str) -> String {
    escape_go_ident(&to_snake_case(wit_name))
}

/// Whether `ident` is a Java reserved keyword, literal (`true`/`false`/`null`), or `_`.
#[must_use]
pub fn is_java_keyword(ident: &str) -> bool {
    JAVA_KEYWORDS.binary_search(&ident).is_ok()
}

/// Whether `ident` is a Java 21+ restricted `TypeIdentifier` (`permits`, `record`, `sealed`,
/// `var`, `yield`).
#[must_use]
pub fn is_java_restricted_type_ident(ident: &str) -> bool {
    JAVA_RESTRICTED_TYPE_IDENTIFIERS
        .binary_search(&ident)
        .is_ok()
}

/// Whether `ident` clashes with a `java.lang.Object` method name (cannot be used as a Java
/// record component name).
#[must_use]
pub fn is_java_object_method(ident: &str) -> bool {
    JAVA_OBJECT_METHODS.binary_search(&ident).is_ok()
}

/// Deterministically escape a Java variable, parameter, method, or package segment identifier by
/// appending `_` when reserved.
#[must_use]
pub fn escape_java_ident(ident: &str) -> String {
    if is_java_keyword(ident) {
        format!("{ident}_")
    } else {
        ident.to_string()
    }
}

/// Deterministically escape a Java type name (`class`, `record`, `interface`, `enum`) by
/// appending `_` when it is a Java keyword or restricted `TypeIdentifier`.
#[must_use]
pub fn escape_java_type_ident(ident: &str) -> String {
    if is_java_keyword(ident) || is_java_restricted_type_ident(ident) {
        format!("{ident}_")
    } else {
        ident.to_string()
    }
}

/// Whether `ident` is a C# reserved keyword.
#[must_use]
pub fn is_csharp_keyword(ident: &str) -> bool {
    CSHARP_KEYWORDS.binary_search(&ident).is_ok()
}

/// Whether `ident` clashes with a `System.Object` or synthesized C# `record` member name.
#[must_use]
pub fn is_csharp_object_member(ident: &str) -> bool {
    CSHARP_OBJECT_MEMBERS.binary_search(&ident).is_ok()
}

/// Deterministically escape a C# identifier using C#'s verbatim `@` prefix when reserved.
#[must_use]
pub fn escape_csharp_ident(ident: &str) -> String {
    if is_csharp_keyword(ident) {
        format!("@{ident}")
    } else {
        ident.to_string()
    }
}

/// Deterministically escape `ident` for `language`.
#[must_use]
pub fn escape_ident(language: TargetLanguage, ident: &str) -> String {
    match language {
        TargetLanguage::TypeScript => escape_typescript_ident(ident),
        TargetLanguage::Python => escape_python_ident(ident),
        TargetLanguage::Go => escape_go_ident(ident),
        TargetLanguage::Java => escape_java_ident(ident),
        TargetLanguage::CSharp => escape_csharp_ident(ident),
    }
}

// ---------------------------------------------------------------------------
// Version & dependency module-path helpers (matching `DepLayout`)
// ---------------------------------------------------------------------------

/// Canonical per-version directory/module name (`v0_1`, `v1_0`).
#[must_use]
pub fn version_dir_name(major: u64, minor: u64) -> String {
    format!("v{major}_{minor}")
}

/// Canonical per-major directory/module name (`v0`, `v1`).
#[must_use]
pub fn major_dir_name(major: u64) -> String {
    format!("v{major}")
}

/// Canonical flattened `<namespace>_<package>` dependency module segment (`example`, `my-types` ->
/// `example_my_types`).
#[must_use]
pub fn dep_package_segment(namespace: &str, name: &str) -> String {
    format!("{}_{}", to_snake_case(namespace), to_snake_case(name))
}

/// Canonical root-relative POSIX directory path for a generated dependency package under
/// `witforge_deps/`.
///
/// Pass `root_major = None` for a shared dependency (`witforge_deps/<ns>_<pkg>`) and
/// `root_major = Some(m)` for a per-major scoped dependency (`witforge_deps/v<m>/<ns>_<pkg>`),
/// matching the internal `DepLayout` convention.
#[must_use]
pub fn dep_dir_path(root_major: Option<u64>, namespace: &str, name: &str) -> String {
    let seg = dep_package_segment(namespace, name);
    match root_major {
        Some(major) => format!("witforge_deps/v{major}/{seg}"),
        None => format!("witforge_deps/{seg}"),
    }
}

/// Canonical protobuf/glue major version tag for a dependency package (`v0` when shared, or
/// `v0_m1` when disambiguated by canonical owner root major `1`).
#[must_use]
pub fn disambiguated_major_tag(dep_major: u64, owner_major: Option<u64>) -> String {
    match owner_major {
        Some(owner) => format!("v{dep_major}_m{owner}"),
        None => format!("v{dep_major}"),
    }
}

/// Normalize WIT doc comments into clean LF-only lines with C0/C1 control characters and
/// decorative `/** *` block-comment gutters stripped.
#[must_use]
pub fn doc_lines(raw_docs: &str) -> Vec<String> {
    docs::normalize_lines(raw_docs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_tables_are_strictly_sorted_and_unique() {
        for (name, table) in [
            ("TYPESCRIPT_KEYWORDS", TYPESCRIPT_KEYWORDS),
            ("PYTHON_KEYWORDS", PYTHON_KEYWORDS),
            ("GO_KEYWORDS", GO_KEYWORDS),
            ("GO_PREDECLARED", GO_PREDECLARED),
            ("JAVA_KEYWORDS", JAVA_KEYWORDS),
            (
                "JAVA_RESTRICTED_TYPE_IDENTIFIERS",
                JAVA_RESTRICTED_TYPE_IDENTIFIERS,
            ),
            ("JAVA_OBJECT_METHODS", JAVA_OBJECT_METHODS),
            ("CSHARP_KEYWORDS", CSHARP_KEYWORDS),
            ("CSHARP_OBJECT_MEMBERS", CSHARP_OBJECT_MEMBERS),
        ] {
            for pair in table.windows(2) {
                assert!(
                    pair[0] < pair[1],
                    "{name} is not strictly sorted/unique at {:?} >= {:?}",
                    pair[0],
                    pair[1]
                );
            }
        }
    }

    #[test]
    fn case_converters_follow_consistent_word_boundaries() {
        assert_eq!(to_snake_case("start-byte"), "start_byte");
        assert_eq!(to_snake_case("HTTP-api"), "http_api");
        assert_eq!(to_snake_case("URL"), "url");
        assert_eq!(to_snake_case("foo2-bar"), "foo2_bar");

        assert_eq!(to_upper_snake_case("start-byte"), "START_BYTE");
        assert_eq!(to_upper_snake_case("HTTP-api"), "HTTP_API");

        assert_eq!(to_pascal_case("start-byte"), "StartByte");
        assert_eq!(to_pascal_case("HTTP-api"), "HttpApi");
        assert_eq!(to_pascal_case("x-kind"), "XKind");

        assert_eq!(to_camel_case("start-byte"), "startByte");
        assert_eq!(to_camel_case("HTTP-api"), "httpApi");
        assert_eq!(to_camel_case("URL"), "url");
        assert_eq!(to_camel_case("x-kind"), "xKind");
    }

    #[test]
    fn deterministic_keyword_escaping_covers_all_five_languages() {
        assert_eq!(escape_typescript_ident("type"), "type_");
        assert_eq!(escape_typescript_ident("default"), "default_");
        assert_eq!(escape_typescript_ident("span"), "span");

        assert_eq!(escape_python_ident("class"), "class_");
        assert_eq!(escape_python_ident("from"), "from_");
        assert_eq!(escape_python_ident("match"), "match_");
        assert_eq!(escape_python_ident("None"), "None_");
        assert_eq!(escape_python_ident("span"), "span");

        assert_eq!(escape_go_ident("func"), "func_");
        assert_eq!(escape_go_ident("_"), "__");
        assert_eq!(escape_go_ident("string"), "string_");
        assert_eq!(escape_go_ident("error"), "error_");
        assert_eq!(escape_go_ident("span"), "span");
        assert_eq!(go_package_ident("my-pkg"), "my_pkg");
        assert_eq!(go_package_ident("type"), "type_");

        assert_eq!(escape_java_ident("default"), "default_");
        assert_eq!(escape_java_ident("record"), "record");
        assert_eq!(escape_java_type_ident("record"), "record_");
        assert_eq!(escape_java_type_ident("var"), "var_");
        assert!(is_java_object_method("toString"));
        assert!(!is_java_object_method("startByte"));

        assert_eq!(escape_csharp_ident("class"), "@class");
        assert_eq!(escape_csharp_ident("string"), "@string");
        assert_eq!(escape_csharp_ident("Span"), "Span");
        assert!(is_csharp_object_member("EqualityContract"));
        assert!(!is_csharp_object_member("StartByte"));

        assert_eq!(escape_ident(TargetLanguage::TypeScript, "class"), "class_");
        assert_eq!(escape_ident(TargetLanguage::Python, "def"), "def_");
        assert_eq!(escape_ident(TargetLanguage::Go, "map"), "map_");
        assert_eq!(escape_ident(TargetLanguage::Java, "import"), "import_");
        assert_eq!(escape_ident(TargetLanguage::CSharp, "event"), "@event");
    }

    #[test]
    fn language_metadata_headers_paths_and_doc_lines_are_canonical() {
        assert_eq!(
            TargetLanguage::TypeScript.plain_header("example", "plugin"),
            "// @generated by witforge (ts, example:plugin); do not edit."
        );
        assert_eq!(
            TargetLanguage::Python.plain_header("example", "plugin"),
            "# @generated by witforge (py, example:plugin); do not edit."
        );
        assert_eq!(
            TargetLanguage::Go.plain_header("example", "plugin"),
            "// Code generated by witforge (go, example:plugin); DO NOT EDIT."
        );
        assert_eq!(
            TargetLanguage::Go.glue_header("example", "plugin"),
            "// Code generated by witforge (go-glue, example:plugin); DO NOT EDIT."
        );
        assert_eq!(
            TargetLanguage::Java.glue_header("example", "plugin"),
            "// @generated by witforge (java-glue, example:plugin); do not edit."
        );
        assert_eq!(
            TargetLanguage::CSharp.plain_header("example", "plugin"),
            "// @generated by witforge (csharp, example:plugin); do not edit."
        );

        assert_eq!(version_dir_name(0, 1), "v0_1");
        assert_eq!(major_dir_name(2), "v2");
        assert_eq!(dep_package_segment("shared", "my-types"), "shared_my_types");
        assert_eq!(
            dep_dir_path(None, "shared", "my-types"),
            "witforge_deps/shared_my_types"
        );
        assert_eq!(
            dep_dir_path(Some(1), "shared", "my-types"),
            "witforge_deps/v1/shared_my_types"
        );
        assert_eq!(disambiguated_major_tag(0, None), "v0");
        assert_eq!(disambiguated_major_tag(0, Some(1)), "v0_m1");
        assert_eq!(
            doc_lines(" * First\r\n * Second\0line\u{2028} * Third\n"),
            vec!["First", "Secondline", "Third"]
        );
    }
}
