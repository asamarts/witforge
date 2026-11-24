//! CLI diagnostic rendering and collection.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use serde_json::{Value, json};

use witforge::diagnostic::{Diagnostic, LocatedDiagnostic, Position, Severity, help_uri};

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
    quiet: bool,
    diagnostics: Vec<LocatedDiagnostic>,
    suppressed: bool,
    stdout_closed: bool,
    stdout_error: Option<io::Error>,
}

impl Reporter {
    pub(crate) fn new(format: Format, quiet: bool) -> Self {
        Self {
            format,
            quiet,
            diagnostics: Vec::new(),
            suppressed: false,
            stdout_closed: false,
            stdout_error: None,
        }
    }

    pub(crate) fn human(&self) -> bool {
        self.format == Format::Human
    }

    pub(crate) fn is_quiet(&self) -> bool {
        self.quiet
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
        self.push_located(LocatedDiagnostic::from_diagnostic(diagnostic));
    }

    pub(crate) fn push_located(&mut self, located: LocatedDiagnostic) {
        if self.human() {
            write_stderr_line(&format!("error: {}", located.diagnostic.message));
        } else {
            self.diagnostics.push(located);
        }
    }

    pub(crate) fn note_located(&mut self, human: &str, located: LocatedDiagnostic) {
        if self.human() {
            if !self.quiet {
                self.write_stdout_line(human);
            }
        } else {
            self.diagnostics.push(located);
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

/// Normalize a physical filesystem path for JSON `file` fields and workflow annotations:
/// cwd-relative with forward slashes when inside the current working directory, otherwise a
/// forward-slash cleaned absolute path (never containing `..` or Windows `\\?\` prefixes).
pub(crate) fn normalize_file_path(raw: &str) -> String {
    match resolve_cwd_relative(raw) {
        NormalizedPath::Relative(rel) => rel,
        NormalizedPath::Absolute(abs) => abs,
    }
}

/// Normalize a physical filesystem path for SARIF `artifactLocation.uri`:
/// cwd-relative percent-encoded URI with forward slashes when inside the current working
/// directory, otherwise an absolute percent-encoded `file:///` URI (never `..` and never a
/// logical identifier).
pub(crate) fn sarif_artifact_uri(raw: &str) -> String {
    match resolve_cwd_relative(raw) {
        NormalizedPath::Relative(rel) => {
            let mut out = String::with_capacity(rel.len());
            encode_relative_uri_path(&mut out, &rel);
            out
        }
        NormalizedPath::Absolute(abs) => absolute_path_to_file_uri(&abs),
    }
}

enum NormalizedPath {
    Relative(String),
    Absolute(String),
}

fn resolve_cwd_relative(raw: &str) -> NormalizedPath {
    let input = Path::new(raw);
    let cwd = std::env::current_dir().ok();
    let cwd_canon = cwd.as_deref().and_then(|dir| fs::canonicalize(dir).ok());

    // Fast path for a relative path that lexically stays inside cwd without `..`.
    if !input.is_absolute() {
        let cleaned = clean_lexical(input);
        if !cleaned
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return NormalizedPath::Relative(path_to_forward_slashes(&cleaned));
        }
    }

    let abs_lexical = if input.is_absolute() {
        clean_lexical(input)
    } else if let Some(ref cwd) = cwd {
        clean_lexical(&cwd.join(input))
    } else {
        clean_lexical(input)
    };

    if let Some(ref cwd) = cwd
        && let Ok(rel) = abs_lexical.strip_prefix(cwd)
    {
        return NormalizedPath::Relative(path_to_forward_slashes(rel));
    }

    let abs_canon = canonicalize_best_effort(&abs_lexical);
    if let Some(ref cwd_canon) = cwd_canon
        && let Ok(rel) = abs_canon.strip_prefix(cwd_canon)
    {
        return NormalizedPath::Relative(path_to_forward_slashes(rel));
    }

    NormalizedPath::Absolute(strip_verbatim_and_slashes(&abs_canon))
}

fn clean_lexical(path: &Path) -> PathBuf {
    let mut parts: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match parts.last() {
                Some(Component::Normal(_)) => {
                    parts.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => parts.push(component),
            },
            _ => parts.push(component),
        }
    }
    if parts.is_empty() {
        PathBuf::from(".")
    } else {
        parts.into_iter().collect()
    }
}

fn canonicalize_best_effort(path: &Path) -> PathBuf {
    if let Ok(canon) = fs::canonicalize(path) {
        return canon;
    }
    let mut tail = Vec::new();
    let mut cursor = path;
    while let Some(name) = cursor.file_name() {
        tail.push(name.to_os_string());
        let Some(parent) = cursor.parent() else {
            break;
        };
        if let Ok(mut canon) = fs::canonicalize(parent) {
            for component in tail.into_iter().rev() {
                canon.push(component);
            }
            return canon;
        }
        cursor = parent;
    }
    path.to_path_buf()
}

fn path_to_forward_slashes(path: &Path) -> String {
    let raw = strip_verbatim_and_slashes(path);
    if raw.is_empty() { ".".to_string() } else { raw }
}

fn strip_verbatim_and_slashes(path: &Path) -> String {
    let text = path.to_string_lossy();
    let stripped = if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!("//{unc}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        text.into_owned()
    };
    stripped.replace('\\', "/")
}

fn absolute_path_to_file_uri(abs_forward: &str) -> String {
    let mut out = String::from("file://");
    if let Some(unc) = abs_forward.strip_prefix("//") {
        encode_uri_path(&mut out, unc);
    } else if abs_forward.starts_with('/') {
        encode_uri_path(&mut out, abs_forward);
    } else {
        // Windows drive path like `C:/path/to/file`.
        out.push('/');
        encode_uri_path(&mut out, abs_forward);
    }
    out
}

fn encode_uri_path(out: &mut String, path: &str) {
    for &byte in path.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~' | b':') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
}

fn encode_relative_uri_path(out: &mut String, path: &str) {
    for &byte in path.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
}

fn diagnostic_json(diagnostics: &[LocatedDiagnostic]) -> Result<String> {
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

fn diagnostic_value(located: &LocatedDiagnostic) -> Value {
    let diagnostic = &located.diagnostic;
    let mut value = json!({
        "code": diagnostic.code,
        "severity": diagnostic.severity.as_str(),
        "message": &diagnostic.message,
    });
    let object = value.as_object_mut().expect("JSON object");
    if let Some(path) = &diagnostic.path {
        object.insert("path".to_string(), Value::String(path.clone()));
    }
    if let Some(file) = &located.file {
        object.insert("file".to_string(), Value::String(normalize_file_path(file)));
    }
    if let Some(logical) = &located.logical {
        object.insert("logical".to_string(), Value::String(logical.clone()));
    }
    if let Some(position) = diagnostic.position {
        insert_position_fields(object, position);
    }
    if !located.related.is_empty() {
        let related: Vec<Value> = located
            .related
            .iter()
            .map(|rel| {
                let mut rel_val = json!({
                    "file": normalize_file_path(&rel.file),
                    "message": &rel.message,
                });
                if let (Some(pos), Some(rel_obj)) = (rel.position, rel_val.as_object_mut()) {
                    insert_position_fields(rel_obj, pos);
                }
                rel_val
            })
            .collect();
        object.insert("related".to_string(), Value::Array(related));
    }
    value
}

fn insert_position_fields(object: &mut serde_json::Map<String, Value>, position: Position) {
    object.insert("line".to_string(), json!(position.line));
    object.insert("column".to_string(), json!(position.column));
    object.insert("end_line".to_string(), json!(position.end_line));
    object.insert("end_column".to_string(), json!(position.end_column));
}

fn sarif_json(diagnostics: &[LocatedDiagnostic]) -> Result<String> {
    let mut rules = BTreeSet::new();
    for located in diagnostics {
        rules.insert(located.diagnostic.code);
    }
    let rules: Vec<Value> = rules
        .into_iter()
        .map(|code| {
            let description = witforge::diagnostic::lookup(code).map_or(code, |info| info.summary);
            json!({
                "id": code,
                "shortDescription": { "text": description },
                "helpUri": help_uri(code),
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

fn sarif_region(position: Position) -> Value {
    json!({
        "startLine": position.line,
        "startColumn": position.column,
        "endLine": position.end_line,
        "endColumn": position.end_column,
    })
}

fn sarif_result(located: &LocatedDiagnostic) -> Value {
    let diagnostic = &located.diagnostic;
    let mut result = json!({
        "ruleId": diagnostic.code,
        "level": diagnostic.severity.as_str(),
        "message": { "text": &diagnostic.message },
    });
    let mut location_obj = serde_json::Map::new();
    if let Some(file) = &located.file {
        let mut physical = json!({
            "artifactLocation": { "uri": sarif_artifact_uri(file) },
        });
        if let (Some(position), Some(object)) = (diagnostic.position, physical.as_object_mut()) {
            object.insert("region".to_string(), sarif_region(position));
        }
        location_obj.insert("physicalLocation".to_string(), physical);
    }
    if let Some(logical) = &located.logical {
        location_obj.insert(
            "logicalLocations".to_string(),
            json!([{ "fullyQualifiedName": logical }]),
        );
    }
    if !location_obj.is_empty() {
        result["locations"] = Value::Array(vec![Value::Object(location_obj)]);
    }
    if !located.related.is_empty() {
        let related: Vec<Value> = located
            .related
            .iter()
            .enumerate()
            .map(|(index, rel)| {
                let mut physical = json!({
                    "artifactLocation": { "uri": sarif_artifact_uri(&rel.file) },
                });
                if let (Some(pos), Some(obj)) = (rel.position, physical.as_object_mut()) {
                    obj.insert("region".to_string(), sarif_region(pos));
                }
                json!({
                    "id": index + 1,
                    "physicalLocation": physical,
                    "message": { "text": &rel.message },
                })
            })
            .collect();
        result["relatedLocations"] = Value::Array(related);
    }
    result
}

/// Build a [`LocatedDiagnostic`] for a note carrying a logical contract path and optional
/// physical source file/position.
pub(crate) fn located_note(
    code: &'static str,
    message: impl Into<String>,
    logical: String,
    physical: Option<&(String, Position)>,
    related: Option<(String, Option<Position>, &'static str)>,
) -> LocatedDiagnostic {
    let mut diagnostic = Diagnostic::new(code, Severity::Note, message).with_path(logical.clone());
    if let Some((_, pos)) = physical {
        diagnostic = diagnostic.with_position(*pos);
    }
    let mut located = LocatedDiagnostic::from_diagnostic(diagnostic).with_logical(logical);
    if let Some((file, _)) = physical {
        located = located.with_file(file.clone());
    }
    if let Some((file, position, rel_msg)) = related {
        located = located.with_related(witforge::diagnostic::RelatedLocation::new(
            file,
            position,
            rel_msg.to_string(),
        ));
    }
    located
}
