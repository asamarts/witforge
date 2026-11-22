//! Stable structured diagnostics.
//!
//! Commands keep their human prose output by default. `--format json` and `--format sarif`
//! replace only the diagnostic stream with a machine-readable envelope; command-specific report
//! output such as `diff` remains governed by [`diff`](crate::diff).
//!
//! A diagnostic carries a stable code, a severity, a human message, and, when available, a path
//! and a one-based UTF-16 source position. [`LocatedDiagnostic`] separates the physical
//! filesystem location (`file`) from the logical WIT contract path (`logical`) and carries
//! secondary [`RelatedLocation`] entries (such as a `wire.lock` line alongside a WIT declaration).

use std::fmt;

/// Stable diagnostic code constants emitted by witforge.
pub mod codes {
    /// Unrecognised WIT construct outside the supported subset (legacy umbrella code).
    pub const SUBSET_UNSUPPORTED: &str = "WF1001";
    /// A lockfile belongs to another package.
    pub const LOCKFILE_WRONG_PACKAGE: &str = "WF1002";
    /// Two names in one scope collide (ASCII letter case or generated identifier/reserved word).
    pub const IDENTIFIER_COLLISION: &str = "WF1003";
    /// Root contract package version is missing or not `<major>.<minor>.0`.
    pub const SUBSET_PACKAGE_VERSION: &str = "WF1010";
    /// Nested `package` block inside a version directory; dependencies must live under `deps/`.
    pub const SUBSET_NESTED_PACKAGE: &str = "WF1011";
    /// Dependency package under `deps/` has a missing or unsupported version.
    pub const SUBSET_DEPENDENCY_VERSION: &str = "WF1012";
    /// Duplicate dependency package identity or dependency sharing the root package identity.
    pub const SUBSET_DEPENDENCY_IDENTITY: &str = "WF1013";
    /// Unsupported WIT annotation (such as `@external-id`).
    pub const SUBSET_ANNOTATION: &str = "WF1014";
    /// Resources and handles (`resource`, `borrow<T>`, `own<T>`) are not supported.
    pub const SUBSET_RESOURCE: &str = "WF1015";
    /// `future` and `stream` types are not supported.
    pub const SUBSET_FUTURE_OR_STREAM: &str = "WF1016";
    /// `char` primitive is not supported; use `string`.
    pub const SUBSET_CHAR: &str = "WF1017";
    /// `error-context` is not supported.
    pub const SUBSET_ERROR_CONTEXT: &str = "WF1018";
    /// Tuples are not supported; use a named record.
    pub const SUBSET_TUPLE: &str = "WF1019";
    /// `map` is not supported; use `list<record>`.
    pub const SUBSET_MAP: &str = "WF1020";
    /// Fixed-length lists are not supported; use `list`.
    pub const SUBSET_FIXED_LENGTH_LIST: &str = "WF1021";
    /// Empty records are not supported.
    pub const SUBSET_EMPTY_RECORD: &str = "WF1022";
    /// `async` functions are not supported.
    pub const SUBSET_ASYNC_FUNCTION: &str = "WF1023";
    /// `get` and `set` accessors are not supported.
    pub const SUBSET_ACCESSOR: &str = "WF1024";
    /// Inline interfaces in a world are not supported.
    pub const SUBSET_INLINE_INTERFACE: &str = "WF1025";
    /// Named (renamed) world interface imports or exports are not supported.
    pub const SUBSET_NAMED_WORLD_INTERFACE: &str = "WF1026";
    /// Functions defined directly in a world are not supported.
    pub const SUBSET_WORLD_FUNCTION: &str = "WF1027";
    /// Types defined directly in a world are not supported.
    pub const SUBSET_WORLD_TYPE: &str = "WF1028";
    /// World imports or exports an interface with no package or from outside `deps/`.
    pub const SUBSET_WORLD_FOREIGN_INTERFACE: &str = "WF1029";
    /// Type nesting, alias chain, canonical type length, wrapper name length, or cycle limit.
    pub const SUBSET_TYPE_LIMIT: &str = "WF1030";
    /// Version directory name (`v<major>.<minor>`) disagrees with its declared package version.
    pub const DIRECTORY_VERSION_MISMATCH: &str = "WF1031";
    /// Versions in a WIT root or diff endpoints declare different package identities.
    pub const PACKAGE_MISMATCH: &str = "WF1032";
    /// Pinned dependency graphs differ across versions, diff endpoints, or `--against` baseline.
    pub const DEPENDENCY_GRAPH_MISMATCH: &str = "WF1033";

    /// Lockfile continuity, drift evaluation, order-section, or baseline-policy violation.
    pub const LOCKFILE_ERROR: &str = "WF1100";
    /// A live member has no wire number.
    pub const LOCKFILE_MISSING_NUMBER: &str = "WF1101";
    /// A removed member is not reserved in the current lockfile.
    pub const LOCKFILE_UNRESERVED_REMOVAL: &str = "WF1102";
    /// A removed member was reintroduced under its reserved name/number.
    pub const LOCKFILE_REINTRODUCED: &str = "WF1103";
    /// A stale draft entry remains in the lockfile.
    pub const LOCKFILE_STALE_DRAFT: &str = "WF1104";
    /// A draft member changed kind or canonical type.
    pub const LOCKFILE_CHANGED_DRAFT: &str = "WF1105";
    /// The lockfile is not in canonical `lock` form.
    pub const LOCKFILE_NON_CANONICAL: &str = "WF1106";
    /// The pinned dependency graph cannot be compared because the baseline is not a version dir.
    pub const LOCKFILE_BASELINE_NO_DEPS: &str = "WF1107";
    /// A draft removal can be restored; `lock --against` would un-reserve the member.
    pub const LOCKFILE_RESTORABLE_DRAFT: &str = "WF1108";

    /// `lock` added a member.
    pub const LOCK_ADDED: &str = "WF1201";
    /// `lock` reserved a removed member.
    pub const LOCK_RESERVED: &str = "WF1202";
    /// `lock` dropped a stale draft.
    pub const LOCK_DROPPED: &str = "WF1203";
    /// `lock` updated a draft's kind or type.
    pub const LOCK_UPDATED: &str = "WF1204";
    /// `lock` restored a draft removal.
    pub const LOCK_RESTORED: &str = "WF1205";

    /// A generated file is missing, changed, extra, unowned, or otherwise refuses `--check`.
    pub const GENERATED_DRIFT: &str = "WF2001";
    /// Version-to-version Rust conversion generation refused an incompatible evolution.
    pub const CONVERSION_REFUSED: &str = "WF2002";

    /// `diff` found a wire-, source-, or generated-API-breaking change.
    pub const DIFF_BREAKING: &str = "WF3001";
    /// `diff` found a compatible change.
    pub const DIFF_COMPATIBLE: &str = "WF3002";
    /// `diff` found a record whose generated new-to-old conversion is guaranteed to fail.
    pub const DIFF_CONVERSION_IMPACT: &str = "WF3003";
    /// `diff` found a required record field addition or retyped member blocking conversion gen.
    pub const DIFF_CONVERSION_BLOCKER: &str = "WF3004";

    /// Internal compiler error (unexpected invariant failure; never usage/I/O/parse/guard).
    pub const INTERNAL_ERROR: &str = "WF9000";
    /// Command-line usage or argument error.
    pub const CLI_USAGE: &str = "WF9001";
    /// Filesystem or I/O error (missing file/lockfile, permission denied, unreadable path).
    pub const IO_ERROR: &str = "WF9002";
    /// Parse or format error (invalid WIT, `wire.lock`, `released` manifest, or `witforge.toml`).
    pub const PARSE_ERROR: &str = "WF9003";
    /// Safety guard or freeze-policy refusal (released version, symlink, output overlap, layout).
    pub const GUARD_REFUSED: &str = "WF9004";
}

/// Catalogue metadata for one stable diagnostic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeInfo {
    /// The stable diagnostic code (for example `WF1015`).
    pub code: &'static str,
    /// The default severity emitted for this code.
    pub severity: Severity,
    /// Short human-readable summary of the condition.
    pub summary: &'static str,
    /// Prior code before a split (for example `Some("WF1001")`), if any.
    pub supersedes: Option<&'static str>,
}

/// The complete sorted catalogue of every diagnostic code witforge can emit.
pub const CATALOG: &[CodeInfo] = &[
    CodeInfo {
        code: codes::SUBSET_UNSUPPORTED,
        severity: Severity::Error,
        summary: "Unrecognised WIT construct outside the supported subset (legacy umbrella code).",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCKFILE_WRONG_PACKAGE,
        severity: Severity::Error,
        summary: "A lockfile belongs to another package.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::IDENTIFIER_COLLISION,
        severity: Severity::Error,
        summary: "Two names in one scope collide (ASCII letter case or generated identifier/reserved word).",
        supersedes: None,
    },
    CodeInfo {
        code: codes::SUBSET_PACKAGE_VERSION,
        severity: Severity::Error,
        summary: "Root contract package version is missing or not `<major>.<minor>.0`.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_NESTED_PACKAGE,
        severity: Severity::Error,
        summary: "Nested `package` block inside a version directory; dependencies must live under `deps/`.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_DEPENDENCY_VERSION,
        severity: Severity::Error,
        summary: "Dependency package under `deps/` has a missing or unsupported version.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_DEPENDENCY_IDENTITY,
        severity: Severity::Error,
        summary: "Duplicate dependency package identity or dependency sharing the root package identity.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_ANNOTATION,
        severity: Severity::Error,
        summary: "Unsupported WIT annotation (only `@since`, `@unstable`, and `@deprecated` are accepted).",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_RESOURCE,
        severity: Severity::Error,
        summary: "Resources and handles are not supported.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_FUTURE_OR_STREAM,
        severity: Severity::Error,
        summary: "`future` and `stream` types are not supported.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_CHAR,
        severity: Severity::Error,
        summary: "`char` is not supported; use `string`.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_ERROR_CONTEXT,
        severity: Severity::Error,
        summary: "`error-context` is not supported.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_TUPLE,
        severity: Severity::Error,
        summary: "Tuples are not supported; use a named record.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_MAP,
        severity: Severity::Error,
        summary: "`map` is not supported; use `list<record>`.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_FIXED_LENGTH_LIST,
        severity: Severity::Error,
        summary: "Fixed-length lists are not supported; use `list`.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_EMPTY_RECORD,
        severity: Severity::Error,
        summary: "Empty records are not supported.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_ASYNC_FUNCTION,
        severity: Severity::Error,
        summary: "`async` functions are not supported.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_ACCESSOR,
        severity: Severity::Error,
        summary: "`get` and `set` accessors are not supported.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_INLINE_INTERFACE,
        severity: Severity::Error,
        summary: "Inline interfaces in a world are not supported.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_NAMED_WORLD_INTERFACE,
        severity: Severity::Error,
        summary: "Named (renamed) world interface imports or exports are not supported.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_WORLD_FUNCTION,
        severity: Severity::Error,
        summary: "Functions defined directly in a world are not supported.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_WORLD_TYPE,
        severity: Severity::Error,
        summary: "Types defined directly in a world are not supported.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_WORLD_FOREIGN_INTERFACE,
        severity: Severity::Error,
        summary: "World imports or exports an interface with no package or outside `deps/`.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::SUBSET_TYPE_LIMIT,
        severity: Severity::Error,
        summary: "Type nesting, alias chain, canonical length, wrapper length, or cycle limit exceeded.",
        supersedes: Some(codes::SUBSET_UNSUPPORTED),
    },
    CodeInfo {
        code: codes::DIRECTORY_VERSION_MISMATCH,
        severity: Severity::Error,
        summary: "Version directory name disagrees with its declared package version.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::PACKAGE_MISMATCH,
        severity: Severity::Error,
        summary: "Versions in a WIT root or diff endpoints declare different package identities.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::DEPENDENCY_GRAPH_MISMATCH,
        severity: Severity::Error,
        summary: "Pinned dependency graphs differ across versions, diff endpoints, or baseline.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCKFILE_ERROR,
        severity: Severity::Error,
        summary: "Lockfile continuity, drift evaluation, order-section, or baseline-policy violation.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCKFILE_MISSING_NUMBER,
        severity: Severity::Error,
        summary: "A live member has no wire number.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCKFILE_UNRESERVED_REMOVAL,
        severity: Severity::Error,
        summary: "A removed member is not reserved in the current lockfile.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCKFILE_REINTRODUCED,
        severity: Severity::Error,
        summary: "A removed member was reintroduced under its reserved name/number.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCKFILE_STALE_DRAFT,
        severity: Severity::Error,
        summary: "A stale draft entry remains in the lockfile.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCKFILE_CHANGED_DRAFT,
        severity: Severity::Error,
        summary: "A draft member changed kind or canonical type.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCKFILE_NON_CANONICAL,
        severity: Severity::Error,
        summary: "The lockfile is not in canonical `lock` form.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCKFILE_BASELINE_NO_DEPS,
        severity: Severity::Note,
        summary: "Pinned dependency graph cannot be compared because the baseline is not a version directory.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCKFILE_RESTORABLE_DRAFT,
        severity: Severity::Error,
        summary: "A draft removal can be restored; `lock --against` would un-reserve the member.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCK_ADDED,
        severity: Severity::Note,
        summary: "`lock` added a member.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCK_RESERVED,
        severity: Severity::Note,
        summary: "`lock` reserved a removed member.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCK_DROPPED,
        severity: Severity::Note,
        summary: "`lock` dropped a stale draft.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCK_UPDATED,
        severity: Severity::Note,
        summary: "`lock` updated a draft's kind or type.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::LOCK_RESTORED,
        severity: Severity::Note,
        summary: "`lock` restored a draft removal.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::GENERATED_DRIFT,
        severity: Severity::Error,
        summary: "A generated file is missing, changed, extra, unowned, or otherwise refuses `--check`.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::CONVERSION_REFUSED,
        severity: Severity::Error,
        summary: "Version-to-version Rust conversion generation refused an incompatible evolution.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::DIFF_BREAKING,
        severity: Severity::Error,
        summary: "`diff` found a wire-, source-, or generated-API-breaking change.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::DIFF_COMPATIBLE,
        severity: Severity::Note,
        summary: "`diff` found a compatible change.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::DIFF_CONVERSION_IMPACT,
        severity: Severity::Note,
        summary: "`diff` found a record whose generated new-to-old conversion is guaranteed to fail.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::DIFF_CONVERSION_BLOCKER,
        severity: Severity::Note,
        summary: "`diff` found a non-defaulted required record field addition or retyped member blocking conversion generation.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::INTERNAL_ERROR,
        severity: Severity::Error,
        summary: "Internal compiler error (unexpected invariant failure).",
        supersedes: None,
    },
    CodeInfo {
        code: codes::CLI_USAGE,
        severity: Severity::Error,
        summary: "Command-line usage or argument error.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::IO_ERROR,
        severity: Severity::Error,
        summary: "Filesystem or I/O error.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::PARSE_ERROR,
        severity: Severity::Error,
        summary: "Parse or format error in WIT, lockfile, released manifest, or configuration.",
        supersedes: None,
    },
    CodeInfo {
        code: codes::GUARD_REFUSED,
        severity: Severity::Error,
        summary: "Safety guard or freeze-policy refusal.",
        supersedes: None,
    },
];

/// Base URL of the public diagnostic code reference page (published by GitHub Pages).
pub const HELP_BASE_URL: &str = "https://asamarts.github.io/witforge/guide/diagnostics.html";

/// Public documentation URL for a stable diagnostic `code`.
#[must_use]
pub fn help_uri(code: &str) -> String {
    format!("{HELP_BASE_URL}#{}", code.to_ascii_lowercase())
}

/// Look up catalogue metadata for a stable diagnostic `code`.
#[must_use]
pub fn lookup(code: &str) -> Option<&'static CodeInfo> {
    CATALOG
        .binary_search_by_key(&code, |info| info.code)
        .ok()
        .map(|index| &CATALOG[index])
}

/// The severity of a structured diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The command cannot produce a valid result.
    Error,
    /// A suspicious condition that does not itself fail the command.
    Warning,
    /// Informational context for a successful command.
    Note,
}

impl Severity {
    /// The stable lowercase token used by JSON and SARIF.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A one-based source position. Columns are counted in UTF-16 code units (matching SARIF 2.1.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// The first line of the range (1-based).
    pub line: usize,
    /// The first column of the range (1-based, in UTF-16 code units).
    pub column: usize,
    /// The last line of the range (1-based).
    pub end_line: usize,
    /// The end column of the range (1-based exclusive, in UTF-16 code units).
    pub end_column: usize,
}

/// A secondary physical source location related to a diagnostic (for example, the `wire.lock`
/// entry corresponding to a WIT contract discrepancy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedLocation {
    /// Physical file path.
    pub file: String,
    /// One-based source range, when known.
    pub position: Option<Position>,
    /// Human explanation of how this location relates to the diagnostic.
    pub message: String,
}

/// A [`Diagnostic`] paired with optional declaring physical file, logical contract/member name,
/// and secondary related locations.
///
/// Kept separate from [`Diagnostic`] so 0.x additions remain minor-SemVer compatible with 0.6.0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedDiagnostic {
    /// The core diagnostic (`code`, `severity`, `message`, `path`, `position`).
    pub diagnostic: Diagnostic,
    /// Declaring physical file path, when known.
    pub file: Option<String>,
    /// Logical contract or member name (for SARIF `logicalLocations`), when known.
    pub logical: Option<String>,
    /// Secondary physical locations (for SARIF `relatedLocations`).
    pub related: Vec<RelatedLocation>,
}

impl LocatedDiagnostic {
    /// Wrap a plain [`Diagnostic`] with no extra physical/logical/related metadata.
    #[must_use]
    pub fn from_diagnostic(diagnostic: Diagnostic) -> Self {
        Self {
            diagnostic,
            file: None,
            logical: None,
            related: Vec::new(),
        }
    }

    /// Attach the declaring physical file path.
    #[must_use]
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    /// Attach a logical contract or member name.
    #[must_use]
    pub fn with_logical(mut self, logical: impl Into<String>) -> Self {
        self.logical = Some(logical.into());
        self
    }

    /// Append a secondary related physical location.
    #[must_use]
    pub fn with_related(mut self, related: RelatedLocation) -> Self {
        self.related.push(related);
        self
    }
}

/// One machine-readable diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// A stable code from the diagnostic catalogue.
    pub code: &'static str,
    /// How serious the condition is.
    pub severity: Severity,
    /// The human-readable message.
    pub message: String,
    /// The contract, source, generated, or lockfile path the message concerns.
    pub path: Option<String>,
    /// A source position when one is available.
    pub position: Option<Position>,
}

impl Diagnostic {
    /// Create a diagnostic with no path or position.
    #[must_use]
    pub fn new(code: &'static str, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            code,
            severity,
            message: message.into(),
            path: None,
            position: None,
        }
    }

    /// Create an error diagnostic with no path or position.
    #[must_use]
    pub fn error(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Error, message)
    }

    /// Add a path.
    #[must_use]
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Add a source position.
    #[must_use]
    pub fn with_position(mut self, position: Position) -> Self {
        self.position = Some(position);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_and_position_are_preserved() {
        let position = Position {
            line: 3,
            column: 5,
            end_line: 3,
            end_column: 9,
        };
        let diagnostic =
            Diagnostic::new("WF0001", Severity::Warning, "suspicious").with_position(position);
        assert_eq!(diagnostic.severity, Severity::Warning);
        assert_eq!(diagnostic.position, Some(position));
    }

    #[test]
    fn catalog_is_sorted_unique_and_searchable() {
        assert_ne!(CATALOG, []);
        for pair in CATALOG.windows(2) {
            assert!(
                pair[0].code < pair[1].code,
                "CATALOG must be strictly sorted: {} >= {}",
                pair[0].code,
                pair[1].code
            );
        }
        for info in CATALOG {
            assert!(
                info.code.starts_with("WF") && info.code.len() == 6,
                "invalid code shape: {}",
                info.code
            );
            assert_eq!(lookup(info.code), Some(info));
            if let Some(old) = info.supersedes {
                assert!(
                    lookup(old).is_some(),
                    "superseded code {old} not in CATALOG"
                );
            }
        }
        assert_eq!(lookup("WF9999"), None);
    }
}
