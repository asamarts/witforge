#![deny(missing_docs)]

//! witforge: a WIT-first contract compiler.
//!
//! A WIT package is the single source of truth for a contract. witforge checks that the package
//! stays inside a restricted, pure subset of WIT that maps cleanly onto every target, keeps a
//! committed, append-only lockfile of wire numbers so the protobuf projection stays stable across
//! versions, and emits plain Rust types and traits (including opt-in Serde derives and
//! `no_std`/`alloc` mode), version-to-version adapters, a `.proto` schema, JSON Schema (Draft 7 by
//! default, opt-in Draft 2020-12), versioned JSON contract IR, and the glue that converts between
//! the plain and protobuf types.
//!
//! - [`load`] parses a version directory's WIT package, [`load_sources`] parses the same layout
//!   from named in-memory sources, and [`subset`] checks either against the supported subset.
//! - [`model`] is the owned contract model the lockfile and the emitters consume.
//! - [`lock`] reads, reconciles, and writes the `wire.lock` lockfile, and
//!   [`ReleasedManifest`] reads the `released` manifest of frozen versions.
//! - [`emit`] generates the [Rust](emit::rust) output (including version-to-version
//!   `convert.rs`), [protobuf](emit::proto), [JSON Schema](emit::json_schema), and
//!   [glue](emit::glue) output for a WIT root and checks committed output for drift.
//! - [`diff`] compares two contract versions across wire, source, generated-API, and protobuf
//!   binary decode compatibility views.
//! - [`inspect`] exports a versioned machine-readable JSON contract IR and human summaries.
//! - [`freeze`] checks release-freeze immutability and retirement ordering against Git history.
//! - [`diagnostic`] defines the stable machine-readable diagnostics shared by the CLI commands.
//! - [`workflow`] owns side-effect-free check, release-readiness, and lock orchestration and
//!   returns structured diagnostics; lockfile writing is explicit.
//! - [`ContractError`] marks contract, lockfile, and output violations, as distinct from usage,
//!   I/O, and parse errors.
//!
//! The `witforge` command-line interface (the `witforge-cli` crate) wraps these as the `check`,
//! `lock`, `rust`, `proto`, `schema`, `glue`, `generate`, `init`, `new-version`, `release`,
//! `retire`, `diff`, `inspect`, `freeze`, `completions`, and `explain` commands. For Rust
//! `build.rs` integration, see the companion `witforge-build` crate. The normative specifications
//! are in [`docs/design`](https://github.com/asamarts/witforge/tree/main/docs/design).

pub mod diagnostic;
pub mod diff;
pub mod emit;
pub mod freeze;
pub mod inspect;
pub mod lock;
pub mod model;
pub mod subset;
pub mod workflow;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use wit_parser::{
    AstItem, InterfaceId, PackageId, PackageName, Resolve, SourceMap, UnresolvedPackage,
    UnresolvedPackageGroup, WorldId, WorldKey,
};

/// A contract, lockfile, or generated-output violation, as opposed to a usage, I/O, parse, or
/// internal error.
///
/// The generators ([`emit::rust`], [`emit::proto`], [`emit::json_schema`], and [`emit::glue`])
/// and [`diff`] report a contract or lockfile violation as this type inside the returned
/// [`anyhow::Error`], so callers can tell the two classes apart with [`is_contract_error`]; the
/// `witforge` CLI does the same for `check` and `lock`, and exits with `1` for a violation and
/// `2` for any other error. The lower-level
/// [`lock::WireLock`] methods return plain errors, which their callers classify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractError {
    message: String,
    diagnostics: Vec<diagnostic::Diagnostic>,
    located: Vec<diagnostic::LocatedDiagnostic>,
}

impl ContractError {
    /// A violation described by `message`.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            diagnostics: Vec::new(),
            located: Vec::new(),
        }
    }

    /// A violation carrying a single structured diagnostic whose message is also used for display.
    #[must_use]
    pub fn with_diagnostic(diagnostic: diagnostic::Diagnostic) -> Self {
        let message = diagnostic.message.clone();
        let located = vec![diagnostic::LocatedDiagnostic::from_diagnostic(
            diagnostic.clone(),
        )];
        Self {
            message,
            diagnostics: vec![diagnostic],
            located,
        }
    }

    /// A violation carrying a single located diagnostic whose message is also used for display.
    #[must_use]
    pub fn with_located_diagnostic(located: diagnostic::LocatedDiagnostic) -> Self {
        let message = located.diagnostic.message.clone();
        let diagnostics = vec![located.diagnostic.clone()];
        Self {
            message,
            diagnostics,
            located: vec![located],
        }
    }

    /// A violation carrying a human `message` together with one or more structured diagnostics.
    #[must_use]
    pub fn with_diagnostics(
        message: impl Into<String>,
        diagnostics: Vec<diagnostic::Diagnostic>,
    ) -> Self {
        let located = diagnostics
            .iter()
            .cloned()
            .map(diagnostic::LocatedDiagnostic::from_diagnostic)
            .collect();
        Self {
            message: message.into(),
            diagnostics,
            located,
        }
    }

    /// A violation carrying a human `message` together with one or more located diagnostics.
    #[must_use]
    pub fn with_located_diagnostics(
        message: impl Into<String>,
        located: Vec<diagnostic::LocatedDiagnostic>,
    ) -> Self {
        let diagnostics = located.iter().map(|item| item.diagnostic.clone()).collect();
        Self {
            message: message.into(),
            diagnostics,
            located,
        }
    }

    /// Classify `error` as a violation, keeping its whole context chain in the message and any
    /// attached structured diagnostics.
    #[must_use]
    pub fn from_error(error: &anyhow::Error) -> Self {
        let located = error_located_diagnostics(error)
            .map_or_else(Vec::new, <[diagnostic::LocatedDiagnostic]>::to_vec);
        let diagnostics = if located.is_empty() {
            error_diagnostics(error).map_or_else(Vec::new, <[diagnostic::Diagnostic]>::to_vec)
        } else {
            located.iter().map(|item| item.diagnostic.clone()).collect()
        };
        Self {
            message: format!("{error:#}"),
            diagnostics,
            located,
        }
    }

    /// Structured diagnostics attached to this violation, if any.
    #[must_use]
    pub fn diagnostics(&self) -> &[diagnostic::Diagnostic] {
        &self.diagnostics
    }

    /// Structured diagnostics with physical/logical source locations attached to this violation.
    #[must_use]
    pub fn located_diagnostics(&self) -> &[diagnostic::LocatedDiagnostic] {
        &self.located
    }
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ContractError {}

/// A non-contract operational error (usage, I/O, parse, or safety guard) carrying structured
/// diagnostics.
///
/// Unlike [`ContractError`] (which the CLI reports with exit `1`), a `DiagnosticError` is not a
/// contract violation: the CLI reports it with exit `2` while preserving its specific diagnostic
/// code and path instead of falling back to `WF9000`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticError {
    message: String,
    diagnostics: Vec<diagnostic::Diagnostic>,
    located: Vec<diagnostic::LocatedDiagnostic>,
}

impl DiagnosticError {
    /// Create an operational error with a human `message` and one or more structured diagnostics.
    #[must_use]
    pub fn new(message: impl Into<String>, diagnostics: Vec<diagnostic::Diagnostic>) -> Self {
        let located = diagnostics
            .iter()
            .cloned()
            .map(diagnostic::LocatedDiagnostic::from_diagnostic)
            .collect();
        Self {
            message: message.into(),
            diagnostics,
            located,
        }
    }

    /// Create an operational error carrying one structured diagnostic.
    #[must_use]
    pub fn single(diagnostic: diagnostic::Diagnostic) -> Self {
        let message = diagnostic.message.clone();
        let located = vec![diagnostic::LocatedDiagnostic::from_diagnostic(
            diagnostic.clone(),
        )];
        Self {
            message,
            diagnostics: vec![diagnostic],
            located,
        }
    }

    /// Create an operational error with a human `message` and located diagnostics.
    #[must_use]
    pub fn with_located(
        message: impl Into<String>,
        located: Vec<diagnostic::LocatedDiagnostic>,
    ) -> Self {
        let diagnostics = located.iter().map(|item| item.diagnostic.clone()).collect();
        Self {
            message: message.into(),
            diagnostics,
            located,
        }
    }

    /// Create an operational error carrying one located diagnostic.
    #[must_use]
    pub fn single_located(located: diagnostic::LocatedDiagnostic) -> Self {
        let message = located.diagnostic.message.clone();
        let diagnostics = vec![located.diagnostic.clone()];
        Self {
            message,
            diagnostics,
            located: vec![located],
        }
    }

    /// Structured diagnostics attached to this error.
    #[must_use]
    pub fn diagnostics(&self) -> &[diagnostic::Diagnostic] {
        &self.diagnostics
    }

    /// Structured diagnostics with physical/logical locations attached to this error.
    #[must_use]
    pub fn located_diagnostics(&self) -> &[diagnostic::LocatedDiagnostic] {
        &self.located
    }
}

impl fmt::Display for DiagnosticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DiagnosticError {}

/// Whether `error`, or any error in its context chain, is a [`ContractError`].
#[must_use]
pub fn is_contract_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(<dyn std::error::Error>::is::<ContractError>)
}

/// Extract structured diagnostics attached to `error` (from a [`ContractError`] or
/// [`DiagnosticError`] in its context chain), if any.
#[must_use]
pub fn error_diagnostics(error: &anyhow::Error) -> Option<&[diagnostic::Diagnostic]> {
    for cause in error.chain() {
        if let Some(contract) = cause.downcast_ref::<ContractError>()
            && !contract.diagnostics.is_empty()
        {
            return Some(&contract.diagnostics);
        }
        if let Some(operational) = cause.downcast_ref::<DiagnosticError>()
            && !operational.diagnostics.is_empty()
        {
            return Some(&operational.diagnostics);
        }
    }
    None
}

/// Extract located structured diagnostics attached to `error` (from a [`ContractError`] or
/// [`DiagnosticError`] in its context chain), if any.
#[must_use]
pub fn error_located_diagnostics(
    error: &anyhow::Error,
) -> Option<&[diagnostic::LocatedDiagnostic]> {
    for cause in error.chain() {
        if let Some(contract) = cause.downcast_ref::<ContractError>()
            && !contract.located.is_empty()
        {
            return Some(&contract.located);
        }
        if let Some(operational) = cause.downcast_ref::<DiagnosticError>()
            && !operational.located.is_empty()
        {
            return Some(&operational.located);
        }
    }
    None
}

/// Build an operational (exit 2) error carrying a single structured diagnostic.
pub(crate) fn operational_error(
    code: &'static str,
    message: impl Into<String>,
    path: Option<String>,
) -> anyhow::Error {
    let mut diag = diagnostic::Diagnostic::error(code, message);
    let mut located_file = None;
    if let Some(path) = path {
        diag = diag.with_path(path.clone());
        located_file = Some(path);
    }
    let mut located = diagnostic::LocatedDiagnostic::from_diagnostic(diag);
    if let Some(file) = located_file {
        located = located.with_file(file);
    }
    anyhow::Error::new(DiagnosticError::single_located(located))
}

/// Build an operational (exit 2) error carrying a single located structured diagnostic with a
/// physical file path and optional 1-based UTF-16 source position.
pub(crate) fn operational_error_at(
    code: &'static str,
    message: impl Into<String>,
    file: String,
    position: Option<diagnostic::Position>,
) -> anyhow::Error {
    let mut diag = diagnostic::Diagnostic::error(code, message).with_path(file.clone());
    if let Some(position) = position {
        diag = diag.with_position(position);
    }
    let located = diagnostic::LocatedDiagnostic::from_diagnostic(diag).with_file(file);
    anyhow::Error::new(DiagnosticError::single_located(located))
}

/// Compute a 1-based UTF-16 [`diagnostic::Position`] covering a single line (1-based `line_no`).
pub(crate) fn line_position(line_no: usize, raw_line: &str) -> diagnostic::Position {
    let trimmed = raw_line.strip_suffix('\r').unwrap_or(raw_line);
    let trimmed = if line_no == 1 {
        trimmed.strip_prefix('\u{feff}').unwrap_or(trimmed)
    } else {
        trimmed
    };
    let width: usize = trimmed.chars().map(char::len_utf16).sum();
    diagnostic::Position {
        line: line_no,
        column: 1,
        end_line: line_no,
        end_column: 1 + width,
    }
}

/// Precomputed line-start table for one source file so repeated span-to-(line, UTF-16 col)
/// lookups stay O(log lines + line length) instead of rescanning from byte 0.
pub(crate) struct LineIndex {
    bom_bytes: usize,
    body: String,
    /// Byte offsets inside `body` where each 1-based line begins (`line_starts[0] == 0`).
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub(crate) fn new(source: String) -> Self {
        let (bom_bytes, body) = if let Some(rest) = source.strip_prefix('\u{feff}') {
            ('\u{feff}'.len_utf8(), rest.to_string())
        } else {
            (0, source)
        };
        let bytes = body.as_bytes();
        let mut line_starts = vec![0usize];
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'\n' || (bytes[i] == b'\r' && bytes.get(i + 1) != Some(&b'\n')) {
                line_starts.push(i + 1);
            }
            i += 1;
        }
        Self {
            bom_bytes,
            body,
            line_starts,
        }
    }

    pub(crate) fn position(&self, start: usize, end: usize) -> diagnostic::Position {
        let start_adj = start.saturating_sub(self.bom_bytes);
        let end_adj = end.saturating_sub(self.bom_bytes);
        let clamped_start = snap_char_boundary_down(&self.body, start_adj.min(self.body.len()));
        let clamped_end = if end_adj <= start_adj {
            clamped_start
        } else {
            snap_char_boundary_up(&self.body, end_adj.min(self.body.len())).max(clamped_start)
        };
        let (line, column) = self.offset_to_line_col_utf16(clamped_start);
        let (end_line, end_column) = if clamped_end == clamped_start {
            (line, column)
        } else {
            self.offset_to_line_col_utf16(clamped_end)
        };
        diagnostic::Position {
            line,
            column,
            end_line,
            end_column,
        }
    }

    fn offset_to_line_col_utf16(&self, target: usize) -> (usize, usize) {
        let line_idx = self
            .line_starts
            .partition_point(|&start| start <= target)
            .saturating_sub(1);
        let line_start = self.line_starts[line_idx];
        let slice = &self.body[line_start..target];
        let col = if slice.is_ascii() {
            1 + slice.bytes().filter(|&b| b != b'\r').count()
        } else {
            1 + slice
                .chars()
                .filter(|&ch| ch != '\r')
                .map(char::len_utf16)
                .sum::<usize>()
        };
        (line_idx + 1, col)
    }
}

/// Convert a UTF-8 byte range inside `source` into a 1-based UTF-16 [`diagnostic::Position`].
///
/// Handles CRLF line endings, leading UTF-8 BOMs, tabs (1 UTF-16 code unit), BMP non-ASCII
/// characters (1 UTF-16 code unit), and supplementary Unicode scalars (2 UTF-16 code units).
/// Offsets that land inside a multi-byte UTF-8 sequence (for example, `wit-parser` lex errors
/// producing `pos..pos+1` on a BOM or non-ASCII byte) are snapped to valid character boundaries.
#[cfg_attr(not(test), allow(dead_code))]
#[must_use]
pub(crate) fn byte_range_to_utf16_position(
    source: &str,
    start: usize,
    end: usize,
) -> diagnostic::Position {
    LineIndex::new(source.to_string()).position(start, end)
}

fn snap_char_boundary_down(source: &str, mut offset: usize) -> usize {
    offset = offset.min(source.len());
    while offset > 0 && !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn snap_char_boundary_up(source: &str, mut offset: usize) -> usize {
    offset = offset.min(source.len());
    while offset < source.len() && !source.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

fn load_line_index<'a>(
    path: &str,
    cache: &'a mut BTreeMap<String, LineIndex>,
    virtual_sources: Option<&BTreeMap<String, String>>,
) -> Option<&'a LineIndex> {
    if !cache.contains_key(path) {
        let text = match virtual_sources {
            Some(sources) => sources.get(path).cloned()?,
            None => fs::read_to_string(path).ok()?,
        };
        cache.insert(path.to_string(), LineIndex::new(text));
    }
    cache.get(path)
}

fn resolve_span_cached_with_sources(
    source_map: &SourceMap,
    span: wit_parser::Span,
    cache: &mut BTreeMap<String, LineIndex>,
    virtual_sources: Option<&BTreeMap<String, String>>,
) -> Option<(String, diagnostic::Position)> {
    let location = source_map.resolve_span(span)?;
    let path = location.path.to_string();
    let index = load_line_index(&path, cache, virtual_sources)?;
    let position = index.position(location.range.start, location.range.end);
    Some((path, position))
}

pub(crate) fn resolve_contract_span_cached(
    contract: &Contract,
    span: wit_parser::Span,
    cache: &mut BTreeMap<String, LineIndex>,
) -> Option<(String, diagnostic::Position)> {
    resolve_span_cached_with_sources(
        &contract.resolve.source_map,
        span,
        cache,
        contract.source_contents.as_ref(),
    )
}

/// Resolve a `wit-parser` [`wit_parser::Span`] against `source_map`.
pub(crate) fn resolve_span_location(
    source_map: &SourceMap,
    span: wit_parser::Span,
) -> Option<(String, diagnostic::Position)> {
    resolve_span_location_with_sources(source_map, span, None)
}

fn resolve_span_location_with_sources(
    source_map: &SourceMap,
    span: wit_parser::Span,
    virtual_sources: Option<&BTreeMap<String, String>>,
) -> Option<(String, diagnostic::Position)> {
    let mut cache = BTreeMap::new();
    resolve_span_cached_with_sources(source_map, span, &mut cache, virtual_sources)
}

struct PackageHeaderMatch {
    ns_name: Option<String>,
    version: Option<String>,
    is_nested_block: bool,
    position: diagnostic::Position,
}

fn parse_package_header_line(rest: &str) -> (Option<String>, Option<String>, bool) {
    let bytes = rest.as_bytes();
    let mut i = "package".len();
    let skip_ws_and_comments = |pos: &mut usize| {
        while *pos < bytes.len() {
            if matches!(bytes[*pos], b' ' | b'\t' | b'\r') {
                *pos += 1;
            } else if *pos + 1 < bytes.len() && bytes[*pos] == b'/' && bytes[*pos + 1] == b'*' {
                *pos += 2;
                let mut depth = 1usize;
                while *pos + 1 < bytes.len() && depth > 0 {
                    if bytes[*pos] == b'/' && bytes[*pos + 1] == b'*' {
                        depth += 1;
                        *pos += 2;
                    } else if bytes[*pos] == b'*' && bytes[*pos + 1] == b'/' {
                        depth -= 1;
                        *pos += 2;
                    } else {
                        *pos += 1;
                    }
                }
                if depth > 0 {
                    *pos = bytes.len();
                }
            } else {
                break;
            }
        }
    };
    skip_ws_and_comments(&mut i);
    let name_start = i;
    while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b'\r' | b';' | b'{' | b'@' | b'/') {
        i += 1;
    }
    let raw_name = &rest[name_start..i];
    let ns_name = raw_name.split_once(':').map(|(ns, name)| {
        let ns = ns.strip_prefix('%').unwrap_or(ns);
        let name = name.strip_prefix('%').unwrap_or(name);
        format!("{ns}:{name}")
    });
    skip_ws_and_comments(&mut i);
    let version = if i < bytes.len() && bytes[i] == b'@' {
        i += 1;
        let ver_start = i;
        while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b'\r' | b';' | b'{' | b'/') {
            i += 1;
        }
        let ver = &rest[ver_start..i];
        (!ver.is_empty()).then(|| ver.to_string())
    } else {
        None
    };
    let mut is_nested_block = false;
    while i < bytes.len() {
        skip_ws_and_comments(&mut i);
        if i >= bytes.len() {
            break;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            break;
        }
        if bytes[i] == b'{' {
            is_nested_block = true;
            break;
        }
        if bytes[i] == b';' {
            break;
        }
        i += 1;
    }
    (ns_name, version, is_nested_block)
}

/// Collect all explicit `package ...` header lines inside `file`, skipping line and nested block
/// comments.
fn collect_package_headers(
    file: &str,
    cache: &mut BTreeMap<String, LineIndex>,
    virtual_sources: Option<&BTreeMap<String, String>>,
) -> Vec<PackageHeaderMatch> {
    let Some(index) = load_line_index(file, cache, virtual_sources) else {
        return Vec::new();
    };
    let mut matches = Vec::new();
    let mut block_comment_depth = 0usize;
    for (line_idx, raw) in index.body.lines().enumerate() {
        let line_no = line_idx + 1;
        let bytes = raw.as_bytes();
        let mut i = 0usize;
        let mut code_start = None;
        while i < bytes.len() {
            if block_comment_depth > 0 {
                if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    block_comment_depth += 1;
                    i += 2;
                    continue;
                }
                if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    block_comment_depth -= 1;
                    i += 2;
                    continue;
                }
                i += 1;
            } else if bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r' {
                i += 1;
            } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
                break;
            } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                block_comment_depth = 1;
                i += 2;
            } else {
                code_start = Some(i);
                break;
            }
        }
        if let Some(start) = code_start {
            let rest = &raw[start..];
            if rest.strip_prefix("package").is_some_and(|after| {
                after.is_empty() || matches!(after.as_bytes()[0], b' ' | b'\t' | b'\r' | b'/')
            }) {
                let (ns_name, version, is_nested_block) = parse_package_header_line(rest);
                matches.push(PackageHeaderMatch {
                    ns_name,
                    version,
                    is_nested_block,
                    position: line_position(line_no, raw),
                });
            }
            // Still advance nested block-comment state across the rest of the line.
            while i + 1 < bytes.len() {
                if block_comment_depth > 0 {
                    if bytes[i] == b'/' && bytes[i + 1] == b'*' {
                        block_comment_depth += 1;
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        block_comment_depth -= 1;
                        i += 2;
                        continue;
                    }
                } else if bytes[i] == b'/' && bytes[i + 1] == b'/' {
                    break;
                } else if bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    block_comment_depth = 1;
                    i += 2;
                    continue;
                }
                i += 1;
            }
        }
    }
    matches
}

fn find_matching_package_header(
    candidates: &[String],
    target: &wit_parser::PackageName,
    prefer_top_level: bool,
    cache: &mut BTreeMap<String, LineIndex>,
    virtual_sources: Option<&BTreeMap<String, String>>,
) -> Option<(String, diagnostic::Position)> {
    let expected_name = format!("{}:{}", target.namespace, target.name);
    let expected_version = target.version.as_ref().map(ToString::to_string);
    let mut best: Option<(u8, String, diagnostic::Position)> = None;
    for candidate in candidates {
        for m in collect_package_headers(candidate, cache, virtual_sources) {
            if m.ns_name.as_deref() != Some(expected_name.as_str()) {
                continue;
            }
            let version_matches = m.version == expected_version;
            let level_matches = !prefer_top_level || !m.is_nested_block;
            let rank = match (level_matches, version_matches) {
                (true, true) => 0,
                (true, false) => 1,
                (false, true) => 2,
                (false, false) => 3,
            };
            if rank == 0 {
                return Some((candidate.clone(), m.position));
            }
            if best
                .as_ref()
                .is_none_or(|(best_rank, _, _)| rank < *best_rank)
            {
                best = Some((rank, candidate.clone(), m.position));
            }
        }
    }
    best.map(|(_, file, pos)| (file, pos))
}

/// Locate the `package ...;` declaration line inside `file`, skipping comments and falling back
/// to line 1.
pub(crate) fn find_package_header_position(
    file: &str,
    cache: &mut BTreeMap<String, LineIndex>,
    virtual_sources: Option<&BTreeMap<String, String>>,
) -> Option<(String, diagnostic::Position)> {
    if let Some(first) = collect_package_headers(file, cache, virtual_sources)
        .into_iter()
        .next()
    {
        return Some((file.to_string(), first.position));
    }
    let index = load_line_index(file, cache, virtual_sources)?;
    let first_line = index.body.lines().next().unwrap_or("");
    Some((file.to_string(), line_position(1, first_line)))
}

fn pick_package_header_file(
    candidates: &[String],
    target: &wit_parser::PackageName,
    prefer_top_level: bool,
    cache: &mut BTreeMap<String, LineIndex>,
    virtual_sources: Option<&BTreeMap<String, String>>,
) -> Option<(String, diagnostic::Position)> {
    if let Some(found) =
        find_matching_package_header(candidates, target, prefer_top_level, cache, virtual_sources)
    {
        return Some(found);
    }
    for candidate in candidates {
        if let Some(first) = collect_package_headers(candidate, cache, virtual_sources)
            .into_iter()
            .next()
        {
            return Some((candidate.clone(), first.position));
        }
    }
    let first = candidates.first()?;
    find_package_header_position(first, cache, virtual_sources)
}

/// Find the declaring file and `package` header position for `package_id` in `contract`.
#[allow(clippy::too_many_lines)]
pub(crate) fn package_location(
    contract: &Contract,
    package_id: PackageId,
    cache: &mut BTreeMap<String, LineIndex>,
) -> Option<(String, diagnostic::Position)> {
    let resolve = &contract.resolve;
    let package = &resolve.packages[package_id];
    let virtual_sources = contract.source_contents.as_ref();
    let prefer_top_level = package_id == contract.package;
    let mut file: Option<String> = None;
    for &iface_id in package.interfaces.values() {
        if let Some(loc) = resolve
            .source_map
            .resolve_span(resolve.interfaces[iface_id].span)
        {
            file = Some(loc.path.to_string());
            break;
        }
    }
    if file.is_none() {
        for &world_id in package.worlds.values() {
            if let Some(loc) = resolve
                .source_map
                .resolve_span(resolve.worlds[world_id].span)
            {
                file = Some(loc.path.to_string());
                break;
            }
        }
    }
    if let Some(file) = &file
        && let Some(found) = find_matching_package_header(
            std::slice::from_ref(file),
            &package.name,
            prefer_top_level,
            cache,
            virtual_sources,
        )
    {
        return Some(found);
    }
    if let Some(entry) = contract.dependency_entry(package_id) {
        if let Some(sources) = virtual_sources {
            let entry_key = entry.to_string_lossy().replace('\\', "/");
            if sources.contains_key(entry_key.as_str()) {
                return pick_package_header_file(
                    &[entry_key],
                    &package.name,
                    false,
                    cache,
                    Some(sources),
                );
            }
            let prefix = format!("{entry_key}/");
            let candidates: Vec<String> = sources
                .keys()
                .filter(|key| key.starts_with(&prefix))
                .cloned()
                .collect();
            if let Some(found) =
                pick_package_header_file(&candidates, &package.name, false, cache, Some(sources))
            {
                return Some(found);
            }
        } else if entry.is_file() {
            return pick_package_header_file(
                &[entry.display().to_string()],
                &package.name,
                false,
                cache,
                None,
            );
        } else if entry.is_dir()
            && let Ok(wit_files) = sorted_wit_files(entry)
        {
            let candidates: Vec<String> = wit_files
                .iter()
                .map(|path| path.display().to_string())
                .collect();
            if let Some(found) =
                pick_package_header_file(&candidates, &package.name, false, cache, None)
            {
                return Some(found);
            }
        }
    }
    let root_candidates: Vec<String> = if let Some(sources) = virtual_sources {
        sources
            .keys()
            .filter(|key| !key.starts_with("deps/"))
            .cloned()
            .collect()
    } else {
        resolve
            .source_map
            .source_files()
            .map(|p| p.display().to_string())
            .collect()
    };
    if let Some(found) = find_matching_package_header(
        &root_candidates,
        &package.name,
        prefer_top_level,
        cache,
        virtual_sources,
    ) {
        return Some(found);
    }
    if prefer_top_level
        && let Some(found) = pick_package_header_file(
            &root_candidates,
            &package.name,
            true,
            cache,
            virtual_sources,
        )
    {
        return Some(found);
    }
    if let Some(file) = file {
        return find_package_header_position(&file, cache, virtual_sources);
    }
    None
}

/// An interface explicitly imported by a root world: `(Option<(namespace, package)>, interface)`.
type ExplicitWorldImport = (Option<(String, String)>, String);
/// A foreign world referenced by a root world's `include`: `(namespace, package, world_name)`.
type ForeignWorldInclude = (String, String, String);

/// One named UTF-8 WIT source used by [`load_sources`].
///
/// Names use `/` separators and follow the standard package layout: root files are
/// `<name>.wit`; dependencies are `deps/<entry>.wit` or `deps/<entry>/<name>.wit`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SourceFile {
    /// Root-relative virtual source name.
    pub name: String,
    /// UTF-8 WIT source text.
    pub contents: String,
}

impl SourceFile {
    /// Create an in-memory WIT source.
    #[must_use]
    pub fn new(name: String, contents: String) -> Self {
        Self { name, contents }
    }
}
/// A parsed WIT package together with the resolve that owns it.
///
/// The fields are crate-private so that `wit-parser` types do not become part of witforge's
/// public API. Callers use [`load`] and the model/subset/lock modules instead.
///
/// The boundary is compile-time enforced; external code cannot reach the resolved package:
///
/// ```compile_fail
/// let contract = witforge::load(".").unwrap();
/// let _ = contract.resolve;
/// ```
#[derive(Debug)]
pub struct Contract {
    pub(crate) resolve: Resolve,
    pub(crate) package: PackageId,
    pub(crate) dependencies: Vec<PackageId>,
    pub(crate) dependency_entries: BTreeMap<PackageId, PathBuf>,
    pub(crate) explicit_world_imports: BTreeMap<String, BTreeSet<ExplicitWorldImport>>,
    pub(crate) included_foreign_worlds: BTreeMap<String, Vec<ForeignWorldInclude>>,
    pub(crate) source_contents: Option<BTreeMap<String, String>>,
}

impl Contract {
    /// Whether `package` is one of the standard `deps/` packages, rather than the root
    /// package or a nested package block in the root's source.
    #[must_use]
    pub(crate) fn is_dependency(&self, package: PackageId) -> bool {
        self.dependencies.contains(&package)
    }

    /// The `deps/` entry path that supplied `package`, when known.
    #[must_use]
    pub(crate) fn dependency_entry(&self, package: PackageId) -> Option<&Path> {
        self.dependency_entries.get(&package).map(PathBuf::as_path)
    }

    /// Whether `interface_id` was explicitly listed as an import of root world `world_name`
    /// (including through a local `include`) rather than synthesized by `wit-parser` world
    /// elaboration.
    #[must_use]
    pub(crate) fn is_explicit_world_import(
        &self,
        world_name: &str,
        interface_id: InterfaceId,
    ) -> bool {
        let interface = &self.resolve.interfaces[interface_id];
        let Some(name) = interface.name.as_deref() else {
            return true;
        };
        let package_key = interface
            .package
            .filter(|package| *package != self.package)
            .map(|package| {
                let pkg = &self.resolve.packages[package].name;
                (pkg.namespace.clone(), pkg.name.clone())
            });
        self.explicit_world_imports
            .get(world_name)
            .is_some_and(|imports| imports.contains(&(package_key, name.to_string())))
    }

    /// Foreign worlds reached by `include` from root world `world_name`.
    #[must_use]
    pub(crate) fn included_foreign_worlds(&self, world_name: &str) -> &[ForeignWorldInclude] {
        self.included_foreign_worlds
            .get(world_name)
            .map_or(&[], Vec::as_slice)
    }

    /// Map canonical logical contract paths (both package-qualified, such as
    /// `ns:pkg/iface/rec.field`, and package-local, such as `iface/rec.field`) to their
    /// declaring physical source file and 1-based UTF-16 [`diagnostic::Position`].
    ///
    /// For a `use` alias, the target type's declaring span is recorded rather than the `use` site.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn member_locations(&self) -> BTreeMap<String, (String, diagnostic::Position)> {
        let mut out = BTreeMap::new();
        let mut cache = BTreeMap::new();
        let resolve = &self.resolve;
        let package = &resolve.packages[self.package];
        let pkg = format!("{}:{}", package.name.namespace, package.name.name);
        if let Some(pkg_loc) = package_location(self, self.package, &mut cache) {
            out.insert(pkg.clone(), pkg_loc);
        }
        for &dep_id in &self.dependencies {
            let dep = &resolve.packages[dep_id];
            let dep_pkg = format!("{}:{}", dep.name.namespace, dep.name.name);
            if let Some(loc) = package_location(self, dep_id, &mut cache) {
                out.entry(dep_pkg).or_insert(loc);
            }
        }
        let mut record = |full_key: String, short_key: Option<String>, span: wit_parser::Span| {
            if let Some(loc) = resolve_contract_span_cached(self, span, &mut cache) {
                if let Some(short) = short_key {
                    out.entry(short).or_insert_with(|| loc.clone());
                }
                out.entry(full_key).or_insert(loc);
            }
        };
        for (iface_name, &iface_id) in &package.interfaces {
            let iface = &resolve.interfaces[iface_id];
            record(
                format!("{pkg}/{iface_name}"),
                Some(iface_name.clone()),
                iface.span,
            );
            for (type_name, &type_id) in &iface.types {
                let mut decl_id = type_id;
                let mut hops = 0usize;
                while hops < 32 {
                    let def = &resolve.types[decl_id];
                    if let wit_parser::TypeDefKind::Type(wit_parser::Type::Id(target_id)) = def.kind
                        && resolve.types[target_id].name.is_some()
                        && resolve.types[target_id].owner != def.owner
                    {
                        decl_id = target_id;
                        hops += 1;
                        continue;
                    }
                    break;
                }
                let def = &resolve.types[decl_id];
                let full_type = format!("{pkg}/{iface_name}/{type_name}");
                let short_type = format!("{iface_name}/{type_name}");
                record(full_type.clone(), Some(short_type.clone()), def.span);
                match &def.kind {
                    wit_parser::TypeDefKind::Record(rec) => {
                        for field in &rec.fields {
                            record(
                                format!("{full_type}.{}", field.name),
                                Some(format!("{short_type}.{}", field.name)),
                                field.span,
                            );
                        }
                    }
                    wit_parser::TypeDefKind::Variant(var) => {
                        for case in &var.cases {
                            record(
                                format!("{full_type}.{}", case.name),
                                Some(format!("{short_type}.{}", case.name)),
                                case.span,
                            );
                        }
                    }
                    wit_parser::TypeDefKind::Enum(en) => {
                        for case in &en.cases {
                            record(
                                format!("{full_type}.{}", case.name),
                                Some(format!("{short_type}.{}", case.name)),
                                case.span,
                            );
                        }
                    }
                    wit_parser::TypeDefKind::Flags(flags) => {
                        for flag in &flags.flags {
                            record(
                                format!("{full_type}.{}", flag.name),
                                Some(format!("{short_type}.{}", flag.name)),
                                flag.span,
                            );
                        }
                    }
                    _ => {}
                }
            }
            for (func_name, func) in &iface.functions {
                record(
                    format!("{pkg}/{iface_name}.{func_name}"),
                    Some(format!("{iface_name}#{func_name}")),
                    func.span,
                );
                record(
                    format!("{pkg}/{iface_name}#{func_name}"),
                    Some(format!("{iface_name}.{func_name}")),
                    func.span,
                );
                for param in &func.params {
                    record(
                        format!("{pkg}/{iface_name}#{func_name}.{}", param.name),
                        Some(format!("{iface_name}.{func_name}.{}", param.name)),
                        param.span,
                    );
                    record(
                        format!("{pkg}/{iface_name}#{func_name}({})", param.name),
                        Some(format!("{iface_name}#{func_name}.{}", param.name)),
                        param.span,
                    );
                }
            }
        }
        for (world_name, &world_id) in &package.worlds {
            let world = &resolve.worlds[world_id];
            record(
                format!("{pkg}/{world_name}"),
                Some(world_name.clone()),
                world.span,
            );
            record(
                format!("{pkg}/{world_name}/import"),
                Some(format!("{world_name}/import")),
                world.span,
            );
            record(
                format!("{pkg}/{world_name}/export"),
                Some(format!("{world_name}/export")),
                world.span,
            );
            for (verb, items) in [("import", &world.imports), ("export", &world.exports)] {
                for (key, item) in items {
                    let (item_name, qualified) = match key {
                        WorldKey::Name(name) => (name.clone(), name.clone()),
                        WorldKey::Interface(id) => {
                            let iface = &resolve.interfaces[*id];
                            let name = iface.name.clone().unwrap_or_default();
                            let qualified = match iface.package {
                                Some(pkg_id) if pkg_id != self.package => {
                                    let dep = &resolve.packages[pkg_id].name;
                                    format!("{}:{}/{name}", dep.namespace, dep.name)
                                }
                                _ => name.clone(),
                            };
                            (name, qualified)
                        }
                    };
                    let item_span = if item.span().is_known() {
                        item.span()
                    } else {
                        world.span
                    };
                    record(
                        format!("{pkg}/{world_name}/{verb}/{qualified}"),
                        Some(format!("{world_name}/{verb}/{qualified}")),
                        item_span,
                    );
                    record(format!("{pkg}/{world_name}/{item_name}"), None, item_span);
                }
            }
        }
        for &dep_id in &self.dependencies {
            let dep = &resolve.packages[dep_id];
            let dep_pkg = format!("{}:{}", dep.name.namespace, dep.name.name);
            for (iface_name, &iface_id) in &dep.interfaces {
                let iface = &resolve.interfaces[iface_id];
                record(format!("{dep_pkg}/{iface_name}"), None, iface.span);
                for (type_name, &type_id) in &iface.types {
                    let def = &resolve.types[type_id];
                    let full_type = format!("{dep_pkg}/{iface_name}/{type_name}");
                    record(full_type.clone(), None, def.span);
                    match &def.kind {
                        wit_parser::TypeDefKind::Record(rec) => {
                            for field in &rec.fields {
                                record(format!("{full_type}.{}", field.name), None, field.span);
                            }
                        }
                        wit_parser::TypeDefKind::Variant(var) => {
                            for case in &var.cases {
                                record(format!("{full_type}.{}", case.name), None, case.span);
                            }
                        }
                        wit_parser::TypeDefKind::Enum(en) => {
                            for case in &en.cases {
                                record(format!("{full_type}.{}", case.name), None, case.span);
                            }
                        }
                        wit_parser::TypeDefKind::Flags(flags) => {
                            for flag in &flags.flags {
                                record(format!("{full_type}.{}", flag.name), None, flag.span);
                            }
                        }
                        _ => {}
                    }
                }
                for (func_name, func) in &iface.functions {
                    record(
                        format!("{dep_pkg}/{iface_name}#{func_name}"),
                        None,
                        func.span,
                    );
                    for param in &func.params {
                        record(
                            format!("{dep_pkg}/{iface_name}#{func_name}({})", param.name),
                            None,
                            param.span,
                        );
                    }
                }
            }
        }
        out
    }
}

/// Load the WIT package in `dir`.
///
/// Files in `dir` and in each package directory under `deps/` are loaded in ascending file-name
/// byte order (never raw directory-enumeration order or locale order), so source spans and model
/// ordering are deterministic across file systems and platforms. All features are enabled, so
/// items gated by `@unstable` are kept; otherwise they would silently disappear and the lockfile
/// would report their members as removed.
///
/// # Errors
///
/// Returns an error when the directory cannot be read or the WIT does not parse or resolve, and a
/// [`ContractError`] when the dependency graph pins more than one version of one package or a
/// dependency uses the root package's namespace and name.
pub fn load(dir: &Path) -> Result<Contract> {
    let context = format!("failed to load the WIT package in {}", dir.display());
    let top_pkg = parse_sorted_dir(dir)
        .with_context(|| format!("failed to parse package: {}", dir.display()))
        .with_context(|| context.clone())?;
    let mut resolve = Resolve {
        all_features: true,
        ..Resolve::default()
    };
    let loaded_deps =
        load_deps_dir(&dir.join("deps"), &mut resolve).with_context(|| context.clone())?;
    finish_load(top_pkg, loaded_deps, resolve, &context, dir, None)
}

/// Load a WIT package from named in-memory UTF-8 sources.
///
/// Source names use the same direct-root and standard `deps/` layout as [`load`]. Files and
/// dependency entries are sorted by their UTF-8 names before parsing, so declaration order and
/// diagnostics are deterministic regardless of caller insertion order. In-memory loading accepts
/// textual `.wit` dependencies; use [`load`] when a dependency entry is a `.wasm` or `.wat` file.
///
/// # Errors
///
/// Returns an operational parse error for an empty, duplicate, absolute, parent-relative,
/// backslash-containing, nested, or non-`.wit` source name, for a missing root source, or when
/// WIT parsing or resolution fails. Returns a [`ContractError`] for an invalid dependency
/// identity, like [`load`].
pub fn load_sources(sources: Vec<SourceFile>) -> Result<Contract> {
    let mut seen = BTreeSet::new();
    let mut root_sources = Vec::new();
    let mut dependency_sources: BTreeMap<String, Vec<&SourceFile>> = BTreeMap::new();
    for source in &sources {
        if source.name.is_empty()
            || source.name.starts_with('/')
            || source.name.contains('\\')
            || !seen.insert(source.name.clone())
        {
            return Err(invalid_source_name(&source.name));
        }
        let parts: Vec<&str> = source.name.split('/').collect();
        if parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
            || parts.last().is_none_or(|name| !has_wit_extension(name))
        {
            return Err(invalid_source_name(&source.name));
        }
        match parts.as_slice() {
            [_file] => root_sources.push(source),
            ["deps", entry] => {
                dependency_sources
                    .entry((*entry).to_string())
                    .or_default()
                    .push(source);
            }
            ["deps", entry, _file] if !has_wit_extension(entry) => {
                dependency_sources
                    .entry((*entry).to_string())
                    .or_default()
                    .push(source);
            }
            _ => return Err(invalid_source_name(&source.name)),
        }
    }
    if root_sources.is_empty() {
        return Err(operational_error(
            diagnostic::codes::PARSE_ERROR,
            "in-memory WIT package has no root `.wit` source",
            None,
        ));
    }

    let top_pkg = parse_memory_group(&root_sources, "<memory>")?;
    let mut loaded_deps = LoadedDeps::default();
    for (entry, group_sources) in dependency_sources {
        let entry_path = PathBuf::from(format!("deps/{entry}"));
        let group = parse_memory_group(&group_sources, &entry_path.display().to_string())
            .with_context(|| format!("failed to parse package: {}", entry_path.display()))?;
        for package in std::iter::once(&group.main).chain(&group.nested) {
            loaded_deps
                .scanned
                .push((package.name.clone(), entry_path.clone()));
            loaded_deps
                .entries_by_package
                .insert(package.name.clone(), entry_path.clone());
        }
        loaded_deps.groups.push(group);
    }
    let source_contents = sources
        .into_iter()
        .map(|source| (source.name, source.contents))
        .collect();
    let resolve = Resolve {
        all_features: true,
        ..Resolve::default()
    };
    finish_load(
        top_pkg,
        loaded_deps,
        resolve,
        "failed to load the in-memory WIT package",
        Path::new("<memory>"),
        Some(source_contents),
    )
}

fn has_wit_extension(name: &str) -> bool {
    Path::new(name).extension() == Some(std::ffi::OsStr::new("wit"))
}

fn invalid_source_name(name: &str) -> anyhow::Error {
    operational_error(
        diagnostic::codes::PARSE_ERROR,
        format!(
            "invalid in-memory source name `{name}`; expected `<name>.wit`, \
             `deps/<entry>.wit`, or `deps/<entry>/<name>.wit`"
        ),
        (!name.is_empty()).then(|| name.to_string()),
    )
}

fn parse_memory_group(
    sources: &[&SourceFile],
    fallback_path: &str,
) -> Result<UnresolvedPackageGroup> {
    let mut sources = sources.to_vec();
    sources.sort_by(|left, right| {
        let left_file = left.name.rsplit('/').next().unwrap_or_default().as_bytes();
        let right_file = right.name.rsplit('/').next().unwrap_or_default().as_bytes();
        left_file
            .cmp(right_file)
            .then_with(|| left.name.as_bytes().cmp(right.name.as_bytes()))
    });
    let mut map = SourceMap::new();
    let mut virtual_sources = BTreeMap::new();
    for source in &sources {
        map.push_str(&source.name, source.contents.clone());
        virtual_sources.insert(source.name.clone(), source.contents.clone());
    }
    map.parse().map_err(|(source_map, err)| {
        let span = err.kind().span();
        let resolved =
            resolve_span_location_with_sources(&source_map, span, Some(&virtual_sources));
        let rendered = err.render(&source_map);
        let message = if let Some(source) = sources
            .iter()
            .find(|source| source.contents.starts_with('\u{feff}'))
        {
            format!(
                "{rendered} (note: {} starts with a UTF-8 byte-order mark (BOM))",
                source.name
            )
        } else {
            rendered
        };
        match resolved {
            Some((file, position)) => operational_error_at(
                diagnostic::codes::PARSE_ERROR,
                message,
                file,
                Some(position),
            ),
            None => operational_error(
                diagnostic::codes::PARSE_ERROR,
                message,
                Some(fallback_path.to_string()),
            ),
        }
    })
}

fn finish_load(
    top_pkg: UnresolvedPackageGroup,
    loaded_deps: LoadedDeps,
    mut resolve: Resolve,
    context: &str,
    fallback_path: &Path,
    source_contents: Option<BTreeMap<String, String>>,
) -> Result<Contract> {
    let (explicit_world_imports, included_foreign_worlds) =
        collect_explicit_world_imports(&top_pkg.main);

    // Check dependency identity before `push_groups` so duplicate package names become a
    // `ContractError` naming the package, versions, and `deps/` entries rather than a
    // `wit-parser` duplicate-package error.
    crate::model::ensure_dependency_identity(&top_pkg.main.name, &loaded_deps.scanned)?;

    let package = resolve
        .push_groups(top_pkg, loaded_deps.groups)
        .map_err(|err| {
            let span = err.kind().span();
            let resolved = resolve_span_location_with_sources(
                &resolve.source_map,
                span,
                source_contents.as_ref(),
            );
            let err_anyhow: anyhow::Error = err.into();
            let rendered = resolve.render_error(&err_anyhow);
            match resolved {
                Some((file, position)) => operational_error_at(
                    diagnostic::codes::PARSE_ERROR,
                    rendered,
                    file,
                    Some(position),
                ),
                None => operational_error(
                    diagnostic::codes::PARSE_ERROR,
                    rendered,
                    Some(fallback_path.display().to_string()),
                ),
            }
        })
        .with_context(|| context.to_string())?;

    let mut dependencies = Vec::new();
    let mut dependency_entries = BTreeMap::new();
    for (id, package_data) in &resolve.packages {
        if id == package {
            continue;
        }
        if let Some(entry) = loaded_deps.binary_entries.get(&id).cloned().or_else(|| {
            loaded_deps
                .entries_by_package
                .get(&package_data.name)
                .cloned()
        }) {
            dependencies.push(id);
            dependency_entries.insert(id, entry);
        }
    }
    Ok(Contract {
        resolve,
        package,
        dependencies,
        dependency_entries,
        explicit_world_imports,
        included_foreign_worlds,
        source_contents,
    })
}

#[derive(Default)]
struct LoadedDeps {
    groups: Vec<UnresolvedPackageGroup>,
    entries_by_package: BTreeMap<PackageName, PathBuf>,
    binary_entries: BTreeMap<PackageId, PathBuf>,
    scanned: Vec<(PackageName, PathBuf)>,
}

fn load_deps_dir(deps_dir: &Path, resolve: &mut Resolve) -> Result<LoadedDeps> {
    let mut loaded = LoadedDeps {
        groups: Vec::new(),
        entries_by_package: BTreeMap::new(),
        binary_entries: BTreeMap::new(),
        scanned: Vec::new(),
    };
    if !deps_dir.exists() {
        return Ok(loaded);
    }
    let dep_paths = sorted_directory_entries(deps_dir).with_context(|| {
        format!(
            "failed to parse dependency directory: {}",
            deps_dir.display()
        )
    })?;
    for entry_path in dep_paths {
        let file_type = fs::symlink_metadata(&entry_path)
            .with_context(|| format!("failed to inspect {}", entry_path.display()))?
            .file_type();
        let is_dir = file_type.is_dir()
            || (file_type.is_symlink() && fs::metadata(&entry_path).is_ok_and(|m| m.is_dir()));
        if is_dir {
            let group = parse_sorted_dir(&entry_path)
                .with_context(|| format!("failed to parse package: {}", entry_path.display()))?;
            for pkg in std::iter::once(&group.main).chain(&group.nested) {
                loaded.scanned.push((pkg.name.clone(), entry_path.clone()));
                loaded
                    .entries_by_package
                    .insert(pkg.name.clone(), entry_path.clone());
            }
            loaded.groups.push(group);
        } else {
            match entry_path.extension().and_then(std::ffi::OsStr::to_str) {
                Some("wit") => {
                    let group = parse_single_wit_file(&entry_path)?;
                    for pkg in std::iter::once(&group.main).chain(&group.nested) {
                        loaded.scanned.push((pkg.name.clone(), entry_path.clone()));
                        loaded
                            .entries_by_package
                            .insert(pkg.name.clone(), entry_path.clone());
                    }
                    loaded.groups.push(group);
                }
                Some("wat" | "wasm") => {
                    let before: BTreeSet<PackageId> =
                        resolve.packages.iter().map(|(id, _)| id).collect();
                    resolve
                        .push_file(&entry_path)
                        .map_err(|err| {
                            if err
                                .chain()
                                .any(<dyn std::error::Error>::is::<std::io::Error>)
                            {
                                err
                            } else {
                                operational_error(
                                    diagnostic::codes::PARSE_ERROR,
                                    format!("{err:#}"),
                                    Some(entry_path.display().to_string()),
                                )
                            }
                        })
                        .with_context(|| {
                            format!("failed to parse package: {}", entry_path.display())
                        })?;
                    for (id, pkg) in &resolve.packages {
                        if !before.contains(&id) {
                            loaded.scanned.push((pkg.name.clone(), entry_path.clone()));
                            loaded.binary_entries.insert(id, entry_path.clone());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(loaded)
}

/// Record the interface imports and foreign world includes explicitly written in each root world
/// (including through local `include`s) before `wit-parser` world elaboration synthesizes
/// implicit dependency imports.
#[allow(clippy::type_complexity)]
fn collect_explicit_world_imports(
    package: &UnresolvedPackage,
) -> (
    BTreeMap<String, BTreeSet<ExplicitWorldImport>>,
    BTreeMap<String, Vec<ForeignWorldInclude>>,
) {
    let mut foreign_interfaces: BTreeMap<InterfaceId, ((String, String), String)> = BTreeMap::new();
    let mut foreign_worlds: BTreeMap<WorldId, ForeignWorldInclude> = BTreeMap::new();
    for (pkg_name, items) in &package.foreign_deps {
        for (item_name, (ast_item, _)) in items {
            match ast_item {
                AstItem::Interface(interface_id) => {
                    foreign_interfaces.insert(
                        *interface_id,
                        (
                            (pkg_name.namespace.clone(), pkg_name.name.clone()),
                            item_name.clone(),
                        ),
                    );
                }
                AstItem::World(world_id) => {
                    foreign_worlds.insert(
                        *world_id,
                        (
                            pkg_name.namespace.clone(),
                            pkg_name.name.clone(),
                            item_name.clone(),
                        ),
                    );
                }
            }
        }
    }
    let mut imports_by_world = BTreeMap::new();
    let mut foreign_includes_by_world = BTreeMap::new();
    for (world_id, world) in &package.worlds {
        if foreign_worlds.contains_key(&world_id) {
            continue;
        }
        let mut imports = BTreeSet::new();
        let mut included = Vec::new();
        let mut visited = BTreeSet::new();
        collect_world_explicit_imports(
            package,
            world_id,
            &foreign_interfaces,
            &foreign_worlds,
            &mut visited,
            &mut imports,
            &mut included,
        );
        imports_by_world.insert(world.name.clone(), imports);
        if !included.is_empty() {
            foreign_includes_by_world.insert(world.name.clone(), included);
        }
    }
    (imports_by_world, foreign_includes_by_world)
}

fn collect_world_explicit_imports(
    package: &UnresolvedPackage,
    world_id: WorldId,
    foreign_interfaces: &BTreeMap<InterfaceId, ((String, String), String)>,
    foreign_worlds: &BTreeMap<WorldId, ForeignWorldInclude>,
    visited: &mut BTreeSet<WorldId>,
    out_imports: &mut BTreeSet<ExplicitWorldImport>,
    out_foreign_worlds: &mut Vec<ForeignWorldInclude>,
) {
    if !visited.insert(world_id) {
        return;
    }
    let world = &package.worlds[world_id];
    for key in world.imports.keys() {
        if let WorldKey::Interface(interface_id) = key {
            if let Some((pkg, name)) = foreign_interfaces.get(interface_id) {
                out_imports.insert((Some(pkg.clone()), name.clone()));
            } else if let Some(name) = package.interfaces[*interface_id].name.as_ref() {
                out_imports.insert((None, name.clone()));
            }
        }
    }
    for include in &world.includes {
        if let Some(foreign) = foreign_worlds.get(&include.id) {
            if !out_foreign_worlds.contains(foreign) {
                out_foreign_worlds.push(foreign.clone());
            }
        } else {
            collect_world_explicit_imports(
                package,
                include.id,
                foreign_interfaces,
                foreign_worlds,
                visited,
                out_imports,
                out_foreign_worlds,
            );
        }
    }
}

/// Sort paths by their file name as raw bytes (UTF-8 bytes for valid UTF-8 names), never by
/// locale or OS directory-enumeration order.
fn sort_paths_by_filename_bytes(paths: &mut [PathBuf]) {
    paths.sort_by(|left, right| {
        let left_name = left
            .file_name()
            .map(std::ffi::OsStr::as_encoded_bytes)
            .unwrap_or_default();
        let right_name = right
            .file_name()
            .map(std::ffi::OsStr::as_encoded_bytes)
            .unwrap_or_default();
        left_name.cmp(right_name).then_with(|| {
            left.as_os_str()
                .as_encoded_bytes()
                .cmp(right.as_os_str().as_encoded_bytes())
        })
    });
}

fn sorted_directory_entries(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in
        fs::read_dir(dir).with_context(|| format!("failed to read directory {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read directory {}", dir.display()))?;
        paths.push(entry.path());
    }
    sort_paths_by_filename_bytes(&mut paths);
    Ok(paths)
}

fn sorted_wit_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry_path in sorted_directory_entries(dir)? {
        let ty = fs::symlink_metadata(&entry_path)
            .with_context(|| format!("failed to inspect {}", entry_path.display()))?
            .file_type();
        if ty.is_dir() || (ty.is_symlink() && entry_path.is_dir()) {
            continue;
        }
        if entry_path.extension().and_then(std::ffi::OsStr::to_str) != Some("wit") {
            continue;
        }
        files.push(entry_path);
    }
    Ok(files)
}

fn parse_sorted_dir(dir: &Path) -> Result<UnresolvedPackageGroup> {
    let files = sorted_wit_files(dir)?;
    let mut map = SourceMap::new();
    for file in &files {
        map.push_file(file)?;
    }
    map.parse().map_err(|(source_map, err)| {
        let span = err.kind().span();
        let resolved = resolve_span_location(&source_map, span);
        let rendered = err.render(&source_map);
        let message = if let Some(bom_file) = files.iter().find(|path| file_has_utf8_bom(path)) {
            format!(
                "{rendered} (note: {} starts with a UTF-8 byte-order mark (BOM))",
                bom_file.display()
            )
        } else {
            rendered
        };
        match resolved {
            Some((file, pos)) => {
                operational_error_at(diagnostic::codes::PARSE_ERROR, message, file, Some(pos))
            }
            None => operational_error(
                diagnostic::codes::PARSE_ERROR,
                message,
                Some(dir.display().to_string()),
            ),
        }
    })
}

fn parse_single_wit_file(path: &Path) -> Result<UnresolvedPackageGroup> {
    let mut map = SourceMap::new();
    map.push_file(path)?;
    map.parse()
        .map_err(|(source_map, err)| {
            let span = err.kind().span();
            let resolved = resolve_span_location(&source_map, span);
            let rendered = err.render(&source_map);
            let message = if file_has_utf8_bom(path) {
                format!(
                    "{rendered} (note: {} starts with a UTF-8 byte-order mark (BOM))",
                    path.display()
                )
            } else {
                rendered
            };
            match resolved {
                Some((file, pos)) => {
                    operational_error_at(diagnostic::codes::PARSE_ERROR, message, file, Some(pos))
                }
                None => operational_error(
                    diagnostic::codes::PARSE_ERROR,
                    message,
                    Some(path.display().to_string()),
                ),
            }
        })
        .with_context(|| format!("failed to parse package: {}", path.display()))
}

fn file_has_utf8_bom(path: &Path) -> bool {
    use std::io::Read as _;
    let mut prefix = [0u8; 3];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut prefix))
        .is_ok()
        && prefix == [0xEF, 0xBB, 0xBF]
}

/// Refuse two models whose pinned dependency graphs cannot share one generated binding surface.
///
/// This is the comparison the generators and `diff` apply. `check --against` uses it when the
/// baseline lockfile belongs to a version directory, so a dependency change cannot pass the
/// lockfile gate while every generator refuses the root.
///
/// # Errors
///
/// Returns a [`ContractError`] for the first changed package, version, interface, type,
/// function, declaration order, stability metadata, or docs.
pub fn ensure_same_dependency_graph(old: &model::Model, new: &model::Model) -> Result<()> {
    crate::emit::versions::ensure_same_dependency_graph(&old.dependencies, &new.dependencies)
}

/// The default released-version manifest name.
pub const RELEASED_MANIFEST: &str = "released";

/// The validated contents of a `wit/released` manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReleasedManifest {
    directories: Vec<String>,
    retired: BTreeSet<String>,
}

impl ReleasedManifest {
    /// Parse and validate a released manifest.
    ///
    /// Entries are plain directory names relative to the manifest's directory, optionally followed
    /// by ASCII spaces/tabs and the literal token `retired`:
    /// `^[A-Za-z0-9][A-Za-z0-9._-]*$` (`v0.1` or `v0.1 retired`). That excludes `.`, `..`, hidden
    /// names, whitespace inside names, separators, and absolute paths. Leading and trailing ASCII
    /// whitespace (space, tab, CR, VT, FF) is trimmed; blank lines and `#` comments are ignored.
    /// Invalid, duplicate, missing, symlinked, and lockfile-less directories are errors, as is any
    /// symlink inside a listed directory.
    ///
    /// # Errors
    ///
    /// Returns an error for an unreadable manifest, any invalid entry, or a symlinked directory
    /// or directory content.
    pub fn load(path: &Path) -> Result<Self> {
        let _ = inspect_manifest_path(path)?;
        let normalized: PathBuf = path.components().collect();
        let text = fs::read_to_string(&normalized)
            .with_context(|| format!("failed to read released manifest {}", path.display()))?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let parent = normalized.parent().unwrap_or_else(|| Path::new("."));
        let mut directories = Vec::new();
        let mut retired = BTreeSet::new();
        let mut seen = BTreeSet::new();
        for (index, raw) in text.lines().enumerate() {
            let line_no = index + 1;
            let line = raw.trim_matches([' ', '\t', '\r', '\x0b', '\x0c']);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((entry_name, is_retired)) = parse_manifest_entry(line) else {
                return Err(operational_error_at(
                    diagnostic::codes::PARSE_ERROR,
                    format!(
                        "line {line_no}: released entry `{line}` is not a plain directory name \
                         (expected `<name>` or `<name> retired` where `<name>` matches \
                         `^[A-Za-z0-9][A-Za-z0-9._-]*$`)"
                    ),
                    path.display().to_string(),
                    Some(line_position(line_no, raw)),
                ));
            };
            if !seen.insert(entry_name.to_string()) {
                return Err(operational_error_at(
                    diagnostic::codes::PARSE_ERROR,
                    format!("line {line_no}: duplicate released entry `{entry_name}`"),
                    path.display().to_string(),
                    Some(line_position(line_no, raw)),
                ));
            }
            let directory = parent.join(entry_name);
            if fs::symlink_metadata(&directory)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                return Err(operational_error(
                    diagnostic::codes::GUARD_REFUSED,
                    format!(
                        "line {line_no}: released directory `{entry_name}` is a symlink; witforge \
                         refuses symlinked version directories"
                    ),
                    Some(directory.display().to_string()),
                ));
            }
            if !directory.is_dir() {
                return Err(operational_error(
                    diagnostic::codes::IO_ERROR,
                    format!("line {line_no}: released directory `{entry_name}` does not exist"),
                    Some(directory.display().to_string()),
                ));
            }
            ensure_no_symlinks(&directory, entry_name)?;
            if !directory.join("wire.lock").is_file() {
                return Err(operational_error(
                    diagnostic::codes::IO_ERROR,
                    format!("line {line_no}: released directory `{entry_name}` has no wire.lock"),
                    Some(directory.join("wire.lock").display().to_string()),
                ));
            }
            directories.push(entry_name.to_string());
            if is_retired {
                retired.insert(entry_name.to_string());
            }
        }
        Ok(Self {
            directories,
            retired,
        })
    }

    /// All released directory names in manifest order (both live and retired).
    #[must_use]
    pub fn directories(&self) -> &[String] {
        &self.directories
    }

    /// Whether `directory` is listed (either live or retired).
    #[must_use]
    pub fn contains(&self, directory: &str) -> bool {
        self.directories.iter().any(|entry| entry == directory)
    }

    /// Whether `directory` is listed and marked `retired`.
    #[must_use]
    pub fn is_retired(&self, directory: &str) -> bool {
        self.retired.contains(directory)
    }

    /// Retired directory names in manifest order.
    #[must_use]
    pub fn retired_directories(&self) -> Vec<&str> {
        self.directories
            .iter()
            .filter(|entry| self.retired.contains(entry.as_str()))
            .map(String::as_str)
            .collect()
    }

    /// Live (non-retired) released directory names in manifest order.
    #[must_use]
    pub fn live_directories(&self) -> Vec<&str> {
        self.directories
            .iter()
            .filter(|entry| !self.retired.contains(entry.as_str()))
            .map(String::as_str)
            .collect()
    }
}

/// Parse one trimmed non-comment `released` manifest line as `(entry_name, is_retired)`.
///
/// Accepts `<name>` or `<name> retired` separated by ASCII spaces or tabs, where `<name>`
/// matches `^[A-Za-z0-9][A-Za-z0-9._-]*$`.
pub(crate) fn parse_manifest_entry(line: &str) -> Option<(&str, bool)> {
    let mut parts = line.split([' ', '\t']).filter(|part| !part.is_empty());
    let name = parts.next()?;
    if !plain_directory_name(name) {
        return None;
    }
    match (parts.next(), parts.next()) {
        (None, None) => Some((name, false)),
        (Some("retired"), None) => Some((name, true)),
        _ => None,
    }
}

/// Validate that retired versions in `manifest` form a contiguous oldest prefix of `versions`
/// (which must be sorted in ascending `(major, minor)` order) and leave at least one live version.
///
/// # Errors
///
/// Returns a [`ContractError`] (`WF1100`) if a live version precedes a retired version or if
/// every version under `wit_root` is retired.
pub fn validate_retirement_order(
    versions: &[(u64, u64, PathBuf)],
    manifest: &ReleasedManifest,
    wit_root: &Path,
) -> Result<()> {
    validate_retirement_order_with_manifest(
        versions,
        manifest,
        wit_root,
        &wit_root.join(RELEASED_MANIFEST),
    )
}

pub(crate) fn validate_retirement_order_with_manifest(
    versions: &[(u64, u64, PathBuf)],
    manifest: &ReleasedManifest,
    wit_root: &Path,
    manifest_path: &Path,
) -> Result<()> {
    let manifest_file = manifest_path.display().to_string();
    let mut first_live: Option<String> = None;
    for (_, _, dir) in versions {
        let name = dir
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default();
        if manifest.is_retired(name) {
            if let Some(ref older_live) = first_live {
                let diag = diagnostic::Diagnostic::error(
                    diagnostic::codes::LOCKFILE_ERROR,
                    format!(
                        "cannot retire `{name}` while older version `{older_live}` is still \
                         live; only the oldest versions of a WIT root may be retired"
                    ),
                )
                .with_path(manifest_file.clone());
                bail!(ContractError::with_located_diagnostic(
                    diagnostic::LocatedDiagnostic::from_diagnostic(diag).with_file(manifest_file)
                ));
            }
        } else if first_live.is_none() {
            first_live = Some(name.to_string());
        }
    }
    if first_live.is_none() && !versions.is_empty() {
        let diag = diagnostic::Diagnostic::error(
            diagnostic::codes::LOCKFILE_ERROR,
            format!(
                "every version directory under {} is retired; at least one live version is \
                 required",
                wit_root.display()
            ),
        )
        .with_path(manifest_file.clone());
        bail!(ContractError::with_located_diagnostic(
            diagnostic::LocatedDiagnostic::from_diagnostic(diag).with_file(manifest_file)
        ));
    }
    Ok(())
}

/// Whether `name` matches `^[A-Za-z0-9][A-Za-z0-9._-]*$`, the released-manifest entry rule shared
/// with [`freeze`].
pub(crate) fn plain_directory_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Refuse any symlink inside `directory` (recursively).
///
/// # Errors
///
/// Returns an operational error (`WF9004`) if any entry inside `directory` is a symlink, or an
/// I/O error if `directory` cannot be read.
pub fn ensure_no_symlinks_in_dir(directory: &Path, entry: &str) -> Result<()> {
    ensure_no_symlinks(directory, entry)
}

/// Discover every canonical `v<major>.<minor>` version directory directly under `wit_root`,
/// sorted in ascending `(major, minor)` order.
///
/// Refuses symlinked version directories, non-directory `v<major>.<minor>` entries, non-canonical
/// version-like names (`v00.1`, `v0.1.1`), duplicate versions, and a root with no version
/// directories.
///
/// # Errors
///
/// Returns an operational error (`WF9002` / `WF9004`) when `wit_root` cannot be read, holds no
/// version directories, or contains an invalid, duplicate, non-directory, or symlinked version
/// entry.
pub fn discover_version_directories(wit_root: &Path) -> Result<Vec<(u64, u64, PathBuf)>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(wit_root)
        .with_context(|| format!("failed to read WIT root {}", wit_root.display()))?
    {
        entries.push(entry?);
    }
    entries.sort_by(|left, right| {
        left.file_name()
            .as_encoded_bytes()
            .cmp(right.file_name().as_encoded_bytes())
    });
    let mut versions = Vec::new();
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((major, minor)) = parse_version_directory(&name)? else {
            continue;
        };
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to read version directory {}", path.display()))?;
        if file_type.is_symlink() {
            ensure_version_directory(&path)?;
        }
        if !file_type.is_dir() {
            return Err(operational_error(
                diagnostic::codes::GUARD_REFUSED,
                format!(
                    "version entry `{}` is not a directory; expected a `v<major>.<minor>` directory",
                    path.display()
                ),
                Some(path.display().to_string()),
            ));
        }
        versions.push((major, minor, path));
    }
    versions.sort_by_key(|(major, minor, _)| (*major, *minor));
    for pair in versions.windows(2) {
        if pair[0].0 == pair[1].0 && pair[0].1 == pair[1].1 {
            return Err(operational_error(
                diagnostic::codes::GUARD_REFUSED,
                format!(
                    "duplicate version directories {} and {}",
                    pair[0].2.display(),
                    pair[1].2.display()
                ),
                Some(pair[1].2.display().to_string()),
            ));
        }
    }
    if versions.is_empty() {
        return Err(operational_error(
            diagnostic::codes::IO_ERROR,
            format!(
                "no v<major>.<minor> version directories under {}",
                wit_root.display()
            ),
            Some(wit_root.display().to_string()),
        ));
    }
    Ok(versions)
}

/// Refuse any symlink inside a released version directory.
///
/// The freeze script checks the tracked history; this check also covers a working tree that was
/// modified after checkout. A released directory's contract, lockfile, and dependency files must
/// be the released bytes, not a link whose target can change outside the manifest's view.
fn ensure_no_symlinks(directory: &Path, entry: &str) -> Result<()> {
    for child in fs::read_dir(directory)
        .with_context(|| format!("failed to read released directory `{entry}`"))?
    {
        let child =
            child.with_context(|| format!("failed to read released directory `{entry}`"))?;
        let file_type = child
            .file_type()
            .with_context(|| format!("failed to inspect `{}`", child.path().display()))?;
        if file_type.is_symlink() {
            return Err(operational_error(
                diagnostic::codes::GUARD_REFUSED,
                format!(
                    "released directory `{entry}` contains symlink `{}`; a released version must \
                     contain regular files and directories",
                    child.path().display()
                ),
                Some(child.path().display().to_string()),
            ));
        }
        if file_type.is_dir() {
            ensure_no_symlinks(&child.path(), entry)?;
        }
    }
    Ok(())
}

/// The default manifest path for a version directory: `<wit-dir>/../released`.
///
/// The version directory is canonicalised first, so `.` and a symlinked WIT root name the
/// directory's real parent. A directory that cannot be resolved is used as given.
#[must_use]
pub fn default_released_manifest(wit_dir: &Path) -> PathBuf {
    let resolved = fs::canonicalize(wit_dir).unwrap_or_else(|_| wit_dir.to_path_buf());
    resolved
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(RELEASED_MANIFEST)
}

/// Refuse a version directory that is a symlink or not a directory.
///
/// witforge never follows a symlinked version directory: a released version reached through a
/// link could change through the link's target, which neither the released manifest beside the
/// link nor `ci/check-release-freeze.sh` would see. The WIT root and its parents may be symlinks.
/// Trailing separators and `.` components are ignored, so `v0.1/` is checked as `v0.1`.
///
/// # Errors
///
/// Returns an error when the directory is a symlink, is missing, or is not a directory.
pub fn ensure_version_directory(wit_dir: &Path) -> Result<()> {
    let path: PathBuf = wit_dir.components().collect();
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("failed to read version directory {}", wit_dir.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(operational_error(
            diagnostic::codes::GUARD_REFUSED,
            format!(
                "version directory `{}` is a symlink; witforge refuses symlinked version \
                 directories, so a released version cannot change through its link's target",
                wit_dir.display()
            ),
            Some(wit_dir.display().to_string()),
        ));
    }
    if !metadata.is_dir() {
        return Err(operational_error(
            diagnostic::codes::IO_ERROR,
            format!(
                "version directory `{}` is not a directory",
                wit_dir.display()
            ),
            Some(wit_dir.display().to_string()),
        ));
    }
    Ok(())
}

/// Refuse a version directory whose name spells another version than its package declares.
///
/// A directory named `v<major>.<minor>` must hold the package version `<major>.<minor>.0`, as
/// the generators require of every version directory; a directory with any other name is an
/// ad-hoc directory, such as a test fixture, and may hold any version. The name is read from
/// the resolved directory, so `.` stands for the directory it is.
///
/// # Errors
///
/// Returns a [`ContractError`] when the package version differs from the version the name
/// spells, and a plain error when the directory cannot be resolved or its name spells a version
/// with a sign or leading zeros (`v+0.1`, `v00.02`), which the generators refuse too.
pub fn ensure_directory_version(wit_dir: &Path, model: &model::Model) -> Result<()> {
    let resolved = fs::canonicalize(wit_dir)
        .with_context(|| format!("failed to resolve version directory {}", wit_dir.display()))?;
    let Some(name) = resolved.file_name().map(|name| name.to_string_lossy()) else {
        return Ok(());
    };
    let Some((major, minor)) = parse_version_directory(&name)? else {
        return Ok(());
    };
    let version = model.package.version;
    if version.major != major || version.minor != minor || version.patch != 0 {
        bail!(ContractError::with_diagnostic(
            diagnostic::Diagnostic::error(
                diagnostic::codes::DIRECTORY_VERSION_MISMATCH,
                format!(
                    "version directory {} declares package version {version}, expected \
                     {major}.{minor}.0; a new version is a new `v<major>.<minor>` directory",
                    wit_dir.display()
                ),
            )
            .with_path(wit_dir.display().to_string())
        ));
    }
    Ok(())
}

/// Load the manifest for a version directory.
///
/// A missing default manifest means no versions are released. An explicitly supplied missing
/// manifest is an error.
///
/// # Errors
///
/// Returns an error for an invalid manifest or an explicitly missing manifest.
pub fn load_released_manifest(wit_dir: &Path, explicit: Option<&Path>) -> Result<ReleasedManifest> {
    load_manifest_or_default(default_released_manifest(wit_dir), explicit)
}

/// Refuse a symlinked `wire.lock` path (including a dangling symlink or trailing separator).
///
/// Missing paths are ignored so callers can preserve their missing-lockfile hints and `--first`
/// behavior.
pub(crate) fn ensure_lockfile_not_symlink(path: &Path) -> Result<()> {
    let normalized: PathBuf = path.components().collect();
    if let Ok(meta) = fs::symlink_metadata(&normalized)
        && meta.file_type().is_symlink()
    {
        return Err(operational_error(
            diagnostic::codes::GUARD_REFUSED,
            format!("refusing to follow symlinked lockfile {}", path.display()),
            Some(path.display().to_string()),
        ));
    }
    Ok(())
}

/// Refuse an explicit `--released` manifest path whose parent directory is not `wit_root`.
///
/// Bare entry names inside a `released` manifest (such as `v0.1 retired`) are scoped to the
/// manifest's own directory; applying a manifest from another WIT root would silently retire or
/// treat matching version directory names in `wit_root` as released.
pub(crate) fn ensure_manifest_in_wit_root(manifest_path: &Path, wit_root: &Path) -> Result<()> {
    let manifest_file = manifest_path.display().to_string();
    let normalized: PathBuf = manifest_path.components().collect();
    if normalized.file_name().is_none() {
        return Err(operational_error(
            diagnostic::codes::GUARD_REFUSED,
            format!("released manifest `{manifest_file}` is not a file path"),
            Some(manifest_file),
        ));
    }
    let parent = normalized
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canon_parent = fs::canonicalize(parent).map_err(|err| {
        operational_error(
            diagnostic::codes::GUARD_REFUSED,
            format!(
                "released manifest `{manifest_file}` parent `{}` cannot be resolved: {err}",
                parent.display()
            ),
            Some(manifest_file.clone()),
        )
    })?;
    let canon_wit_root = fs::canonicalize(wit_root)
        .with_context(|| format!("failed to resolve WIT root {}", wit_root.display()))?;
    if canon_parent != canon_wit_root {
        return Err(operational_error(
            diagnostic::codes::GUARD_REFUSED,
            format!(
                "released manifest `{manifest_file}` belongs to `{}`, not `{}`",
                canon_parent.display(),
                canon_wit_root.display()
            ),
            Some(manifest_file),
        ));
    }
    Ok(())
}

/// Load the manifest for a WIT root, as the generators do; the default is `<wit-root>/released`.
///
/// A missing default manifest means no versions are released. An explicitly supplied missing
/// manifest is an error.
///
/// # Errors
///
/// Returns an error for an invalid manifest or an explicitly missing manifest.
pub fn load_root_released_manifest(
    wit_root: &Path,
    explicit: Option<&Path>,
) -> Result<ReleasedManifest> {
    load_manifest_or_default(wit_root.join(RELEASED_MANIFEST), explicit)
}

fn inspect_manifest_path(path: &Path) -> Result<Option<fs::Metadata>> {
    let normalized: PathBuf = path.components().collect();
    if normalized.file_name().is_none() {
        return Err(operational_error(
            diagnostic::codes::GUARD_REFUSED,
            format!("released manifest `{}` is not a file path", path.display()),
            Some(path.display().to_string()),
        ));
    }
    match fs::symlink_metadata(&normalized) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(operational_error(
                    diagnostic::codes::GUARD_REFUSED,
                    format!(
                        "released manifest `{}` is a symlink; commit a regular file",
                        path.display()
                    ),
                    Some(path.display().to_string()),
                ));
            }
            Ok(Some(meta))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => {
            Err(err).with_context(|| format!("failed to read released manifest {}", path.display()))
        }
    }
}

fn load_manifest_or_default(default: PathBuf, explicit: Option<&Path>) -> Result<ReleasedManifest> {
    let path = explicit.map_or(default, Path::to_path_buf);
    if explicit.is_none() && inspect_manifest_path(&path)?.is_none() {
        return Ok(ReleasedManifest::default());
    }
    ReleasedManifest::load(&path)
}

/// Refuse to lock a version directory listed in `wit/released`, or one that is a symlink.
///
/// The decision compares resolved paths, not names: `wit_dir` and every listed directory are
/// canonicalised, so `.` and `..` components and a symlinked WIT root cannot hide a released
/// directory. The `released` manifest beside the directory always applies, read both where the
/// directory is written (its lexical parent) and where it resolves to (its canonical parent); an
/// `explicit` manifest is read as well, never instead.
///
/// # Errors
///
/// Returns an error when the directory is a symlink, cannot be resolved, or is released, or a
/// manifest is invalid.
pub fn ensure_not_released(wit_dir: &Path, explicit: Option<&Path>) -> Result<()> {
    ensure_version_directory(wit_dir)?;
    let directory = fs::canonicalize(wit_dir)
        .with_context(|| format!("failed to resolve version directory {}", wit_dir.display()))?;
    if let Some(released) = released_directories(&[wit_dir], explicit)?
        .into_iter()
        .find(|released| released.directory == directory)
    {
        return Err(operational_error(
            diagnostic::codes::GUARD_REFUSED,
            format!(
                "version directory `{}` is released (listed as `{}` in {}) and cannot be locked",
                wit_dir.display(),
                released.entry,
                released.manifest.display()
            ),
            Some(wit_dir.display().to_string()),
        ));
    }
    Ok(())
}

/// Whether `wit_dir` is listed in the manifests [`ensure_not_released`] reads.
///
/// # Errors
///
/// Returns an error when the directory cannot be resolved or a manifest is invalid.
pub fn is_released(wit_dir: &Path, explicit: Option<&Path>) -> Result<bool> {
    let directory = fs::canonicalize(wit_dir)
        .with_context(|| format!("failed to resolve version directory {}", wit_dir.display()))?;
    Ok(released_directories(&[wit_dir], explicit)?
        .iter()
        .any(|released| released.directory == directory))
}

/// Refuse to write a lockfile into a released version directory, as [`ensure_not_released`]
/// refuses to lock one: `lock --lock <path>` must not rewrite a released `wire.lock`.
///
/// The lockfile's directory is canonicalised and compared with every directory released by the
/// manifests [`ensure_not_released`] reads for `wit_dir`, by the `released` manifest beside the
/// lockfile's own directory, and by the one beside each of its ancestors that is named like a
/// version directory (`v<major>.<minor>`), each ancestor taken both as written and as resolved.
/// So neither a lockfile in another WIT root, nor one in a subdirectory of a released directory,
/// nor an `explicit` manifest that omits the directory gets past the manifest that lists it.
///
/// # Errors
///
/// Returns an error when the lockfile's directory cannot be resolved, is or is inside a
/// released directory, or a manifest is invalid.
pub fn ensure_lockfile_not_released(
    lock: &Path,
    wit_dir: &Path,
    explicit: Option<&Path>,
) -> Result<()> {
    let parent = lock
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let resolved = fs::canonicalize(parent).with_context(|| {
        format!(
            "failed to resolve the directory of lockfile {}",
            lock.display()
        )
    })?;
    let ancestors = version_ancestors(parent, &resolved)?;
    let mut directories = vec![wit_dir, parent];
    directories.extend(ancestors.iter().map(PathBuf::as_path));
    if let Some(released) = released_directories(&directories, explicit)?
        .into_iter()
        .find(|released| resolved.starts_with(&released.directory))
    {
        return Err(operational_error(
            diagnostic::codes::GUARD_REFUSED,
            format!(
                "lockfile `{}` is in the released directory `{}` (listed as `{}` in {}) and \
                 cannot be written",
                lock.display(),
                released.directory.display(),
                released.entry,
                released.manifest.display()
            ),
            Some(lock.display().to_string()),
        ));
    }
    Ok(())
}

/// The ancestors of a directory, `written` (made absolute lexically) and `resolved`, that are
/// named like a version directory: `v`, ASCII digits, `.`, ASCII digits. Leading zeros are
/// accepted, so a non-canonical spelling of a listed directory is not passed over.
fn version_ancestors(written: &Path, resolved: &Path) -> Result<Vec<PathBuf>> {
    let written = lexical_absolute(written)?;
    let mut found: Vec<PathBuf> = Vec::new();
    for ancestor in written.ancestors().chain(resolved.ancestors()) {
        let versioned = ancestor
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .and_then(|name| name.strip_prefix('v'))
            .and_then(|rest| rest.split_once('.'))
            .is_some_and(|(major, minor)| {
                [major, minor].iter().all(|digits| {
                    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
                })
            });
        if versioned && !found.iter().any(|known| known == ancestor) {
            found.push(ancestor.to_path_buf());
        }
    }
    Ok(found)
}

/// A directory a released manifest lists, resolved.
struct ReleasedDirectory {
    /// The canonical directory.
    directory: PathBuf,
    /// The manifest entry that names it.
    entry: String,
    /// The manifest that lists it.
    manifest: PathBuf,
}

/// Every directory released by the `explicit` manifest and by the default manifest beside each
/// of `directories`, at its lexical and its canonical location. A missing default manifest
/// releases nothing.
fn released_directories(
    directories: &[&Path],
    explicit: Option<&Path>,
) -> Result<Vec<ReleasedDirectory>> {
    let mut manifests: Vec<PathBuf> = explicit.iter().map(|path| path.to_path_buf()).collect();
    for directory in directories {
        let mut defaults = vec![default_released_manifest(directory)];
        if let Some(parent) = lexical_absolute(directory)?.parent() {
            defaults.push(parent.join(RELEASED_MANIFEST));
        }
        for candidate in defaults {
            if inspect_manifest_path(&candidate)?.is_some() && !manifests.contains(&candidate) {
                manifests.push(candidate);
            }
        }
    }
    let mut released = Vec::new();
    for path in manifests {
        let manifest = ReleasedManifest::load(&path)?;
        let base = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        for entry in manifest.directories() {
            let listed = base.join(entry);
            let directory = fs::canonicalize(&listed).with_context(|| {
                format!("failed to resolve released directory {}", listed.display())
            })?;
            released.push(ReleasedDirectory {
                directory,
                entry: entry.clone(),
                manifest: path.clone(),
            });
        }
    }
    Ok(released)
}

/// `path` made absolute with its `.` and `..` components removed lexically, as written rather
/// than as the file system resolves it.
fn lexical_absolute(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)
        .with_context(|| format!("failed to resolve {}", path.display()))?;
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    Ok(normalized)
}

/// The version a `v<major>.<minor>` directory name spells, or `None` for any other name.
///
/// Both numbers must be unsigned 64-bit ASCII digits without a sign or leading zeros. Any name
/// starting with `v` or `V` followed by an ASCII digit (such as `v0.2.0`, `V0.2`, `v0.2-rc1`,
/// `v00.2`, `v0.02`, `v1`, `v1.`, or an overflowing number), or a signed spelling such as
/// `v+0.2`, is an error rather than a skipped entry so a mistyped version cannot silently drop
/// out of generated output.
pub(crate) fn parse_version_directory(name: &str) -> Result<Option<(u64, u64)>> {
    if let Some((major, minor)) = name.strip_prefix('v').and_then(|rest| rest.split_once('.'))
        && canonical_number(major)
        && canonical_number(minor)
        && let (Ok(major_number), Ok(minor_number)) = (major.parse::<u64>(), minor.parse::<u64>())
    {
        return Ok(Some((major_number, minor_number)));
    }
    if let Some((major, minor)) = name
        .strip_prefix(['v', 'V'])
        .and_then(|rest| rest.split_once('.'))
        && let (Ok(major_number), Ok(minor_number)) = (major.parse::<u64>(), minor.parse::<u64>())
    {
        return Err(operational_error(
            diagnostic::codes::GUARD_REFUSED,
            format!(
                "version directory `{name}` must be spelled `v{major_number}.{minor_number}`: ASCII \
                 digits without a sign or leading zeros"
            ),
            Some(name.to_string()),
        ));
    }
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'v' | b'V') && bytes[1].is_ascii_digit() {
        return Err(operational_error(
            diagnostic::codes::GUARD_REFUSED,
            format!(
                "invalid version entry `{name}`: expected `v<major>.<minor>` with unsigned 64-bit \
                 ASCII digits and no leading zeros, patch version, or suffix"
            ),
            Some(name.to_string()),
        ));
    }
    Ok(None)
}

fn canonical_number(digits: &str) -> bool {
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
}

/// Format `path` as one POSIX shell word that witforge reads back as the same path.
///
/// Unchanged when it holds only characters no shell treats specially, and single-quoted
/// otherwise. A relative path starting with `-` gets a leading `./` so the command line does not
/// read it as an option.
pub(crate) fn shell_quote(path: &Path) -> String {
    let mut text = path.to_string_lossy().into_owned();
    if text.starts_with('-') {
        text.insert_str(0, "./");
    }
    let plain = !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"%+,-./:=@_".contains(&byte));
    if plain {
        text
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// The highest sibling `v<major>.<minor>` version directory of `dir` in the same major, if any.
pub(crate) fn sibling_predecessor(dir: &Path) -> Option<(u64, u64, PathBuf)> {
    let canonical = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let name = canonical.file_name()?.to_str()?;
    let (major, minor) = parse_version_directory(name).ok()??;
    let parent = dir.parent().filter(|parent| !parent.as_os_str().is_empty());
    let scan_parent = parent.or_else(|| canonical.parent())?;
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in fs::read_dir(scan_parent).ok()?.flatten() {
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }
        let Some(entry_name) = entry_path.file_name().and_then(std::ffi::OsStr::to_str) else {
            continue;
        };
        let Ok(Some((sibling_major, sibling_minor))) = parse_version_directory(entry_name) else {
            continue;
        };
        if sibling_major == major
            && sibling_minor < minor
            && best
                .as_ref()
                .is_none_or(|(best_minor, _)| sibling_minor > *best_minor)
        {
            best = Some((sibling_minor, entry_path));
        }
    }
    let (prev_minor, prev_path) = best?;
    Some((major, prev_minor, prev_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_paths_by_filename_bytes_is_deterministic_byte_order() {
        let mut shuffled = vec![
            PathBuf::from("dir/z.wit"),
            PathBuf::from("dir/b.wit"),
            PathBuf::from("dir/B.wit"),
            PathBuf::from("dir/a.wit"),
            PathBuf::from("dir/10.wit"),
            PathBuf::from("dir/2.wit"),
            PathBuf::from("dir/é.wit"),
        ];
        sort_paths_by_filename_bytes(&mut shuffled);
        let names: Vec<&str> = shuffled
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        // Raw UTF-8 byte order: digits ('1' < '2'), uppercase ('B'), lowercase ('a' < 'b' < 'z'),
        // then non-ASCII UTF-8 bytes (0xc3 ...).
        assert_eq!(
            names,
            vec![
                "10.wit", "2.wit", "B.wit", "a.wit", "b.wit", "z.wit", "é.wit"
            ]
        );
    }

    #[test]
    fn sorted_load_preserves_source_positions_for_every_file() {
        let dir =
            std::env::temp_dir().join(format!("witforge-lib-sorted-spans-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Write `z.wit` before `a.wit`; `a.wit` must still receive earlier spans.
        fs::write(
            dir.join("z.wit"),
            "package ex:spans@0.1.0;\n\ninterface zeta {\n    record z { id: u32 }\n}\n",
        )
        .unwrap();
        fs::write(
            dir.join("a.wit"),
            "interface alpha {\n    record a { id: u32 }\n}\n",
        )
        .unwrap();
        let contract = load(&dir).unwrap();
        let pkg = &contract.resolve.packages[contract.package];
        let alpha_span = contract.resolve.interfaces[pkg.interfaces["alpha"]].span;
        let zeta_span = contract.resolve.interfaces[pkg.interfaces["zeta"]].span;
        assert!(alpha_span.start() < zeta_span.start());
        let alpha_loc = contract
            .resolve
            .source_map
            .resolve_span(alpha_span)
            .unwrap();
        let zeta_loc = contract.resolve.source_map.resolve_span(zeta_span).unwrap();
        assert!(alpha_loc.path.ends_with("a.wit"), "{}", alpha_loc.path);
        assert!(zeta_loc.path.ends_with("z.wit"), "{}", zeta_loc.path);
        let alpha_rendered = contract.resolve.render_location(alpha_span);
        let zeta_rendered = contract.resolve.render_location(zeta_span);
        assert!(alpha_rendered.contains("a.wit:1:"), "{alpha_rendered}");
        assert!(zeta_rendered.contains("z.wit:3:"), "{zeta_rendered}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn utf16_positions_handle_tabs_non_ascii_crlf_and_bom() {
        // Tab counts as 1 UTF-16 code unit.
        let tab = "\tfoo";
        assert_eq!(
            byte_range_to_utf16_position(tab, 1, 4),
            diagnostic::Position {
                line: 1,
                column: 2,
                end_line: 1,
                end_column: 5,
            }
        );
        // BMP character `é` is 2 UTF-8 bytes, 1 UTF-16 code unit.
        let bmp = "// café\nfoo";
        let foo_start = bmp.find("foo").unwrap();
        assert_eq!(
            byte_range_to_utf16_position(bmp, foo_start, foo_start + 3),
            diagnostic::Position {
                line: 2,
                column: 1,
                end_line: 2,
                end_column: 4,
            }
        );
        let bmp_inline = "éfoo";
        assert_eq!(
            byte_range_to_utf16_position(bmp_inline, 'é'.len_utf8(), bmp_inline.len()),
            diagnostic::Position {
                line: 1,
                column: 2,
                end_line: 1,
                end_column: 5,
            }
        );
        // Supplementary character `😀` is 4 UTF-8 bytes, 2 UTF-16 code units.
        let astral = "😀foo";
        assert_eq!(
            byte_range_to_utf16_position(astral, '😀'.len_utf8(), astral.len()),
            diagnostic::Position {
                line: 1,
                column: 3,
                end_line: 1,
                end_column: 6,
            }
        );
        // CRLF does not double-count line breaks or columns.
        let crlf = "abc\r\n\t😀xyz\r\n";
        let xyz_start = crlf.find("xyz").unwrap();
        assert_eq!(
            byte_range_to_utf16_position(crlf, xyz_start, xyz_start + 3),
            diagnostic::Position {
                line: 2,
                column: 4,
                end_line: 2,
                end_column: 7,
            }
        );
        // Leading UTF-8 BOM: error on BOM bytes (0..1) clamps safely to 1:1, and token after BOM starts at 1:1.
        let bom = "\u{feff}package foo;";
        assert_eq!(
            byte_range_to_utf16_position(bom, 0, 1),
            diagnostic::Position {
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 1,
            }
        );
        let pkg_start = bom.find("package").unwrap();
        assert_eq!(
            byte_range_to_utf16_position(bom, pkg_start, pkg_start + 7),
            diagnostic::Position {
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 8,
            }
        );
    }

    #[test]
    fn load_sources_resolves_parse_and_resolution_spans_from_memory() {
        // Parse error on line 3 in a virtual file that does not exist on disk.
        let parse_err = load_sources(vec![SourceFile::new(
            "virtual-only.wit".to_string(),
            "package ex:mem@0.1.0;\ninterface api {\n  record broken { x: }\n}\n".to_string(),
        )])
        .unwrap_err();
        let diags = error_located_diagnostics(&parse_err).expect("located parse diagnostic");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].file.as_deref(), Some("virtual-only.wit"));
        let pos = diags[0]
            .diagnostic
            .position
            .expect("parse error position from virtual buffer");
        assert_eq!(pos.line, 3, "{diags:?}");

        // Resolution error on line 4 (unknown imported type) resolves from virtual buffer,
        // never falling back to disk even when the virtual file does not exist on disk.
        let resolve_err = load_sources(vec![SourceFile::new(
            "virtual-resolve.wit".to_string(),
            "package ex:mem@0.1.0;\ninterface api {\n  record r {\n    x: nonexistent,\n  }\n}\n"
                .to_string(),
        )])
        .unwrap_err();
        let diags = error_located_diagnostics(&resolve_err).expect("located resolve diagnostic");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].file.as_deref(), Some("virtual-resolve.wit"));
        let pos = diags[0]
            .diagnostic
            .position
            .expect("resolution error position from virtual buffer");
        assert_eq!(pos.line, 4, "{diags:?}");

        // Shadowing check: `load_line_index` with `Some(virtual_sources)` must never read disk.
        let mut cache = BTreeMap::new();
        let mut virtual_sources = BTreeMap::new();
        virtual_sources.insert("Cargo.toml".to_string(), "line1\nline2\n".to_string());
        let index = load_line_index("Cargo.toml", &mut cache, Some(&virtual_sources)).unwrap();
        assert_eq!(index.body, "line1\nline2\n");
        assert!(
            load_line_index("README.md", &mut cache, Some(&virtual_sources)).is_none(),
            "memory-backed lookup must not read unrelated files from disk"
        );
    }
}
