//! Stable structured diagnostics.
//!
//! Commands keep their human prose output by default. `--format json` and `--format sarif`
//! replace only the diagnostic stream with a machine-readable envelope; command-specific report
//! output such as `diff` remains governed by [`diff`](crate::diff).
//!
//! A diagnostic carries a stable code, a severity, a human message, and, when the source of the
//! message identifies one, a path and a one-based source position. WIT subset violations carry
//! their logical contract path (for example `ns:pkg/interface/type`) because the owned model
//! deliberately does not retain `wit-parser` spans; generated-output and lockfile diagnostics
//! carry filesystem or member paths.

use std::fmt;

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

/// A one-based source position. Columns are counted in Unicode scalar values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// The first line of the range.
    pub line: usize,
    /// The first column of the range.
    pub column: usize,
    /// The last line of the range.
    pub end_line: usize,
    /// The last column of the range.
    pub end_column: usize,
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
