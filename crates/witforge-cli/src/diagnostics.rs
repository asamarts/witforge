//! CLI diagnostic rendering and collection.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use clap::ValueEnum;
use serde_json::{Value, json};

use witforge::diagnostic::{Diagnostic, Severity};

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
}

impl Reporter {
    pub(crate) fn new(format: Format) -> Self {
        Self {
            format,
            diagnostics: Vec::new(),
            suppressed: false,
        }
    }

    pub(crate) fn human(&self) -> bool {
        self.format == Format::Human
    }

    pub(crate) fn suppress(&mut self) {
        self.suppressed = true;
    }

    pub(crate) fn push(&mut self, diagnostic: Diagnostic) {
        if self.human() {
            eprintln!("error: {}", diagnostic.message);
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
            eprintln!("error: {location}: {}", message.into());
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
            println!("{human}");
        } else {
            self.diagnostics
                .push(Diagnostic::new(code, Severity::Note, message).with_path(path));
        }
    }

    pub(crate) fn finish(&self) -> Result<()> {
        if self.suppressed || self.human() {
            return Ok(());
        }
        match self.format {
            Format::Human => Ok(()),
            Format::Json => {
                println!("{}", diagnostic_json(&self.diagnostics)?);
                Ok(())
            }
            Format::Sarif => {
                println!("{}", sarif_json(&self.diagnostics)?);
                Ok(())
            }
        }
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
            json!({
                "id": code,
                "shortDescription": { "text": code },
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
