//! Library-owned `check` and `lock` orchestration.
//!
//! A `build.rs` can validate and generate without spawning the CLI:
//!
//! ```no_run
//! use std::path::Path;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let wit_root = Path::new("wit");
//!     let report = witforge::workflow::check_root(wit_root, None)?;
//!     if !report.is_ok() {
//!         return Err(format!("contract check failed: {:#?}", report.findings).into());
//!     }
//!     witforge::emit::rust::generate(wit_root, Path::new("src/generated"))?;
//!     Ok(())
//! }
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::diagnostic::{
    Diagnostic, LocatedDiagnostic, Position, RelatedLocation, Severity, codes,
};
use crate::emit;
use crate::lock::{LockMode, PackageName, Reconciliation, WireLock};
use crate::model::{self, Model};
use crate::{
    ContractError, discover_version_directories, ensure_directory_version,
    ensure_lockfile_not_released, ensure_not_released, ensure_same_dependency_graph,
    ensure_version_directory, error_diagnostics, error_located_diagnostics, is_contract_error,
    is_released, load, load_released_manifest, load_root_released_manifest, operational_error,
    operational_error_at, shell_quote, sibling_predecessor, subset, validate_retirement_order,
};

/// Whether a path is one contract version, a version root, both, or neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckTargetKind {
    /// Direct WIT sources, `deps/`, or `wire.lock`, with no version children.
    VersionDirectory,
    /// One or more `v<major>.<minor>` children and no direct version sources.
    WitRoot,
    /// Both direct version sources and version children.
    Both,
    /// Neither direct version sources nor version children.
    Neither,
}

/// How an existing lockfile is classified during check or reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Baseline {
    /// Treat existing entries as released.
    #[default]
    Default,
    /// Treat existing entries as draft entries for a first version or new major.
    First,
    /// Treat entries in this older lockfile as released and later entries as drafts.
    Against(PathBuf),
}

/// Options shared by single-version [`check`] and [`lock`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct VersionOptions {
    /// Version directory containing WIT sources.
    pub directory: PathBuf,
    /// Lockfile path; defaults to `<directory>/wire.lock`.
    pub lockfile: Option<PathBuf>,
    /// Existing-entry classification.
    pub baseline: Baseline,
    /// Explicit released manifest; the default beside the version still applies.
    pub released: Option<PathBuf>,
}

impl VersionOptions {
    /// Create default-mode options for `directory`.
    #[must_use]
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            lockfile: None,
            baseline: Baseline::Default,
            released: None,
        }
    }

    /// Use an explicit lockfile path.
    #[must_use]
    pub fn with_lockfile(mut self, path: PathBuf) -> Self {
        self.lockfile = Some(path);
        self
    }

    /// Use `baseline` to classify existing entries.
    #[must_use]
    pub fn with_baseline(mut self, baseline: Baseline) -> Self {
        self.baseline = baseline;
        self
    }

    /// Read an additional explicit released manifest.
    #[must_use]
    pub fn with_released(mut self, path: PathBuf) -> Self {
        self.released = Some(path);
        self
    }

    fn against(&self) -> Option<&Path> {
        match &self.baseline {
            Baseline::Against(path) => Some(path),
            Baseline::Default | Baseline::First => None,
        }
    }

    fn first(&self) -> bool {
        matches!(self.baseline, Baseline::First)
    }
}

/// How a workflow finding is rendered in human output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FindingKind {
    /// An ordinary error: `error: <message>` on stderr.
    Error,
    /// A subset error: `error: <logical path>: <message>` on stderr.
    SubsetError,
    /// A non-failing note on stdout, suppressed by quiet mode.
    Note,
    /// A human-only summary error; structured output already has the individual diagnostics.
    HumanOnlyError,
}

/// One workflow diagnostic plus its byte-stable human rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Finding {
    /// Rendering category.
    pub kind: FindingKind,
    /// Exact human line, without its trailing newline.
    pub human: String,
    /// Structured diagnostic; absent only for a human-only summary.
    pub diagnostic: Option<LocatedDiagnostic>,
}

impl Finding {
    fn error(located: LocatedDiagnostic) -> Self {
        Self {
            kind: FindingKind::Error,
            human: format!("error: {}", located.diagnostic.message),
            diagnostic: Some(located),
        }
    }

    fn subset(located: LocatedDiagnostic) -> Self {
        let logical = located
            .logical
            .as_deref()
            .or(located.diagnostic.path.as_deref())
            .unwrap_or_default();
        Self {
            kind: FindingKind::SubsetError,
            human: format!("error: {logical}: {}", located.diagnostic.message),
            diagnostic: Some(located),
        }
    }

    fn note(human: String, located: LocatedDiagnostic) -> Self {
        Self {
            kind: FindingKind::Note,
            human,
            diagnostic: Some(located),
        }
    }

    fn human_only_error(message: &str) -> Self {
        Self {
            kind: FindingKind::HumanOnlyError,
            human: format!("error: {message}"),
            diagnostic: None,
        }
    }
}

/// Structured result of a check or lock workflow.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Report {
    /// Findings in deterministic emission order.
    pub findings: Vec<Finding>,
}

impl Report {
    /// Whether no error finding was reported.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.findings
            .iter()
            .all(|finding| finding.kind == FindingKind::Note)
    }

    fn error(&mut self, code: &'static str, message: String, path: Option<String>) {
        let mut diagnostic = Diagnostic::error(code, message);
        let file = path.clone();
        if let Some(path) = path {
            diagnostic = diagnostic.with_path(path);
        }
        let mut located = LocatedDiagnostic::from_diagnostic(diagnostic);
        if let Some(file) = file {
            located = located.with_file(file);
        }
        self.findings.push(Finding::error(located));
    }

    fn push_error(&mut self, located: LocatedDiagnostic) {
        self.findings.push(Finding::error(located));
    }

    fn merge(&mut self, other: Self) {
        self.findings.extend(other.findings);
    }
}

/// A lock reconciliation result that can be inspected before writing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LockReport {
    /// Structured diagnostics and human rendering metadata.
    pub report: Report,
    /// Destination lockfile path.
    pub lockfile: PathBuf,
    /// Canonical lockfile text when reconciliation was accepted.
    pub rendered: Option<String>,
}

impl LockReport {
    /// Whether reconciliation succeeded and canonical text is available.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.report.is_ok() && self.rendered.is_some()
    }

    /// Atomically write the reconciled lockfile.
    ///
    /// # Errors
    ///
    /// Returns an error when reconciliation was refused or the atomic write fails.
    pub fn write(&self) -> Result<()> {
        let Some(rendered) = &self.rendered else {
            return Err(ContractError::new("lockfile reconciliation was refused").into());
        };
        WireLock::write_rendered_atomic(&self.lockfile, rendered)
    }
}

/// Classify an existing path for the high-level check workflow.
///
/// # Errors
///
/// Returns an operational I/O error when the directory cannot be read.
pub fn classify_check_target(path: &Path) -> Result<CheckTargetKind> {
    let mut has_version_sources = false;
    let mut has_version_dirs = false;
    for entry in fs::read_dir(path)
        .with_context(|| format!("failed to read directory {}", path.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read directory {}", path.display()))?;
        let entry_path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "wire.lock" {
            has_version_sources = true;
            continue;
        }
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", entry_path.display()))?;
        let is_dir = file_type.is_dir() || (file_type.is_symlink() && entry_path.is_dir());
        let is_wit_file = !is_dir
            && entry_path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("wit"));
        if (is_dir && name == "deps") || is_wit_file {
            has_version_sources = true;
        } else if crate::parse_version_directory(&name).is_ok_and(|version| version.is_some()) {
            has_version_dirs = true;
        }
    }
    Ok(match (has_version_sources, has_version_dirs) {
        (true, false) => CheckTargetKind::VersionDirectory,
        (false, true) => CheckTargetKind::WitRoot,
        (true, true) => CheckTargetKind::Both,
        (false, false) => CheckTargetKind::Neither,
    })
}

/// Check one version directory without writing.
///
/// # Errors
///
/// Returns operational errors as `anyhow::Error`; contract errors retain [`ContractError`]
/// classification. Contract drift is collected in the returned report.
pub fn check(options: &VersionOptions) -> Result<Report> {
    check_with_loaded(options, None, true).map(|(report, _)| report)
}

#[allow(clippy::too_many_lines)]
fn check_with_loaded(
    options: &VersionOptions,
    released_override: Option<bool>,
    check_baseline_deps: bool,
) -> Result<(Report, Option<(crate::Contract, Model)>)> {
    ensure_version_directory(&options.directory)?;
    ensure_not_wit_root(&options.directory, "check")?;
    if released_override.is_none() {
        load_released_manifest(&options.directory, options.released.as_deref())?;
    }
    let contract = load(&options.directory)?;
    let violations = subset::check_located_diagnostics(&contract);
    if !violations.is_empty() {
        return Ok((
            Report {
                findings: violations.into_iter().map(Finding::subset).collect(),
            },
            None,
        ));
    }
    let member_locations = contract.member_locations();
    let model = model::build(&contract)?;
    ensure_directory_version(&options.directory, &model)?;
    ensure_first_is_allowed(options, "check")?;
    ensure_baseline_is_allowed(options, "check")?;
    let members = model.members()?;
    let lock_path = lock_path(options);
    let (lock, text) = read_lock(&lock_path, options)?;
    let lock_positions = text
        .as_deref()
        .map(WireLock::entry_positions)
        .unwrap_or_default();
    let (previous, against_positions) = read_previous(options.against())?;
    let mut result = Report::default();
    if check_baseline_deps && let Some(against) = options.against() {
        if let Some(baseline) = baseline_model(against)? {
            if let Err(error) = ensure_same_dependency_graph(&baseline, &model) {
                result.error(
                    codes::DEPENDENCY_GRAPH_MISMATCH,
                    format!("{error:#}"),
                    Some(against.display().to_string()),
                );
                return Ok((result, Some((contract, model))));
            }
        } else {
            let against_text = against.display().to_string();
            let diagnostic = Diagnostic::new(
                codes::LOCKFILE_BASELINE_NO_DEPS,
                Severity::Note,
                format!(
                    "cannot compare the pinned dependency graph: baseline `{against_text}` is not a version-directory wire.lock"
                ),
            )
            .with_path(against_text.clone());
            result.findings.push(Finding::note(
                format!(
                    "note: cannot compare the pinned dependency graph: baseline `{against_text}` is not the wire.lock of a version directory; run a generator over the WIT root"
                ),
                LocatedDiagnostic::from_diagnostic(diagnostic).with_file(against_text),
            ));
        }
    }
    let released = match released_override {
        Some(flag) => flag,
        None => is_released(&options.directory, options.released.as_deref())?,
    };
    let remedy = if released {
        Remedy::Released(options.directory.display().to_string())
    } else {
        Remedy::Lock(lock_command(options))
    };
    let package = package_name(&model);
    if !check_lockfile_packages(
        &package,
        &lock,
        &lock_path,
        &lock_positions,
        previous.as_ref(),
        options.against(),
        &against_positions,
        &mut result,
    ) {
        return Ok((result, Some((contract, model))));
    }
    let mode = lock_mode(options, previous.as_ref());
    let case_collisions: BTreeSet<(String, String)> = lock
        .case_collisions(&members, mode)
        .unwrap_or_default()
        .into_iter()
        .map(|diagnostic| (diagnostic.path.unwrap_or_default(), diagnostic.message))
        .collect();
    let reconciliation = match lock.drift_report_with(&members, mode) {
        Ok(report) => report,
        Err(error) => {
            result.error(
                codes::LOCKFILE_ERROR,
                format!("{error:#}"),
                Some(lock_path.display().to_string()),
            );
            return Ok((result, Some((contract, model))));
        }
    };
    let canonical = released
        || text
            .as_deref()
            .is_none_or(|text| lock.is_canonical(&text.replace("\r\n", "\n")));
    if !canonical {
        result.error(
            codes::LOCKFILE_NON_CANONICAL,
            format!(
                "lockfile {} is not in the form `witforge lock` writes; {}",
                lock_path.display(),
                remedy.advice(" to rewrite it")
            ),
            Some(lock_path.display().to_string()),
        );
    }
    if canonical
        && reconciliation.is_empty()
        && let Err(error) = lock.validate_order_section(&members, released)
    {
        result.error(
            codes::LOCKFILE_ERROR,
            format!(
                "{error:#}; {}",
                remedy.advice(" to update the order section")
            ),
            Some(lock_path.display().to_string()),
        );
        return Ok((result, Some((contract, model))));
    }
    if canonical
        && reconciliation.is_empty()
        && let Err(error) = emit::proto::emit_major(&model, &lock)
    {
        if is_contract_error(&error) {
            append_contract_error(&mut result, &error, Some(&lock_path));
            return Ok((result, Some((contract, model))));
        }
        return Err(error);
    }
    let lock_file = lock_path.display().to_string();
    let context = LockDiagnosticContext {
        member_locations: &member_locations,
        lock_file: &lock_file,
        lock_positions: &lock_positions,
        against_file: options.against().map(|path| path.display().to_string()),
        against_positions: &against_positions,
    };
    report_check_changes(
        &reconciliation,
        &case_collisions,
        options.against().is_some(),
        &remedy,
        &context,
        &mut result,
    );
    Ok((result, Some((contract, model))))
}

/// Check every version under one WIT root in semver order.
///
/// # Errors
///
/// Returns operational errors immediately and collects contract violations across versions.
#[allow(clippy::too_many_lines)]
pub fn check_root(wit_root: &Path, explicit_released: Option<&Path>) -> Result<Report> {
    let manifest = load_root_released_manifest(wit_root, explicit_released)?;
    let versions = discover_version_directories(wit_root)?;
    validate_retirement_order(&versions, &manifest, wit_root)?;
    let mut since_history = model::SinceHistory::new(
        versions
            .iter()
            .map(|(major, minor, _)| model::Version::new(*major, *minor, 0)),
    );
    let mut previous_by_major: BTreeMap<u64, PathBuf> = BTreeMap::new();
    let mut first_model: Option<Model> = None;
    let mut first_model_by_major: BTreeMap<u64, Model> = BTreeMap::new();
    let mut result = Report::default();
    for (major, _minor, directory) in versions {
        let name = directory
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default();
        let released = manifest.contains(name) || is_released(&directory, explicit_released)?;
        let predecessor = previous_by_major.get(&major).cloned();
        let baseline = if released {
            Baseline::Default
        } else if let Some(predecessor) = &predecessor {
            let predecessor_lock = predecessor.join("wire.lock");
            if !predecessor_lock.is_file() {
                result.error(
                    codes::LOCKFILE_ERROR,
                    format!(
                        "cannot check {} against predecessor {}: missing lockfile {}",
                        directory.display(),
                        predecessor.display(),
                        predecessor_lock.display()
                    ),
                    Some(predecessor_lock.display().to_string()),
                );
                previous_by_major.insert(major, directory);
                continue;
            }
            Baseline::Against(predecessor_lock)
        } else {
            Baseline::First
        };
        let mut options = VersionOptions::new(directory.clone()).with_baseline(baseline);
        if let Some(path) = explicit_released {
            options = options.with_released(path.to_path_buf());
        }
        let loaded = match check_with_loaded(&options, Some(released), false) {
            Ok((report, loaded)) => {
                result.merge(report);
                loaded
            }
            Err(error) if is_contract_error(&error) => {
                append_contract_error(&mut result, &error, None);
                None
            }
            Err(error) => return Err(error),
        };

        if let Some((contract, current_model)) = loaded {
            let same_package = match &first_model {
                Some(baseline)
                    if baseline.package.namespace != current_model.package.namespace
                        || baseline.package.name != current_model.package.name =>
                {
                    result.error(
                        codes::PACKAGE_MISMATCH,
                        format!(
                            "{} declares package {}:{}, expected {}:{}",
                            directory.display(),
                            current_model.package.namespace,
                            current_model.package.name,
                            baseline.package.namespace,
                            baseline.package.name
                        ),
                        Some(directory.display().to_string()),
                    );
                    false
                }
                Some(_) => true,
                None => {
                    first_model = Some(current_model.clone());
                    true
                }
            };
            if same_package {
                let stability_errors = since_history.check(&current_model);
                if !stability_errors.is_empty() {
                    let locations = contract.member_locations();
                    for (path, message) in stability_errors {
                        let mut diagnostic = Diagnostic::error(codes::SUBSET_ANNOTATION, message)
                            .with_path(path.clone());
                        if let Some((_, position)) = locations.get(&path) {
                            diagnostic = diagnostic.with_position(*position);
                        }
                        let mut located = LocatedDiagnostic::from_diagnostic(diagnostic)
                            .with_logical(path.clone());
                        if let Some((file, _)) = locations.get(&path) {
                            located = located.with_file(file.clone());
                        } else {
                            located = located.with_file(directory.display().to_string());
                        }
                        result.push_error(located);
                    }
                }
            }
            if let Some(baseline) = first_model_by_major.get(&major) {
                if let Err(error) = ensure_same_dependency_graph(baseline, &current_model) {
                    result.error(
                        codes::DEPENDENCY_GRAPH_MISMATCH,
                        format!("{error:#}"),
                        Some(directory.display().to_string()),
                    );
                }
            } else {
                first_model_by_major.insert(major, current_model);
            }
            if released
                && let Some(predecessor) = &predecessor
                && let Ok((current_lock, _)) = read_required_lock(&directory.join("wire.lock"))
                && let Ok((predecessor_lock, _)) =
                    read_required_lock(&predecessor.join("wire.lock"))
                && let Err(error) = current_lock.check_append_only(&predecessor_lock)
            {
                result.error(
                    codes::LOCKFILE_ERROR,
                    format!(
                        "{} does not continue {}: {error:#}",
                        directory.join("wire.lock").display(),
                        predecessor.join("wire.lock").display()
                    ),
                    Some(directory.join("wire.lock").display().to_string()),
                );
            }
        }
        previous_by_major.insert(major, directory);
    }
    Ok(result)
}

/// Validate a version directory as a release candidate inside its enclosing WIT root.
///
/// Validates both the single version directory (subset, lockfile canonical form and drift against
/// its same-major predecessor or `--first`, order section, and protobuf schema) and the enclosing
/// WIT-root invariants up through this version: package identity, `@since` history, same-major
/// dependency-graph identity, and retirement order in the target manifest.
///
/// When `explicit_released` is `Some(path)` and `path` does not exist yet, it is treated as an
/// empty manifest without falling back to the default `released` manifest beside `version_dir`.
///
/// # Errors
///
/// Returns operational errors immediately and collects contract violations into [`Report`].
#[allow(clippy::too_many_lines)]
pub fn check_release(version_dir: &Path, explicit_released: Option<&Path>) -> Result<Report> {
    ensure_version_directory(version_dir)?;
    let canonical_dir = fs::canonicalize(version_dir).with_context(|| {
        format!(
            "failed to resolve version directory {}",
            version_dir.display()
        )
    })?;
    let entry_name = canonical_dir
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_string)
        .ok_or_else(|| {
            operational_error(
                codes::CLI_USAGE,
                format!(
                    "`{}` is not a `v<major>.<minor>` version directory",
                    version_dir.display()
                ),
                Some(version_dir.display().to_string()),
            )
        })?;
    let Some((target_major, target_minor)) = version_from_dir(&canonical_dir) else {
        return Err(operational_error(
            codes::CLI_USAGE,
            format!(
                "`{}` is not a `v<major>.<minor>` version directory",
                version_dir.display()
            ),
            Some(version_dir.display().to_string()),
        ));
    };
    let manifest_path = explicit_released.map_or_else(
        || crate::default_released_manifest(version_dir),
        Path::to_path_buf,
    );
    let manifest = if manifest_path.exists() {
        crate::ReleasedManifest::load(&manifest_path)?
    } else {
        crate::ReleasedManifest::default()
    };
    let already_listed = manifest.contains(&entry_name);

    let wit_root = canonical_dir.parent().ok_or_else(|| {
        operational_error(
            codes::IO_ERROR,
            format!(
                "version directory `{}` has no enclosing WIT root",
                version_dir.display()
            ),
            Some(version_dir.display().to_string()),
        )
    })?;
    let versions = discover_version_directories(wit_root)?;
    validate_retirement_order(&versions, &manifest, wit_root)?;

    let predecessor = versions
        .iter()
        .filter(|(major, minor, _)| *major == target_major && *minor < target_minor)
        .max_by_key(|(_, minor, _)| *minor)
        .map(|(_, _, path)| path.clone());
    let baseline = if already_listed {
        Baseline::Default
    } else if let Some(pred) = &predecessor {
        Baseline::Against(pred.join("wire.lock"))
    } else {
        Baseline::First
    };
    let mut options = VersionOptions::new(version_dir.to_path_buf()).with_baseline(baseline);
    if manifest_path.exists() {
        options = options.with_released(manifest_path);
    }
    let (mut result, target_loaded) = check_with_loaded(&options, Some(already_listed), true)?;
    if !result.is_ok() {
        return Ok(result);
    }

    let mut since_history = model::SinceHistory::new(
        versions
            .iter()
            .filter(|(major, minor, _)| (*major, *minor) <= (target_major, target_minor))
            .map(|(major, minor, _)| model::Version::new(*major, *minor, 0)),
    );
    let mut first_model: Option<Model> = None;
    let mut first_model_by_major: BTreeMap<u64, Model> = BTreeMap::new();
    let mut target_loaded = target_loaded;

    for (major, minor, directory) in versions {
        if (major, minor) > (target_major, target_minor) {
            break;
        }
        let is_target = major == target_major && minor == target_minor;
        let (contract, current_model) = if is_target && let Some(loaded) = target_loaded.take() {
            loaded
        } else {
            let contract = load(&directory)?;
            let violations = subset::check_located_diagnostics(&contract);
            if !violations.is_empty() {
                for violation in violations {
                    result.findings.push(Finding::subset(violation));
                }
                continue;
            }
            let model = model::build(&contract)?;
            (contract, model)
        };

        let same_package = match &first_model {
            Some(baseline)
                if baseline.package.namespace != current_model.package.namespace
                    || baseline.package.name != current_model.package.name =>
            {
                result.error(
                    codes::PACKAGE_MISMATCH,
                    format!(
                        "{} declares package {}:{}, expected {}:{}",
                        directory.display(),
                        current_model.package.namespace,
                        current_model.package.name,
                        baseline.package.namespace,
                        baseline.package.name
                    ),
                    Some(directory.display().to_string()),
                );
                false
            }
            Some(_) => true,
            None => {
                first_model = Some(current_model.clone());
                true
            }
        };
        if same_package {
            let stability_errors = since_history.check(&current_model);
            if !stability_errors.is_empty() {
                let locations = contract.member_locations();
                for (path, message) in stability_errors {
                    let mut diagnostic = Diagnostic::error(codes::SUBSET_ANNOTATION, message)
                        .with_path(path.clone());
                    if let Some((_, position)) = locations.get(&path) {
                        diagnostic = diagnostic.with_position(*position);
                    }
                    let mut located =
                        LocatedDiagnostic::from_diagnostic(diagnostic).with_logical(path.clone());
                    if let Some((file, _)) = locations.get(&path) {
                        located = located.with_file(file.clone());
                    } else {
                        located = located.with_file(directory.display().to_string());
                    }
                    result.push_error(located);
                }
            }
        }
        if let Some(baseline) = first_model_by_major.get(&major) {
            if (!is_target || already_listed)
                && let Err(error) = ensure_same_dependency_graph(baseline, &current_model)
            {
                result.error(
                    codes::DEPENDENCY_GRAPH_MISMATCH,
                    format!("{error:#}"),
                    Some(directory.display().to_string()),
                );
            }
        } else {
            first_model_by_major.insert(major, current_model);
        }
    }

    Ok(result)
}

/// Reconcile one version directory and return the canonical lockfile without writing it.
///
/// Call [`LockReport::write`] only after inspecting the report. Released directories, subset
/// violations, and reconciliation discrepancies never produce a writable lockfile.
///
/// # Errors
///
/// Returns operational errors as `anyhow::Error`; contract refusals are collected in the report
/// when the CLI historically collected them, and otherwise retain [`ContractError`] classification.
#[allow(clippy::too_many_lines)]
pub fn lock(options: &VersionOptions) -> Result<LockReport> {
    ensure_not_released(&options.directory, options.released.as_deref())?;
    ensure_not_wit_root(&options.directory, "lock")?;
    let contract = load(&options.directory)?;
    let violations = subset::check_located_diagnostics(&contract);
    if !violations.is_empty() {
        let count = violations.len();
        let mut report = Report {
            findings: violations.into_iter().map(Finding::subset).collect(),
        };
        report.findings.push(Finding::human_only_error(&format!(
            "{count} subset violation(s); the lockfile was not updated"
        )));
        return Ok(LockReport {
            report,
            lockfile: lock_path(options),
            rendered: None,
        });
    }
    let member_locations = contract.member_locations();
    let model = model::build(&contract)?;
    ensure_directory_version(&options.directory, &model)?;
    let members = model.members()?;
    let lock_path = lock_path(options);
    ensure_lockfile_not_released(&lock_path, &options.directory, options.released.as_deref())?;
    ensure_first_is_allowed(options, "lock")?;
    ensure_baseline_is_allowed(options, "lock")?;
    let (mut wire_lock, existing_text) = read_lock(&lock_path, options)?;
    let lock_positions = existing_text
        .as_deref()
        .map(WireLock::entry_positions)
        .unwrap_or_default();
    let (previous, against_positions) = read_previous(options.against())?;
    let mut report = Report::default();
    let package = package_name(&model);
    if !check_lockfile_packages(
        &package,
        &wire_lock,
        &lock_path,
        &lock_positions,
        previous.as_ref(),
        options.against(),
        &against_positions,
        &mut report,
    ) {
        return Ok(LockReport {
            report,
            lockfile: lock_path,
            rendered: None,
        });
    }
    let mode = lock_mode(options, previous.as_ref());
    let case_collisions: BTreeSet<(String, String)> = wire_lock
        .case_collisions(&members, mode)
        .unwrap_or_default()
        .into_iter()
        .map(|diagnostic| (diagnostic.path.unwrap_or_default(), diagnostic.message))
        .collect();
    let reconciliation = match wire_lock.reconcile_report_with(&members, mode) {
        Ok(reconciliation) => reconciliation,
        Err(error) => {
            report.error(
                codes::LOCKFILE_ERROR,
                format!("{error:#}"),
                Some(lock_path.display().to_string()),
            );
            return Ok(LockReport {
                report,
                lockfile: lock_path,
                rendered: None,
            });
        }
    };
    let lock_file = lock_path.display().to_string();
    let context = LockDiagnosticContext {
        member_locations: &member_locations,
        lock_file: &lock_file,
        lock_positions: &lock_positions,
        against_file: options.against().map(|path| path.display().to_string()),
        against_positions: &against_positions,
    };
    let changes = &reconciliation.changes;
    for discrepancy in &reconciliation.discrepancies {
        let code =
            if case_collisions.contains(&(discrepancy.key.clone(), discrepancy.message.clone())) {
                codes::IDENTIFIER_COLLISION
            } else {
                codes::LOCKFILE_ERROR
            };
        report.push_error(context.locate_discrepancy(
            code,
            &discrepancy.key,
            discrepancy.message.clone(),
        ));
    }
    for (key, number) in &changes.reintroduced {
        report.push_error(context.locate_member(
            codes::LOCKFILE_REINTRODUCED,
            key,
            reintroduced(key, *number, options.against().is_some()),
            false,
            Some("reserved entry in wire.lock"),
        ));
    }
    if !report.is_ok() {
        report.findings.push(Finding::human_only_error(&format!(
            "{} lockfile discrepancy(s); the lockfile was not updated",
            reconciliation.discrepancies.len() + changes.reintroduced.len()
        )));
        return Ok(LockReport {
            report,
            lockfile: lock_path,
            rendered: None,
        });
    }

    let rendered = wire_lock.render();
    let written_positions = WireLock::entry_positions(&rendered);
    report_lock_notes(
        &reconciliation,
        &member_locations,
        &lock_file,
        &written_positions,
        &mut report,
    );
    Ok(LockReport {
        report,
        lockfile: lock_path,
        rendered: Some(rendered),
    })
}

fn append_contract_error(report: &mut Report, error: &anyhow::Error, fallback: Option<&Path>) {
    if let Some(located) = error_located_diagnostics(error) {
        for located in located {
            let mut located = located.clone();
            if located.file.is_none()
                && let Some(path) = fallback
            {
                located.file = Some(path.display().to_string());
            }
            if located.diagnostic.path.is_none()
                && let Some(path) = fallback
            {
                located.diagnostic = located.diagnostic.with_path(path.display().to_string());
            }
            report.push_error(located);
        }
    } else if let Some(diagnostics) = error_diagnostics(error) {
        for diagnostic in diagnostics {
            report.push_error(LocatedDiagnostic::from_diagnostic(diagnostic.clone()));
        }
    } else {
        report.error(
            codes::INTERNAL_ERROR,
            format!("{error:#}"),
            fallback.map(|path| path.display().to_string()),
        );
    }
}

fn package_name(model: &Model) -> PackageName {
    PackageName::new(model.package.namespace.clone(), model.package.name.clone())
}

fn lock_path(options: &VersionOptions) -> PathBuf {
    options
        .lockfile
        .clone()
        .unwrap_or_else(|| options.directory.join("wire.lock"))
}

fn lock_mode<'a>(options: &VersionOptions, previous: Option<&'a WireLock>) -> LockMode<'a> {
    match (&options.baseline, previous) {
        (Baseline::Against(_), Some(previous)) => LockMode::Against(previous),
        (Baseline::First, _) => LockMode::First,
        (Baseline::Default | Baseline::Against(_), _) => LockMode::Default,
    }
}

fn baseline_model(path: &Path) -> Result<Option<Model>> {
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    if path.file_name().and_then(|name| name.to_str()) != Some("wire.lock")
        || crate::parse_version_directory(
            parent
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default(),
        )?
        .is_none()
    {
        return Ok(None);
    }
    let contract = load(parent)
        .with_context(|| format!("failed to load the baseline version {}", parent.display()))?;
    let violations = subset::check_located_diagnostics(&contract);
    if !violations.is_empty() {
        let messages: Vec<String> = violations
            .iter()
            .map(|located| match &located.diagnostic.path {
                Some(path) => format!("{path}: {}", located.diagnostic.message),
                None => located.diagnostic.message.clone(),
            })
            .collect();
        return Err(ContractError::with_located_diagnostics(
            format!("{}: {}", parent.display(), messages.join("; ")),
            violations,
        )
        .into());
    }
    Ok(Some(model::build(&contract)?))
}

fn same_parent(left: Option<&Path>, right: Option<&Path>) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return false;
    };
    fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf())
        == fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf())
}

fn against_command(options: &VersionOptions, verb: &str, baseline: &Path) -> String {
    let mut command = format!("witforge {verb} {}", shell_quote(&options.directory));
    if let Some(lock) = &options.lockfile {
        let _ = write!(command, " --lock {}", shell_quote(lock));
    }
    let _ = write!(command, " --against {}", shell_quote(baseline));
    if let Some(released) = &options.released {
        let _ = write!(command, " --released {}", shell_quote(released));
    }
    command
}

fn ensure_first_is_allowed(options: &VersionOptions, verb: &str) -> Result<()> {
    if !options.first() {
        return Ok(());
    }
    let current =
        fs::canonicalize(&options.directory).unwrap_or_else(|_| options.directory.clone());
    let Some((major, predecessor_minor, predecessor)) = sibling_predecessor(&current) else {
        return Ok(());
    };
    let baseline = predecessor.join("wire.lock");
    Err(ContractError::with_diagnostic(
        Diagnostic::error(
            codes::LOCKFILE_ERROR,
            format!(
                "`--first` is refused for `{}`: major {major} already has {} at minor {predecessor_minor}, so a new minor continues that lockfile. Run `{}` instead",
                options.directory.display(),
                predecessor.display(),
                against_command(options, verb, &baseline)
            ),
        )
        .with_path(options.directory.display().to_string()),
    )
    .into())
}

fn ensure_baseline_is_allowed(options: &VersionOptions, verb: &str) -> Result<()> {
    let Some(against) = options.against() else {
        return Ok(());
    };
    if against.file_name().and_then(|name| name.to_str()) != Some("wire.lock") || !against.is_file()
    {
        return Ok(());
    }
    let current =
        fs::canonicalize(&options.directory).unwrap_or_else(|_| options.directory.clone());
    let Some((current_major, current_minor)) = version_from_dir(&current) else {
        return Ok(());
    };
    let Some(parent) = against.parent() else {
        return Ok(());
    };
    let Some((baseline_major, baseline_minor)) = version_from_dir(parent) else {
        return Ok(());
    };
    if !same_parent(current.parent(), parent.parent()) {
        return Ok(());
    }
    if baseline_major != current_major {
        return Err(ContractError::with_diagnostic(
            Diagnostic::error(
                codes::LOCKFILE_ERROR,
                format!(
                    "`--against {}` is the lockfile of major {baseline_major}, but `{}` is major {current_major}; a new major starts a fresh lockfile with `--first`. Run `witforge {verb} {} --first` instead",
                    against.display(),
                    options.directory.display(),
                    shell_quote(&options.directory)
                ),
            )
            .with_path(against.display().to_string()),
        )
        .into());
    }
    if baseline_minor < current_minor
        && let Some((_major, predecessor_minor, predecessor)) = sibling_predecessor(&current)
        && baseline_minor != predecessor_minor
    {
        return Err(ContractError::with_diagnostic(
            Diagnostic::error(
                codes::LOCKFILE_ERROR,
                format!(
                    "`--against {}` skips the immediate predecessor `{}`; run `{}` instead",
                    against.display(),
                    predecessor.display(),
                    against_command(options, verb, &predecessor.join("wire.lock"))
                ),
            )
            .with_path(against.display().to_string()),
        )
        .into());
    }
    Ok(())
}

fn version_from_dir(path: &Path) -> Option<(u64, u64)> {
    crate::parse_version_directory(path.file_name()?.to_str()?)
        .ok()
        .flatten()
}

fn read_previous(path: Option<&Path>) -> Result<(Option<WireLock>, BTreeMap<String, Position>)> {
    match path {
        Some(path) => {
            let (lock, positions) = read_required_lock(path)?;
            Ok((Some(lock), positions))
        }
        None => Ok((None, BTreeMap::new())),
    }
}

fn read_lock(path: &Path, options: &VersionOptions) -> Result<(WireLock, Option<String>)> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let lock = parse_lock(&text, path)?;
            Ok((lock, Some(text)))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if options.first() {
                return Ok((WireLock::default(), None));
            }
            if let Some(previous) = options.against() {
                return Err(operational_error(
                    codes::IO_ERROR,
                    format!(
                        "lockfile {} does not exist; a new minor starts as a copy of the previous minor's lockfile: copy {} to {} first, then rerun `{}`",
                        path.display(),
                        previous.display(),
                        path.display(),
                        lock_command(options)
                    ),
                    Some(path.display().to_string()),
                ));
            }
            let current =
                fs::canonicalize(&options.directory).unwrap_or_else(|_| options.directory.clone());
            if let Some((major, _, predecessor)) = sibling_predecessor(&current) {
                let previous = predecessor.join("wire.lock");
                if previous.is_file() {
                    return Err(operational_error(
                        codes::IO_ERROR,
                        format!(
                            "lockfile {path} does not exist; major {major} already has {previous_dir}, so copy {previous} to {path} and run `{against}` (or for a new major, create a fresh lockfile with `--first`)",
                            path = path.display(),
                            previous_dir = predecessor.display(),
                            previous = previous.display(),
                            against = against_command(options, "lock", &previous)
                        ),
                        Some(path.display().to_string()),
                    ));
                }
            }
            Err(operational_error(
                codes::IO_ERROR,
                format!(
                    "lockfile {path} does not exist; for the first version of a package or of a new major, create it with `{first}`; for a new minor, copy the previous minor's wire.lock to {path} and run witforge lock on that directory with --against pointing at the previous minor's wire.lock",
                    path = path.display(),
                    first = lock_command_with(options, Some("--first"))
                ),
                Some(path.display().to_string()),
            ))
        }
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn read_required_lock(path: &Path) -> Result<(WireLock, BTreeMap<String, Position>)> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let lock = parse_lock(&text, path)?;
    let positions = WireLock::entry_positions(&text);
    Ok((lock, positions))
}

fn parse_lock(text: &str, path: &Path) -> Result<WireLock> {
    WireLock::parse(text).map_err(|error| {
        let position = error_diagnostics(&error)
            .and_then(|diagnostics| diagnostics.first())
            .and_then(|diagnostic| diagnostic.position);
        operational_error_at(
            codes::PARSE_ERROR,
            format!("in {}: {error:#}", path.display()),
            path.display().to_string(),
            position,
        )
    })
}

fn ensure_not_wit_root(directory: &Path, verb: &str) -> Result<()> {
    let mut has_wit_files = false;
    let mut versions = Vec::new();
    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("wit"))
        {
            has_wit_files = true;
            break;
        }
        if path.is_dir()
            && let Some((major, minor)) = version_from_dir(&path)
            && let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str)
        {
            versions.push((major, minor, name.to_string(), path));
        }
    }
    if has_wit_files || versions.is_empty() {
        return Ok(());
    }
    versions.sort_by_key(|(major, minor, _, _)| (*major, *minor));
    let names: Vec<&str> = versions
        .iter()
        .map(|(_, _, name, _)| name.as_str())
        .collect();
    let example = &versions.last().expect("non-empty").3;
    Err(operational_error(
        codes::CLI_USAGE,
        format!(
            "`{}` is a WIT root containing version directories ({}), not a version directory; `{verb}` expects a single version directory (for example `witforge {verb} {}`)",
            directory.display(),
            names.join(", "),
            shell_quote(example)
        ),
        Some(directory.display().to_string()),
    ))
}

#[allow(clippy::too_many_arguments)]
fn check_lockfile_packages(
    package: &PackageName,
    lock: &WireLock,
    lock_path: &Path,
    lock_positions: &BTreeMap<String, Position>,
    previous: Option<&WireLock>,
    against_path: Option<&Path>,
    against_positions: &BTreeMap<String, Position>,
    report: &mut Report,
) -> bool {
    if let Err(error) = lock.check_package(package) {
        let file = lock_path.display().to_string();
        let position = lock
            .entries()
            .find(|(key, _)| key.package() != package)
            .and_then(|(key, _)| lock_positions.get(&key.key()).copied());
        let mut diagnostic = Diagnostic::error(codes::LOCKFILE_WRONG_PACKAGE, format!("{error:#}"))
            .with_path(file.clone());
        if let Some(position) = position {
            diagnostic = diagnostic.with_position(position);
        }
        report.push_error(LocatedDiagnostic::from_diagnostic(diagnostic).with_file(file));
        return false;
    }
    if let (Some(previous), Some(against_path)) = (previous, against_path)
        && let Err(error) = previous.check_package(package)
    {
        let file = against_path.display().to_string();
        let position = previous
            .entries()
            .find(|(key, _)| key.package() != package)
            .and_then(|(key, _)| against_positions.get(&key.key()).copied());
        let mut diagnostic = Diagnostic::error(codes::LOCKFILE_WRONG_PACKAGE, format!("{error:#}"))
            .with_path(file.clone());
        if let Some(position) = position {
            diagnostic = diagnostic.with_position(position);
        }
        report.push_error(LocatedDiagnostic::from_diagnostic(diagnostic).with_file(file));
        return false;
    }
    true
}

struct LockDiagnosticContext<'a> {
    member_locations: &'a BTreeMap<String, (String, Position)>,
    lock_file: &'a str,
    lock_positions: &'a BTreeMap<String, Position>,
    against_file: Option<String>,
    against_positions: &'a BTreeMap<String, Position>,
}

impl LockDiagnosticContext<'_> {
    fn lock_entry(&self, key: &str) -> (String, Option<Position>) {
        if let Some(position) = self.lock_positions.get(key).copied() {
            return (self.lock_file.to_string(), Some(position));
        }
        if let Some(against) = &self.against_file
            && let Some(position) = self.against_positions.get(key).copied()
        {
            return (against.clone(), Some(position));
        }
        (self.lock_file.to_string(), None)
    }

    fn locate_member(
        &self,
        code: &'static str,
        key: &str,
        message: String,
        prefer_lock: bool,
        related_message: Option<&str>,
    ) -> LocatedDiagnostic {
        let mut diagnostic = Diagnostic::error(code, message).with_path(key.to_string());
        let wit_location = self.member_locations.get(key);
        let (lock_file, lock_position) = self.lock_entry(key);
        let mut located = if prefer_lock {
            if let Some(position) = lock_position {
                diagnostic = diagnostic.with_position(position);
            }
            LocatedDiagnostic::from_diagnostic(diagnostic)
                .with_file(lock_file)
                .with_logical(key.to_string())
        } else if let Some((file, position)) = wit_location {
            diagnostic = diagnostic.with_position(*position);
            let mut item = LocatedDiagnostic::from_diagnostic(diagnostic)
                .with_file(file.clone())
                .with_logical(key.to_string());
            if let Some(message) = related_message {
                item = item.with_related(RelatedLocation::new(
                    lock_file,
                    lock_position,
                    message.to_string(),
                ));
            }
            item
        } else {
            if let Some(position) = lock_position {
                diagnostic = diagnostic.with_position(position);
            }
            LocatedDiagnostic::from_diagnostic(diagnostic)
                .with_file(lock_file)
                .with_logical(key.to_string())
        };
        if prefer_lock
            && let (Some((file, position)), Some(message)) = (wit_location, related_message)
        {
            located = located.with_related(RelatedLocation::new(
                file.clone(),
                Some(*position),
                message.to_string(),
            ));
        }
        located
    }

    fn locate_discrepancy(
        &self,
        code: &'static str,
        key: &str,
        message: String,
    ) -> LocatedDiagnostic {
        let mut diagnostic = Diagnostic::error(code, message).with_path(key.to_string());
        let wit_location = self.member_locations.get(key);
        let (lock_file, lock_position) = self
            .lock_positions
            .get(key)
            .copied()
            .map(|position| (self.lock_file.to_string(), Some(position)))
            .or_else(|| {
                self.lock_positions
                    .iter()
                    .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
                    .map(|(_, &position)| (self.lock_file.to_string(), Some(position)))
            })
            .or_else(|| {
                self.against_file.as_ref().and_then(|file| {
                    self.against_positions
                        .get(key)
                        .copied()
                        .or_else(|| {
                            self.against_positions
                                .iter()
                                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
                                .map(|(_, &position)| position)
                        })
                        .map(|position| (file.clone(), Some(position)))
                })
            })
            .unwrap_or_else(|| (self.lock_file.to_string(), None));
        if let Some((file, position)) = wit_location {
            diagnostic = diagnostic.with_position(*position);
            LocatedDiagnostic::from_diagnostic(diagnostic)
                .with_file(file.clone())
                .with_logical(key.to_string())
                .with_related(RelatedLocation::new(
                    lock_file,
                    lock_position,
                    "conflicting entry in wire.lock".to_string(),
                ))
        } else {
            if let Some(position) = lock_position {
                diagnostic = diagnostic.with_position(position);
            }
            LocatedDiagnostic::from_diagnostic(diagnostic)
                .with_file(lock_file)
                .with_logical(key.to_string())
        }
    }
}

enum Remedy {
    Lock(String),
    Released(String),
}

impl Remedy {
    fn advice(&self, purpose: &str) -> String {
        match self {
            Self::Lock(command) => format!("run `{command}`{purpose}"),
            Self::Released(directory) => format!(
                "`{directory}` is released, so `witforge lock` refuses it; restore its released WIT and wire.lock"
            ),
        }
    }
}

fn report_check_changes(
    reconciliation: &Reconciliation,
    case_collisions: &BTreeSet<(String, String)>,
    against: bool,
    remedy: &Remedy,
    context: &LockDiagnosticContext<'_>,
    report: &mut Report,
) {
    for discrepancy in &reconciliation.discrepancies {
        if case_collisions.contains(&(discrepancy.key.clone(), discrepancy.message.clone())) {
            report.push_error(context.locate_discrepancy(
                codes::IDENTIFIER_COLLISION,
                &discrepancy.key,
                discrepancy.message.clone(),
            ));
        } else {
            report.push_error(context.locate_discrepancy(
                codes::LOCKFILE_ERROR,
                &discrepancy.key,
                format!("{}; {}", discrepancy.message, remedy.advice("")),
            ));
        }
    }
    for (key, _) in &reconciliation.restored {
        report.push_error(context.locate_member(
            codes::LOCKFILE_RESTORABLE_DRAFT,
            key,
            format!(
                "draft removal of {key} can be restored; {}",
                remedy.advice("")
            ),
            false,
            Some("reserved entry in wire.lock"),
        ));
    }
    for (key, _) in &reconciliation.changes.added {
        report.push_error(context.locate_member(
            codes::LOCKFILE_MISSING_NUMBER,
            key,
            format!("{key} has no wire number; {}", remedy.advice("")),
            false,
            Some("wire.lock has no entry for this member"),
        ));
    }
    for (key, number) in &reconciliation.changes.reserved {
        report.push_error(context.locate_member(
            codes::LOCKFILE_UNRESERVED_REMOVAL,
            key,
            format!(
                "{key} = {number} is no longer in the contract; {}",
                remedy.advice(" to reserve it")
            ),
            true,
            None,
        ));
    }
    for (key, number) in &reconciliation.changes.reintroduced {
        report.push_error(context.locate_member(
            codes::LOCKFILE_REINTRODUCED,
            key,
            reintroduced(key, *number, against),
            false,
            Some("reserved entry in wire.lock"),
        ));
    }
    for (key, number) in &reconciliation.changes.dropped {
        report.push_error(context.locate_member(
            codes::LOCKFILE_STALE_DRAFT,
            key,
            format!(
                "stale draft {key} = {number}; {}",
                remedy.advice(" to drop it")
            ),
            true,
            None,
        ));
    }
    for (key, _, _) in &reconciliation.changes.updated {
        report.push_error(context.locate_member(
            codes::LOCKFILE_CHANGED_DRAFT,
            key,
            format!(
                "draft {key} changed kind or type; {}",
                remedy.advice(" to update it")
            ),
            false,
            Some("existing entry in wire.lock"),
        ));
    }
}

fn report_lock_notes(
    reconciliation: &Reconciliation,
    member_locations: &BTreeMap<String, (String, Position)>,
    lock_file: &str,
    lock_positions: &BTreeMap<String, Position>,
    report: &mut Report,
) {
    let mut emit = |code: &'static str, human: String, message: String, key: &str| {
        let lock_position = lock_positions.get(key).copied();
        let wit_location = member_locations.get(key);
        let located = if wit_location.is_some() {
            located_note(
                code,
                message,
                key.to_string(),
                wit_location,
                Some((lock_file.to_string(), lock_position, "lockfile entry")),
            )
        } else {
            let fallback = lock_position.map(|position| (lock_file.to_string(), position));
            let mut item = located_note(code, message, key.to_string(), fallback.as_ref(), None);
            if item.file.is_none() {
                item = item.with_file(lock_file.to_string());
            }
            item
        };
        report.findings.push(Finding::note(human, located));
    };
    for (key, number) in &reconciliation.restored {
        emit(
            codes::LOCK_RESTORED,
            format!("restored {key} = {number}"),
            format!("restored {key} = {number}"),
            key,
        );
    }
    for (key, number) in &reconciliation.changes.dropped {
        emit(
            codes::LOCK_DROPPED,
            format!("dropped  {key} = {number}"),
            format!("dropped {key} = {number}"),
            key,
        );
    }
    for (key, _, _) in &reconciliation.changes.updated {
        emit(
            codes::LOCK_UPDATED,
            format!("updated  {key}"),
            format!("updated {key}"),
            key,
        );
    }
    for (key, number) in &reconciliation.changes.reserved {
        emit(
            codes::LOCK_RESERVED,
            format!("reserved {key} = {number}"),
            format!("reserved {key} = {number}"),
            key,
        );
    }
    for (key, number) in &reconciliation.changes.added {
        emit(
            codes::LOCK_ADDED,
            format!("added    {key} = {number}"),
            format!("added {key} = {number}"),
            key,
        );
    }
}

fn located_note(
    code: &'static str,
    message: String,
    logical: String,
    physical: Option<&(String, Position)>,
    related: Option<(String, Option<Position>, &'static str)>,
) -> LocatedDiagnostic {
    let mut diagnostic = Diagnostic::new(code, Severity::Note, message).with_path(logical.clone());
    if let Some((_, position)) = physical {
        diagnostic = diagnostic.with_position(*position);
    }
    let mut located = LocatedDiagnostic::from_diagnostic(diagnostic).with_logical(logical);
    if let Some((file, _)) = physical {
        located = located.with_file(file.clone());
    }
    if let Some((file, position, message)) = related {
        located = located.with_related(RelatedLocation::new(file, position, message.to_string()));
    }
    located
}

fn lock_command(options: &VersionOptions) -> String {
    let mode = match &options.baseline {
        Baseline::First => Some("--first".to_string()),
        Baseline::Against(previous) => Some(format!("--against {}", shell_quote(previous))),
        Baseline::Default => None,
    };
    lock_command_with(options, mode.as_deref())
}

fn lock_command_with(options: &VersionOptions, mode: Option<&str>) -> String {
    let mut command = format!("witforge lock {}", shell_quote(&options.directory));
    if let Some(lock) = &options.lockfile {
        let _ = write!(command, " --lock {}", shell_quote(lock));
    }
    if let Some(mode) = mode {
        let _ = write!(command, " {mode}");
    }
    if let Some(released) = &options.released {
        let _ = write!(command, " --released {}", shell_quote(released));
    }
    command
}

fn reintroduced(key: &str, number: u32, against: bool) -> String {
    if against {
        format!(
            "{key} was removed and its name and number {number} stay reserved; give the member a new name"
        )
    } else {
        format!(
            "{key} was removed and its name and number {number} stay reserved; if the removal was a draft, use `--against` with the released baseline; otherwise give the member a new name"
        )
    }
}
