//! CLI diagnostic rendering and collection.

use std::collections::BTreeSet;
use std::io::{self, Write};

use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use serde_json::{Value, json};

use witforge::diagnostic::{Diagnostic, Severity};

/// Write `text` to stdout and flush, ignoring a closed pipe and returning any other I/O error.
pub(crate) fn write_stdout_str(text: &str) -> Result<bool> {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(false),
        Err(err) => Err(anyhow!("failed to write to stdout: {err}")),
    }
}

/// Write `text` to stderr without panicking if stderr is closed or unwritable.
pub(crate) fn write_stderr_str(text: &str) {
    let mut stderr = io::stderr().lock();
    let _ = stderr.write_all(text.as_bytes());
    let _ = stderr.flush();
}

/// Write one line to stderr without panicking if stderr is closed or unwritable.
pub(crate) fn write_stderr_line(line: &str) {
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "{line}");
    let _ = stderr.flush();
}

/// The diagnostic output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Format {
    /// Human-readable diagnostics on stderr; the default.
    Human,
    /// A versioned JSON diagnostic envelope on stdout.
    Json,
    /// SARIF 2.1.0 on stdout.
    Sarif,
}

pub(crate) struct Reporter {
    format: Format,
    diagnostics: Vec<Diagnostic>,
    suppressed: bool,
    stdout_closed: bool,
    stdout_error: Option<io::Error>,
}

impl Reporter {
    pub(crate) fn new(format: Format) -> Self {
        Self {
            format,
            diagnostics: Vec::new(),
            suppressed: false,
            stdout_closed: false,
            stdout_error: None,
        }
    }

    pub(crate) fn human(&self) -> bool {
        self.format == Format::Human
    }

    pub(crate) fn suppress(&mut self) {
        self.suppressed = true;
    }

    pub(crate) fn is_suppressed(&self) -> bool {
        self.suppressed
    }

    pub(crate) fn write_stdout_line(&mut self, line: &str) {
        if self.stdout_closed || self.stdout_error.is_some() {
            return;
        }
        let mut stdout = io::stdout().lock();
        if let Err(err) = writeln!(stdout, "{line}") {
            if err.kind() == io::ErrorKind::BrokenPipe {
                self.stdout_closed = true;
            } else {
                self.stdout_error = Some(err);
            }
        }
    }

    pub(crate) fn push(&mut self, diagnostic: Diagnostic) {
        if self.human() {
            write_stderr_line(&format!("error: {}", diagnostic.message));
        } else {
            self.diagnostics.push(diagnostic);
        }
    }

    pub(crate) fn error(
        &mut self,
        code: &'static str,
        message: impl Into<String>,
        path: Option<String>,
    ) {
        let mut diagnostic = Diagnostic::error(code, message);
        if let Some(path) = path {
            diagnostic = diagnostic.with_path(path);
        }
        self.push(diagnostic);
    }

    pub(crate) fn error_with_location(
        &mut self,
        code: &'static str,
        location: impl Into<String>,
        message: impl Into<String>,
    ) {
        let location = location.into();
        if self.human() {
            write_stderr_line(&format!("error: {location}: {}", message.into()));
        } else {
            self.diagnostics
                .push(Diagnostic::error(code, message).with_path(location));
        }
    }

    pub(crate) fn note(
        &mut self,
        code: &'static str,
        human: &str,
        message: impl Into<String>,
        path: String,
    ) {
        if self.human() {
            self.write_stdout_line(human);
        } else {
            self.diagnostics
                .push(Diagnostic::new(code, Severity::Note, message).with_path(path));
        }
    }

    pub(crate) fn finish(&mut self) -> Result<()> {
        if !self.suppressed && !self.human() {
            let rendered = match self.format {
                Format::Human => None,
                Format::Json => Some(diagnostic_json(&self.diagnostics)?),
                Format::Sarif => Some(sarif_json(&self.diagnostics)?),
            };
            if let Some(text) = rendered {
                self.write_stdout_line(&text);
            }
        }
        if !self.stdout_closed && self.stdout_error.is_none() {
            let mut stdout = io::stdout().lock();
            if let Err(err) = stdout.flush() {
                if err.kind() == io::ErrorKind::BrokenPipe {
                    self.stdout_closed = true;
                } else {
                    self.stdout_error = Some(err);
                }
            }
        }
        if let Some(err) = self.stdout_error.take() {
            return Err(anyhow!("failed to write to stdout: {err}"));
        }
        Ok(())
    }
}

fn diagnostic_json(diagnostics: &[Diagnostic]) -> Result<String> {
    let values: Vec<Value> = diagnostics.iter().map(diagnostic_value).collect();
    serde_json::to_string_pretty(&json!({
        "version": 1,
        "tool": {
            "name": "witforge",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "diagnostics": values,
    }))
    .context("failed to render diagnostics as JSON")
}

fn diagnostic_value(diagnostic: &Diagnostic) -> Value {
    let mut value = json!({
        "code": diagnostic.code,
        "severity": diagnostic.severity.as_str(),
        "message": &diagnostic.message,
    });
    let object = value.as_object_mut().expect("JSON object");
    if let Some(path) = &diagnostic.path {
        object.insert("path".to_string(), Value::String(path.clone()));
    }
    if let Some(position) = diagnostic.position {
        object.insert("line".to_string(), json!(position.line));
        object.insert("column".to_string(), json!(position.column));
        object.insert("end_line".to_string(), json!(position.end_line));
        object.insert("end_column".to_string(), json!(position.end_column));
    }
    value
}

fn sarif_json(diagnostics: &[Diagnostic]) -> Result<String> {
    let mut rules = BTreeSet::new();
    for diagnostic in diagnostics {
        rules.insert(diagnostic.code);
    }
    let rules: Vec<Value> = rules
        .into_iter()
        .map(|code| {
            let description = witforge::diagnostic::lookup(code).map_or(code, |info| info.summary);
            json!({
                "id": code,
                "shortDescription": { "text": description },
            })
        })
        .collect();
    let results: Vec<Value> = diagnostics.iter().map(sarif_result).collect();
    serde_json::to_string_pretty(&json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "witforge",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/asamarts/witforge",
                    "rules": rules,
                }
            },
            "results": results,
        }],
    }))
    .context("failed to render diagnostics as SARIF")
}

fn sarif_result(diagnostic: &Diagnostic) -> Value {
    let mut result = json!({
        "ruleId": diagnostic.code,
        "level": diagnostic.severity.as_str(),
        "message": { "text": &diagnostic.message },
    });
    if let Some(path) = &diagnostic.path {
        let mut physical = json!({
            "artifactLocation": { "uri": path.replace('\\', "/") },
        });
        if let (Some(position), Some(object)) = (diagnostic.position, physical.as_object_mut()) {
            object.insert(
                "region".to_string(),
                json!({
                    "startLine": position.line,
                    "startColumn": position.column,
                    "endLine": position.end_line,
                    "endColumn": position.end_column,
                }),
            );
        }
        result["locations"] = json!([{ "physicalLocation": physical }]);
    }
    result
}

pub(crate) fn diagnostic_path(message: &str) -> Option<String> {
    let start = message.find('`')?;
    let rest = &message[start + 1..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}
