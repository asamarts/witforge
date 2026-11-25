//! Contract diffing and breaking-change classification.
//!
//! A diff compares two version directories, or the highest version under two WIT roots. It
//! reports source changes together with three independent compatibility views:
//!
//! - **wire identity** is the append-only lockfile's numbers, kinds, and canonical types. A
//!   renumbering, kind change, or type change breaks it; an addition or a reserved removal does
//!   not.
//! - **source** is the WIT/component shape. Adding, removing, retyping, or reordering anything
//!   that participates in a component type breaks it, as does a world import or export change.
//! - **generated API** is the Rust surface: record fields, variant cases, enum cases, flags,
//!   functions, parameters, and named type shapes. Adding a field, case, flag, function, or
//!   parameter is API-breaking even when it is wire-compatible, because generated structs and
//!   traits are exhaustive. Reordering named fields is not API-breaking; reordering function
//!   parameters is.
//!
//! Added record fields whose type has a default (`option<T>`, `list<T>`, `map<K, V>`, or an alias
//! or named definition of one) are marked `defaulted`: the generated old-to-new conversion can fill
//! them, but the source and generated API still change.
//!
//! Root-package docs are not compared. Stability changes are reported as non-breaking metadata
//! changes; dependency docs and stability remain part of the exact pinned graph and are refused
//! before local declarations are compared.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::diagnostic::{Diagnostic, LocatedDiagnostic, Position, RelatedLocation, codes};
use crate::emit::defaults::default_expr;
use crate::emit::versions::ensure_same_dependency_graph;
use crate::emit::{TypeRenderStyle, type_kind_name, type_shape};
use crate::lock::{PackageName, WireLock};
use crate::model::{
    Dependency, Interface, Member, Model, TypeDef, TypeDefKind, TypeExpr, Version, WorldInterface,
};
use crate::{
    ContractError, DiagnosticError, ensure_directory_version, ensure_version_directory,
    error_diagnostics, error_located_diagnostics, is_contract_error, line_position, load,
    operational_error, operational_error_at, parse_manifest_entry, subset,
};

/// One endpoint of a diff: the selected version directory and its identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The version directory that was compared.
    pub path: PathBuf,
    /// The package version.
    pub version: Version,
    /// The package name as `namespace:name`.
    pub package: String,
}

/// The kind of one contract change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// A declaration exists only in the new version.
    Added {
        /// Whether an added record field has a defaultable type.
        defaulted: bool,
    },
    /// A declaration exists only in the old version.
    Removed,
    /// Common members changed declaration order.
    Reordered {
        /// Their old relative order.
        before: Vec<String>,
        /// Their new relative order.
        after: Vec<String>,
    },
    /// A member or type kept its name but changed type.
    Retyped {
        /// The old type.
        from: String,
        /// The new type.
        to: String,
    },
    /// A named type kept its name but changed kind, such as record to enum.
    KindChanged {
        /// The old kind.
        from: String,
        /// The new kind.
        to: String,
    },
    /// A common member kept its kind and type but changed wire number.
    Renumbered {
        /// The old number.
        from: u32,
        /// The new number.
        to: u32,
    },
    /// WIT stability, introduction, or deprecation metadata changed.
    StabilityChanged {
        /// The previous metadata.
        from: String,
        /// The new metadata.
        to: String,
    },
}

impl ChangeKind {
    fn rank(&self) -> u8 {
        match self {
            Self::KindChanged { .. } => 0,
            Self::Retyped { .. } => 1,
            Self::Renumbered { .. } => 2,
            Self::Removed => 3,
            Self::Added { .. } => 4,
            Self::Reordered { .. } => 5,
            Self::StabilityChanged { .. } => 6,
        }
    }
}

/// The typed declaration category affected by a contract change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ChangeElement {
    /// A named interface.
    Interface,
    /// A named type.
    Type,
    /// A record field.
    RecordField,
    /// Record-field declaration order.
    RecordFieldOrder,
    /// A variant case.
    VariantCase,
    /// Variant-case declaration order.
    VariantCaseOrder,
    /// An enum case.
    EnumCase,
    /// Enum-case declaration order.
    EnumCaseOrder,
    /// A flags member.
    Flag,
    /// Flags declaration order.
    FlagOrder,
    /// A function parameter.
    Parameter,
    /// Parameter declaration order.
    ParameterOrder,
    /// A function result.
    FunctionResult,
    /// A protobuf method id.
    MethodId,
    /// An interface function.
    Function,
    /// Function declaration order.
    FunctionOrder,
    /// A world.
    World,
    /// A world import.
    WorldImport,
    /// World-import declaration order.
    WorldImportOrder,
    /// A world export.
    WorldExport,
    /// World-export declaration order.
    WorldExportOrder,
    /// A reserved lockfile entry.
    ReservedEntry,
}

impl ChangeElement {
    /// The stable human and JSON label for this declaration category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Interface => "interface",
            Self::Type => "type",
            Self::RecordField => "record field",
            Self::RecordFieldOrder => "record field order",
            Self::VariantCase => "variant case",
            Self::VariantCaseOrder => "variant case order",
            Self::EnumCase => "enum case",
            Self::EnumCaseOrder => "enum case order",
            Self::Flag => "flag",
            Self::FlagOrder => "flag order",
            Self::Parameter => "parameter",
            Self::ParameterOrder => "parameter order",
            Self::FunctionResult => "function result",
            Self::MethodId => "method id",
            Self::Function => "function",
            Self::FunctionOrder => "function order",
            Self::World => "world",
            Self::WorldImport => "world import",
            Self::WorldImportOrder => "world import order",
            Self::WorldExport => "world export",
            Self::WorldExportOrder => "world export order",
            Self::ReservedEntry => "reserved entry",
        }
    }

    const fn order(self) -> Self {
        match self {
            Self::RecordField => Self::RecordFieldOrder,
            Self::VariantCase => Self::VariantCaseOrder,
            Self::EnumCase => Self::EnumCaseOrder,
            Self::Flag => Self::FlagOrder,
            Self::Parameter => Self::ParameterOrder,
            Self::WorldImport => Self::WorldImportOrder,
            Self::WorldExport => Self::WorldExportOrder,
            other => other,
        }
    }
}

impl fmt::Display for ChangeElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One change and its three compatibility verdicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// What changed.
    pub kind: ChangeKind,
    /// The typed kind of declaration.
    pub element: ChangeElement,
    /// The WIT path, without the package name.
    pub path: String,
    /// The wire number when the change concerns a numbered member.
    pub number: Option<u32>,
    /// Whether the change breaks wire identity.
    pub wire_breaking: bool,
    /// Whether the change breaks the WIT/component source shape.
    pub source_breaking: bool,
    /// Whether the change breaks the generated Rust API.
    pub api_breaking: bool,
}

impl Change {
    /// Whether any compatibility view is broken.
    #[must_use]
    pub fn is_breaking(&self) -> bool {
        self.wire_breaking || self.source_breaking || self.api_breaking
    }
}

/// Counts over a [`DiffReport`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    /// Total changes.
    pub changes: usize,
    /// Changes breaking at least one view.
    pub breaking: usize,
    /// Changes breaking wire identity.
    pub wire_breaking: usize,
    /// Changes breaking the WIT/component source shape.
    pub source_breaking: usize,
    /// Changes breaking the generated Rust API.
    pub api_breaking: usize,
}

/// A complete comparison of two contract versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReport {
    /// The old endpoint.
    pub old: Endpoint,
    /// The new endpoint.
    pub new: Endpoint,
    /// Changes in deterministic path order.
    pub changes: Vec<Change>,
    /// Counts over `changes`.
    pub summary: Summary,
}

/// One record type whose new-to-old conversion is guaranteed to fail.
///
/// A newer-to-older conversion returns `ConversionError::MissingMember` for every input when the
/// older record requires a member the newer version removed and that member's type has no
/// default. For adjacent versions this is the generated `convert.rs` adapter when the emitter can
/// generate the pair; for other pairs it is the same contract-level consequence. This is stronger
/// than a removed variant or enum case, which fails only for values that use the removed case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionImpact {
    /// The record type's WIT path, such as `types/task`.
    pub type_path: String,
    /// The newer source version the conversion starts from.
    pub from_version: Version,
    /// The older target version the conversion cannot produce.
    pub to_version: Version,
    /// The older record member the newer version cannot supply.
    pub missing_member: String,
    /// The canonical WIT member path, such as `types/task.required`.
    pub missing_path: String,
}

/// One type conversion omitted across a major-version boundary.
///
/// Inside a major, an evolution that cannot be converted is refused by `witforge rust`. Across a
/// major boundary, `convert.rs` generates the adapters that still convert and omits the rest, per
/// direction, including any containing type transitively. Consumers write manual adapters for the
/// listed conversions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionOmission {
    /// The source contract version, such as `0.2.0`.
    pub from_version: Version,
    /// The target contract version, such as `1.0.0`.
    pub to_version: Version,
    /// The canonical WIT type path, such as `types/order`.
    pub type_path: String,
    /// Human explanation of why the conversion is omitted in this direction.
    pub reason: String,
}

/// Decode-compatibility verdict for one contract change under the protobuf binary decode profile
/// within a single major version.
///
/// The decode view models two processes speaking the same major's protobuf schema over binary
/// protobuf encoding (`prost` / generated glue / the non-Rust decode profile). Two different
/// majors have two distinct protobuf packages negotiated separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeVerdict {
    /// Whether a reader compiled against the newer version accepts protobuf binary messages
    /// encoded by a writer at the older version.
    pub new_reads_old: bool,
    /// Whether a reader compiled against the older version accepts protobuf binary messages
    /// encoded by a writer at the newer version.
    pub old_reads_new: bool,
}

impl DecodeVerdict {
    /// Both directions accept each other's messages.
    pub const COMPATIBLE: Self = Self {
        new_reads_old: true,
        old_reads_new: true,
    };

    /// Neither direction can safely decode the other's messages.
    pub const INCOMPATIBLE: Self = Self {
        new_reads_old: false,
        old_reads_new: false,
    };

    /// Whether either decode direction rejects messages under the decode profile.
    #[must_use]
    pub const fn is_breaking(self) -> bool {
        !self.new_reads_old || !self.old_reads_new
    }
}

/// Counts of decode-breaking changes over a [`DiffReport`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecodeSummary {
    /// Changes breaking at least one decode direction (`!new_reads_old || !old_reads_new`).
    pub breaking: usize,
    /// Changes where a newer reader rejects messages from an older writer (`!new_reads_old`).
    pub new_reads_old_breaking: usize,
    /// Changes where an older reader rejects messages from a newer writer (`!old_reads_new`).
    pub old_reads_new_breaking: usize,
}

/// Complete comparison result including conversion impacts, cross-major omissions, decode
/// compatibility verdicts (aligned 1:1 with `analysis.report.changes`), and physical source
/// locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullDiff {
    /// The compatibility report and guaranteed new-to-old conversion-impact notes.
    pub analysis: DiffAnalysis,
    /// Per-direction cross-major conversion omissions matching `convert.rs`.
    pub conversion_omissions: Vec<ConversionOmission>,
    /// Decode-compatibility verdict for each change in `analysis.report.changes`, in order.
    pub decode_verdicts: Vec<DecodeVerdict>,
    /// Counts over `decode_verdicts`.
    pub decode_summary: DecodeSummary,
    /// Physical source locations indexed by logical contract path.
    pub locations: BTreeMap<String, (String, Position)>,
}

/// A complete comparison plus its guaranteed conversion-impact notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffAnalysis {
    /// The compatibility report.
    pub report: DiffReport,
    /// Guaranteed new-to-old conversion failures, in deterministic type-path order.
    pub conversion_impacts: Vec<ConversionImpact>,
}

/// Compare two version directories, or the highest version under two WIT roots.
///
/// This is [`compare_with_impacts`] without collecting the conversion-impact notes.
///
/// # Errors
///
/// Returns an error for an unreadable path, an invalid WIT package or lockfile, a version
/// directory that does not match its package version, a stale or unlocked live member, two
/// different package names, or (within one major) mismatched dependency graphs. Contract
/// violations carry [`ContractError`].
pub fn compare(old: &Path, new: &Path) -> Result<DiffReport> {
    compare_report(old, new, false).map(|full| full.analysis.report)
}

/// Compare two version directories, or the highest version under two WIT roots, and report
/// guaranteed new-to-old conversion failures.
///
/// A path containing `wire.lock` is a version directory. Any other path is a WIT root, and
/// the WIT-root loader selects its highest live version. Both endpoints must declare the same
/// `namespace:name` and, within one major, resolve the identical pinned dependency graph.
///
/// The notes do not change any compatibility verdict or count. They name a shared record type
/// whose older version requires a member the newer version removed, when that member's type has
/// no default, so a newer-to-older conversion returns `MissingMember` for every input.
///
/// # Errors
///
/// Returns the same errors as [`compare`].
pub fn compare_with_impacts(old: &Path, new: &Path) -> Result<DiffAnalysis> {
    compare_report(old, new, true).map(|full| full.analysis)
}

/// Compare two version directories or WIT roots and return the per-direction cross-major
/// conversion omissions that require consumer code.
///
/// Within one major, returns an empty vector. Across a major boundary, returns the exact same set
/// listed in the top comment block of `convert.rs`.
///
/// # Errors
///
/// Returns the same errors as [`compare`].
pub fn compare_omissions(old: &Path, new: &Path) -> Result<Vec<ConversionOmission>> {
    compare_report(old, new, true).map(|full| full.conversion_omissions)
}

/// Compare two version directories or WIT roots and also return the physical source locations
/// (`(file, Position)`) for every logical contract path in the compared versions.
///
/// Paths present in the new contract resolve to the new contract's declaring file and position;
/// removed items resolve to the old contract's declaring file and position.
///
/// # Errors
///
/// Returns the same errors as [`compare`].
#[allow(clippy::type_complexity)]
pub fn compare_with_locations(
    old: &Path,
    new: &Path,
) -> Result<(DiffAnalysis, BTreeMap<String, (String, Position)>)> {
    let full = compare_report(old, new, true)?;
    Ok((full.analysis, full.locations))
}

/// Compare two version directories or WIT roots and return the compatibility analysis,
/// cross-major conversion omissions, and physical source locations.
///
/// # Errors
///
/// Returns the same errors as [`compare`].
#[allow(clippy::type_complexity)]
pub fn compare_with_omissions(
    old: &Path,
    new: &Path,
) -> Result<(
    DiffAnalysis,
    Vec<ConversionOmission>,
    BTreeMap<String, (String, Position)>,
)> {
    let full = compare_report(old, new, true)?;
    Ok((full.analysis, full.conversion_omissions, full.locations))
}

/// Compare two version directories or WIT roots and return every compatibility view: wire
/// identity, source, generated API, protobuf binary decode compatibility, conversion impacts,
/// cross-major conversion omissions, and physical source locations.
///
/// # Errors
///
/// Returns the same errors as [`compare`].
pub fn compare_full(old: &Path, new: &Path) -> Result<FullDiff> {
    compare_report(old, new, true)
}

/// Internal variant of [`compare_full`] with pre-validated released flags for both endpoints
/// (used by [`crate::emit::doc`] when an explicit `--released` manifest was already loaded).
pub(crate) fn compare_full_with_released_flags(
    old: &Path,
    old_released: bool,
    new: &Path,
    new_released: bool,
) -> Result<FullDiff> {
    compare_report_with_options(old, None, Some(old_released), new, Some(new_released), true)
}

/// Compare a version directory or WIT root on disk (`target`) against its committed Git tree at
/// revision `git_base`.
///
/// The tree at `git_base` is materialized directly from Git's object database into a private
/// temporary directory under `std::env::temp_dir()` (respecting `TMPDIR`) and removed before
/// returning, on both success and error paths. Symlinks and submodules in either the Git tree or
/// `target` are refused (`WF9004`), and missing revisions in shallow clones fail with `WF9004`
/// and `fetch-depth: 0` guidance.
///
/// Reported endpoint paths and physical locations for the base revision are rewritten to
/// `<git_base>:<repo-relative-path>` so no ephemeral temporary path appears in output.
///
/// # Errors
///
/// Returns an operational [`crate::DiagnosticError`] (exit `2` in the CLI) when `git_base` is
/// invalid, `git` is missing from `PATH`, `target` is outside any Git repository, `git_base` is
/// unavailable (including shallow clones), `target` did not exist as a directory at `git_base`, or
/// a symlink/submodule guard fires; returns contract errors as in [`compare`].
#[allow(clippy::too_many_lines)]
pub fn compare_git_base(git_base: &str, target: &Path) -> Result<FullDiff> {
    validate_git_base_arg(git_base)?;

    let norm_target: PathBuf = target.components().collect();
    let target_meta = fs::symlink_metadata(&norm_target).map_err(|err| {
        operational_error(
            codes::IO_ERROR,
            format!(
                "diff --git-base: failed to inspect target `{}`: {err}",
                target.display()
            ),
            Some(target.display().to_string()),
        )
    })?;
    if target_meta.file_type().is_symlink() {
        return Err(operational_error(
            codes::GUARD_REFUSED,
            format!(
                "diff --git-base: target `{}` is a symlink; pass the directory itself",
                target.display()
            ),
            Some(target.display().to_string()),
        ));
    }
    if !target_meta.is_dir() {
        return Err(operational_error(
            codes::IO_ERROR,
            format!(
                "diff --git-base: target `{}` is not a directory",
                target.display()
            ),
            Some(target.display().to_string()),
        ));
    }

    let abs_target = fs::canonicalize(&norm_target)
        .with_context(|| format!("failed to resolve target {}", target.display()))?;
    let top_output = run_git_diff(&abs_target, &["rev-parse", "--show-toplevel"])?;
    if !top_output.status.success() {
        return Err(operational_error(
            codes::GUARD_REFUSED,
            format!(
                "diff --git-base: `{}` is not inside a git repository",
                target.display()
            ),
            Some(target.display().to_string()),
        ));
    }
    let top_text = String::from_utf8_lossy(&top_output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    let repo = fs::canonicalize(Path::new(&top_text))
        .with_context(|| format!("failed to resolve git repository root `{top_text}`"))?;
    let rel_target = abs_target.strip_prefix(&repo).map_err(|_| {
        operational_error(
            codes::GUARD_REFUSED,
            format!(
                "diff --git-base: `{}` is not inside the repository {}",
                target.display(),
                repo.display()
            ),
            Some(target.display().to_string()),
        )
    })?;
    let rel_git = to_git_path(rel_target);
    ensure_no_symlinks_in_target_dir(&norm_target, rel_git.is_empty())?;

    let commit_spec = format!("{git_base}^{{commit}}");
    let rev_out = run_git_diff(&repo, &["rev-parse", "--verify", "--quiet", &commit_spec])?;
    if !rev_out.status.success() {
        let shallow_out = run_git_diff(&repo, &["rev-parse", "--is-shallow-repository"])?;
        if shallow_out.status.success()
            && String::from_utf8_lossy(&shallow_out.stdout).trim() == "true"
        {
            return Err(operational_error(
                codes::GUARD_REFUSED,
                format!(
                    "diff --git-base: revision `{git_base}` is not available in this shallow \
                     clone; fetch full history or the base ref (in GitHub Actions, set \
                     `fetch-depth: 0` on `actions/checkout`)"
                ),
                Some(target.display().to_string()),
            ));
        }
        return Err(operational_error(
            codes::CLI_USAGE,
            format!("diff --git-base: invalid revision `{git_base}`"),
            None,
        ));
    }
    let commit = String::from_utf8_lossy(&rev_out.stdout).trim().to_string();

    let display_base = if rel_git.is_empty() {
        format!("{git_base}:.")
    } else {
        format!("{git_base}:{rel_git}")
    };

    let tree_spec = if rel_git.is_empty() {
        format!("{commit}^{{tree}}")
    } else {
        verify_git_target_is_tree(&repo, &commit, git_base, &rel_git)?;
        format!("{commit}:{rel_git}")
    };

    let dir_name = abs_target
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| !name.is_empty() && !name.contains('/') && !name.contains('\\'))
        .unwrap_or("root");

    let temp_guard = TempDirGuard::create()?;
    let materialized_dir = temp_guard.path().join(dir_name);
    fs::create_dir_all(&materialized_dir).with_context(|| {
        format!(
            "failed to create temporary directory {}",
            materialized_dir.display()
        )
    })?;

    let mut blobs = collect_git_tree_blobs(&repo, &tree_spec, &display_base, rel_git.is_empty())?;
    let writes: Vec<(PathBuf, String)> = blobs
        .drain(..)
        .map(|(rel, oid)| (materialized_dir.join(rel), oid))
        .collect();
    materialize_git_blobs(&repo, &writes)?;

    // When diffing a single version directory (`wit/v0.2`), validate any sibling `released`
    // manifest directly against `<commit>` in Git (rather than writing a partial `<temp>/released`
    // beside a lone `<temp>/v0.2` where sibling entries like `v0.1` are absent) and pass the
    // resulting released flag directly into `load_endpoint`.
    let old_released_override = if !rel_git.is_empty()
        && (materialized_dir.join("wire.lock").is_file()
            || looks_like_version_directory(&materialized_dir))
    {
        let (parent_dir_git, parent_released) = match rel_git.rsplit_once('/') {
            Some((parent, _)) => (Some(parent), format!("{parent}/released")),
            None => (None, "released".to_string()),
        };
        let is_rel = validate_git_sibling_released(
            &repo,
            &commit,
            git_base,
            parent_dir_git,
            &parent_released,
            dir_name,
        )?;
        Some(is_rel)
    } else {
        None
    };

    let mut full = match compare_report_with_options(
        &materialized_dir,
        Some(&display_base),
        old_released_override,
        target,
        None,
        true,
    ) {
        Ok(full) => full,
        Err(err) => {
            return Err(redact_git_base_error(
                &err,
                &materialized_dir,
                temp_guard.path(),
                git_base,
                &rel_git,
            ));
        }
    };
    if let Some(rewritten) = rewrite_git_display_path(
        &full.analysis.report.old.path,
        &materialized_dir,
        git_base,
        &rel_git,
    ) {
        full.analysis.report.old.path = PathBuf::from(rewritten);
    }
    for (file, _) in full.locations.values_mut() {
        if let Some(rewritten) =
            rewrite_git_display_path(Path::new(file), &materialized_dir, git_base, &rel_git)
        {
            *file = rewritten;
        }
    }
    Ok(full)
}

/// Build the report, optionally collecting conversion-impact notes, cross-major omissions, and
/// source locations.
fn compare_report(old_arg: &Path, new_arg: &Path, collect_impacts: bool) -> Result<FullDiff> {
    compare_report_with_options(old_arg, None, None, new_arg, None, collect_impacts)
}

#[allow(clippy::too_many_lines)]
fn compare_report_with_options(
    old_arg: &Path,
    old_label: Option<&str>,
    old_released_override: Option<bool>,
    new_arg: &Path,
    new_released_override: Option<bool>,
    collect_impacts: bool,
) -> Result<FullDiff> {
    let old_display = old_label.map_or_else(|| old_arg.display().to_string(), str::to_string);
    let old = load_endpoint_with_options(old_arg, old_released_override)
        .with_context(|| format!("failed to read the old contract {old_display}"))?;
    let new = load_endpoint_with_options(new_arg, new_released_override)
        .with_context(|| format!("failed to read the new contract {}", new_arg.display()))?;
    if old.endpoint.package != new.endpoint.package {
        let mut diag = Diagnostic::error(
            codes::PACKAGE_MISMATCH,
            format!(
                "cannot diff package {} against {}: the package names differ",
                old.endpoint.package, new.endpoint.package
            ),
        )
        .with_path(new_arg.display().to_string());
        let located = if let Some((file, pos)) = new.locations.get(&new.endpoint.package) {
            diag = diag.with_position(*pos);
            LocatedDiagnostic::from_diagnostic(diag)
                .with_file(file.clone())
                .with_logical(new.endpoint.package.clone())
        } else {
            LocatedDiagnostic::from_diagnostic(diag).with_file(new_arg.display().to_string())
        };
        bail!(ContractError::with_located_diagnostic(located));
    }
    if new.endpoint.version < old.endpoint.version {
        let message = if let Some(label) = old_label {
            format!(
                "cannot diff version {} (at `{label}`) against older version {} (at `{}`)",
                old.endpoint.version,
                new.endpoint.version,
                new_arg.display(),
            )
        } else {
            format!(
                "cannot diff version {} against older version {}; pass the older contract first \
                 (run `witforge diff {} {}`)",
                old.endpoint.version,
                new.endpoint.version,
                crate::shell_quote(new_arg),
                crate::shell_quote(old_arg),
            )
        };
        return Err(operational_error(codes::CLI_USAGE, message, None));
    }
    if old.model.package.version.major == new.model.package.version.major {
        ensure_same_dependency_graph(&old.model.dependencies, &new.model.dependencies)?;
    }
    let mut builder = Builder {
        old: &old,
        new: &new,
        changes: Vec::new(),
    };
    builder.compare_interfaces()?;
    builder.compare_worlds();
    builder.compare_reserved_entries();
    let mut entries = builder.changes;
    entries.sort_by(|(left, _), (right, _)| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.kind.rank().cmp(&right.kind.rank()))
            .then_with(|| left.element.cmp(&right.element))
    });
    let (changes, decode_verdicts): (Vec<Change>, Vec<DecodeVerdict>) = entries.into_iter().unzip();
    let summary = summarize(&changes);
    let decode_summary = summarize_decode(&decode_verdicts);
    let conversion_omissions = if collect_impacts {
        crate::emit::convert::analyze_cross_major_omissions(&old.model, &new.model)?
    } else {
        Vec::new()
    };
    let reverse_omitted: BTreeSet<&str> = conversion_omissions
        .iter()
        .filter(|omission| {
            omission.from_version == new.model.package.version
                && omission.to_version == old.model.package.version
        })
        .map(|omission| omission.type_path.as_str())
        .collect();
    let conversion_impacts = if collect_impacts {
        conversion_impacts(&old, &new, &reverse_omitted)?
    } else {
        Vec::new()
    };
    let pkg_prefix = format!("{}/", old.endpoint.package);
    let mut locations = old.locations;
    locations.extend(new.locations);
    for (key, pos) in &new.lock_positions {
        let logical = key.strip_prefix(&pkg_prefix).unwrap_or(key).to_string();
        locations
            .entry(logical)
            .or_insert_with(|| (new.lock_file.clone(), *pos));
    }
    for (key, pos) in &old.lock_positions {
        let logical = key.strip_prefix(&pkg_prefix).unwrap_or(key).to_string();
        locations
            .entry(logical)
            .or_insert_with(|| (old.lock_file.clone(), *pos));
    }
    Ok(FullDiff {
        analysis: DiffAnalysis {
            report: DiffReport {
                old: old.endpoint,
                new: new.endpoint,
                changes,
                summary,
            },
            conversion_impacts,
        },
        conversion_omissions,
        decode_verdicts,
        decode_summary,
        locations,
    })
}

fn summarize_decode(verdicts: &[DecodeVerdict]) -> DecodeSummary {
    DecodeSummary {
        breaking: verdicts.iter().filter(|v| v.is_breaking()).count(),
        new_reads_old_breaking: verdicts.iter().filter(|v| !v.new_reads_old).count(),
        old_reads_new_breaking: verdicts.iter().filter(|v| !v.old_reads_new).count(),
    }
}

/// Whether omitting a field or parameter of type `ty` from protobuf wire bytes is accepted by the
/// decode profile (and generated Rust glue) without returning `MissingField`.
///
/// Only `option<T>` (including `option<list<u8>>`) and non-byte `list<T>` (emitted as `repeated`)
/// accept absence on the wire. Required `list<u8>` (including alias chains resolving to `u8`) is
/// emitted as proto3 `optional bytes`, whose absence is rejected with `MissingField`.
fn is_absent_ok_on_wire(model: &Model, ty: &TypeExpr) -> bool {
    is_absent_ok_in(model, None, ty, 0)
}

fn is_absent_ok_in(
    model: &Model,
    context: Option<&Dependency>,
    ty: &TypeExpr,
    depth: usize,
) -> bool {
    if depth > crate::model::MAX_ALIAS_CHAIN {
        return false;
    }
    match ty {
        TypeExpr::Option(_) | TypeExpr::Map(..) => true,
        TypeExpr::List(elem) => !resolves_to_u8_in(model, context, elem, 0),
        TypeExpr::Named(name) => {
            let Some((resolved, def)) = model.type_def_in(context, name) else {
                return false;
            };
            match &def.kind {
                TypeDefKind::Option(_) | TypeDefKind::Map(..) => true,
                TypeDefKind::List(elem) => !resolves_to_u8_in(model, resolved, elem, 0),
                TypeDefKind::Alias(target) => is_absent_ok_in(model, resolved, target, depth + 1),
                TypeDefKind::Record(_)
                | TypeDefKind::Variant(_)
                | TypeDefKind::Enum(_)
                | TypeDefKind::Flags(_)
                | TypeDefKind::FixedLengthList(..)
                | TypeDefKind::Result(_) => false,
            }
        }
        _ => false,
    }
}

fn resolves_to_u8_in(
    model: &Model,
    context: Option<&Dependency>,
    ty: &TypeExpr,
    depth: usize,
) -> bool {
    if depth > crate::model::MAX_ALIAS_CHAIN {
        return false;
    }
    match ty {
        TypeExpr::U8 => true,
        TypeExpr::Named(name) => {
            let Some((resolved, def)) = model.type_def_in(context, name) else {
                return false;
            };
            match &def.kind {
                TypeDefKind::Alias(target) => resolves_to_u8_in(model, resolved, target, depth + 1),
                _ => false,
            }
        }
        _ => false,
    }
}

fn conversion_impacts(
    old: &Loaded,
    new: &Loaded,
    reverse_omitted: &BTreeSet<&str>,
) -> Result<Vec<ConversionImpact>> {
    let mut impacts = Vec::new();
    for old_interface in &old.model.interfaces {
        let Some(new_interface) = new.model.interface(&old_interface.name) else {
            continue;
        };
        for old_type in &old_interface.types {
            let type_path = format!("{}/{}", old_interface.name, old_type.name);
            if reverse_omitted.contains(type_path.as_str()) {
                continue;
            }
            let Some(new_type) = new_interface
                .types
                .iter()
                .find(|new_type| new_type.name == old_type.name)
            else {
                continue;
            };
            let (TypeDefKind::Record(old_fields), TypeDefKind::Record(new_fields)) =
                (&old_type.kind, &new_type.kind)
            else {
                continue;
            };
            let mut missing = None;
            for old_field in old_fields {
                if new_fields
                    .iter()
                    .any(|new_field| new_field.name == old_field.name)
                {
                    continue;
                }
                if default_expr(&old.model, &old_field.ty)?.is_none() {
                    missing = Some(old_field);
                    break;
                }
            }
            if let Some(field) = missing {
                impacts.push(ConversionImpact {
                    type_path: type_path.clone(),
                    from_version: new.model.package.version,
                    to_version: old.model.package.version,
                    missing_member: field.name.clone(),
                    missing_path: format!("{type_path}.{}", field.name),
                });
            }
        }
    }
    impacts.sort_by(|left, right| {
        left.type_path
            .cmp(&right.type_path)
            .then_with(|| left.missing_member.cmp(&right.missing_member))
    });
    Ok(impacts)
}

fn summarize(changes: &[Change]) -> Summary {
    Summary {
        changes: changes.len(),
        breaking: changes.iter().filter(|change| change.is_breaking()).count(),
        wire_breaking: changes.iter().filter(|change| change.wire_breaking).count(),
        source_breaking: changes
            .iter()
            .filter(|change| change.source_breaking)
            .count(),
        api_breaking: changes.iter().filter(|change| change.api_breaking).count(),
    }
}

struct Loaded {
    endpoint: Endpoint,
    model: Model,
    members: BTreeMap<String, Member>,
    numbers: BTreeMap<String, u32>,
    entries: BTreeMap<String, crate::lock::Entry>,
    locations: BTreeMap<String, (String, Position)>,
    lock_file: String,
    lock_positions: BTreeMap<String, Position>,
}

impl Loaded {
    #[allow(clippy::too_many_lines)]
    fn new(
        path: &Path,
        model: Model,
        lock: &WireLock,
        released: bool,
        locations: BTreeMap<String, (String, Position)>,
        lock_positions: &BTreeMap<String, Position>,
    ) -> Result<Self> {
        let package = format!("{}:{}", model.package.namespace, model.package.name);
        let package_name = PackageName {
            namespace: model.package.namespace.clone(),
            name: model.package.name.clone(),
        };
        let lock_path = path.join("wire.lock");
        let lock_file = lock_path.display().to_string();
        lock.check_package(&package_name).map_err(|error| {
            let foreign_pos = lock
                .entries()
                .find(|(key, _)| key.package() != &package_name)
                .and_then(|(key, _)| lock_positions.get(&key.key()).copied());
            let mut diag = Diagnostic::error(codes::LOCKFILE_WRONG_PACKAGE, format!("{error:#}"))
                .with_path(lock_file.clone());
            if let Some(pos) = foreign_pos {
                diag = diag.with_position(pos);
            }
            ContractError::with_located_diagnostic(
                LocatedDiagnostic::from_diagnostic(diag).with_file(lock_file.clone()),
            )
        })?;
        let member_list = model.members()?;
        let members: BTreeMap<String, Member> = member_list
            .iter()
            .cloned()
            .map(|member| (member.key(), member))
            .collect();
        let entries: BTreeMap<String, crate::lock::Entry> = lock
            .entries()
            .map(|(key, entry)| (key.key(), entry.clone()))
            .collect();
        let numbers: BTreeMap<String, u32> = entries
            .iter()
            .map(|(key, entry)| (key.clone(), entry.number))
            .collect();
        let remedy = || lock_remedy(path, released);
        let locate_member =
            |code: &'static str, key: String, message: String, related_msg: Option<&str>| {
                let mut diag = Diagnostic::error(code, message).with_path(key.clone());
                let lock_pos = lock_positions.get(&key).copied();
                if let Some((file, pos)) = locations.get(&key) {
                    diag = diag.with_position(*pos);
                    let mut item = LocatedDiagnostic::from_diagnostic(diag)
                        .with_file(file.clone())
                        .with_logical(key);
                    if let Some(msg) = related_msg {
                        item = item.with_related(RelatedLocation {
                            file: lock_file.clone(),
                            position: lock_pos,
                            message: msg.to_string(),
                        });
                    }
                    item
                } else {
                    if let Some(pos) = lock_pos {
                        diag = diag.with_position(pos);
                    }
                    LocatedDiagnostic::from_diagnostic(diag)
                        .with_file(lock_file.clone())
                        .with_logical(key)
                }
            };
        for member in &member_list {
            let key = member.key();
            let Some(entry) = lock.get(&key) else {
                bail!(ContractError::with_located_diagnostic(locate_member(
                    codes::LOCKFILE_MISSING_NUMBER,
                    key.clone(),
                    format!("live member `{key}` has no wire number; {}", remedy()),
                    Some("wire.lock has no entry for this member"),
                )));
            };
            if entry.reserved {
                bail!(ContractError::with_located_diagnostic(locate_member(
                    codes::LOCKFILE_REINTRODUCED,
                    key.clone(),
                    format!(
                        "live member `{key}` is reserved in the lockfile; {}",
                        remedy()
                    ),
                    Some("reserved entry in wire.lock"),
                )));
            }
            if entry.kind != member.kind || entry.ty != member.ty {
                bail!(ContractError::with_located_diagnostic(locate_member(
                    codes::LOCKFILE_CHANGED_DRAFT,
                    key.clone(),
                    format!(
                        "live member `{key}` has a stale kind or canonical type; {}",
                        remedy()
                    ),
                    Some("existing entry in wire.lock"),
                )));
            }
        }
        // A draft entry that no longer belongs to the contract is stale too. Removed released
        // members stay valid when they are reserved; an unreserved entry with no live member is
        // the draft form `witforge lock` would drop. Header declaration order is deliberately
        // not validated here: a reorder is a source change `diff` reports, not a stale lockfile.
        for (key, entry) in lock.entries() {
            if !members.contains_key(&key.key()) && !entry.reserved {
                let key_str = key.key();
                let mut diag = Diagnostic::error(
                    codes::LOCKFILE_STALE_DRAFT,
                    format!("lockfile entry `{key}` is stale; {}", remedy()),
                )
                .with_path(key_str.clone());
                if let Some(pos) = lock_positions.get(&key_str).copied() {
                    diag = diag.with_position(pos);
                }
                bail!(ContractError::with_located_diagnostic(
                    LocatedDiagnostic::from_diagnostic(diag)
                        .with_file(lock_file.clone())
                        .with_logical(key_str),
                ));
            }
        }
        lock.validate_order_structure(&member_list)
            .map_err(|error| {
                let diag =
                    Diagnostic::error(codes::LOCKFILE_ERROR, format!("{error:#}; {}", remedy()))
                        .with_path(lock_file.clone());
                ContractError::with_located_diagnostic(
                    LocatedDiagnostic::from_diagnostic(diag).with_file(lock_file.clone()),
                )
            })?;
        Ok(Self {
            endpoint: Endpoint {
                path: path.to_path_buf(),
                version: model.package.version,
                package,
            },
            model,
            members,
            numbers,
            entries,
            locations,
            lock_file,
            lock_positions: lock_positions.clone(),
        })
    }

    fn number(&self, parent: &str, name: &str) -> Result<u32> {
        let key = format!("{}/{parent}.{name}", self.endpoint.package);
        self.numbers.get(&key).copied().with_context(|| {
            format!(
                "member `{key}` has no wire number in {}",
                self.endpoint.path.display()
            )
        })
    }

    fn preserves(&self, old: &Self, key: &str) -> bool {
        let Some(old_entry) = old.entries.get(key) else {
            return true;
        };
        matches!(
            self.entries.get(key),
            Some(entry)
                if entry.reserved
                    && entry.number == old_entry.number
                    && entry.kind == old_entry.kind
                    && entry.ty == old_entry.ty
        )
    }

    fn preserves_scope(&self, old: &Self, prefix: &str) -> bool {
        old.entries
            .iter()
            .filter(|(key, _)| {
                key.starts_with(&format!("{prefix}/"))
                    || key.starts_with(&format!("{prefix}."))
                    || key.starts_with(&format!("{prefix}#"))
            })
            .all(|(key, _)| self.preserves(old, key))
    }

    /// Numbers reserved in this lockfile under `parent`.
    fn reserved_numbers(&self, parent: &str) -> BTreeSet<u32> {
        let prefix = format!("{}/{parent}.", self.endpoint.package);
        self.entries
            .iter()
            .filter(|(key, entry)| entry.reserved && key.starts_with(&prefix))
            .map(|(_, entry)| entry.number)
            .collect()
    }

    /// Live member names keyed by wire number under `parent`.
    fn live_numbers(&self, parent: &str) -> BTreeMap<u32, String> {
        let prefix = format!("{}/{parent}.", self.endpoint.package);
        self.entries
            .iter()
            .filter_map(|(key, entry)| {
                if entry.reserved {
                    return None;
                }
                let name = key.strip_prefix(&prefix)?;
                if name.contains('/') || name.contains('#') || name.contains('.') {
                    return None;
                }
                Some((entry.number, name.to_string()))
            })
            .collect()
    }

    fn member(&self, parent: &str, name: &str) -> Result<&Member> {
        let key = format!("{}/{parent}.{name}", self.endpoint.package);
        self.members.get(&key).with_context(|| {
            format!(
                "member `{key}` is missing from the model of {}",
                self.endpoint.path.display()
            )
        })
    }

    fn member_type(&self, parent: &str, name: &str) -> Result<String> {
        Ok(self
            .member(parent, name)?
            .ty
            .clone()
            .unwrap_or_else(|| "_".to_string()))
    }
}

fn lock_remedy(path: &Path, released: bool) -> String {
    if released {
        return format!(
            "`{}` is released, so `witforge lock` refuses it; restore its released WIT and \
             wire.lock",
            path.display()
        );
    }
    if let Some((_, _, predecessor)) = crate::sibling_predecessor(path) {
        let prev_lock = predecessor.join("wire.lock");
        if prev_lock.is_file() {
            return format!(
                "run `witforge lock {} --against {}`",
                crate::shell_quote(path),
                crate::shell_quote(&prev_lock)
            );
        }
    }
    format!("run `witforge lock {}`", crate::shell_quote(path))
}

fn load_endpoint_with_options(path: &Path, released_override: Option<bool>) -> Result<Loaded> {
    let lock_path = path.join("wire.lock");
    crate::ensure_lockfile_not_symlink(&lock_path)?;
    if lock_path.is_file() {
        return load_version(path, released_override);
    }
    if looks_like_version_directory(path) {
        let lock_path = path.join("wire.lock");
        let quoted_dir = crate::shell_quote(path);
        let message = if let Some((major, _, predecessor)) = crate::sibling_predecessor(path)
            && predecessor.join("wire.lock").is_file()
        {
            let prev_lock = predecessor.join("wire.lock");
            format!(
                "version directory `{}` has no `wire.lock`; major {major} already has {}, so \
                 copy {} to {} and run `witforge lock {} --against {}` (or for a new major, \
                 create a fresh lockfile with `--first`)",
                path.display(),
                predecessor.display(),
                prev_lock.display(),
                lock_path.display(),
                quoted_dir,
                crate::shell_quote(&prev_lock),
            )
        } else {
            format!(
                "version directory `{}` has no `wire.lock`; for a first version of a package or of a \
                 new major, run `witforge lock {} --first`; for a new minor, copy the previous \
                 minor's wire.lock to {} and run witforge lock on that directory with --against \
                 pointing at the previous minor's wire.lock",
                path.display(),
                quoted_dir,
                lock_path.display(),
            )
        };
        return Err(operational_error(
            codes::IO_ERROR,
            message,
            Some(lock_path.display().to_string()),
        ));
    }
    let mut versions = crate::emit::versions::load_versions(path)?;
    let version = versions.pop().with_context(|| {
        format!(
            "no version directories with a wire.lock under {}",
            path.display()
        )
    })?;
    let released = match released_override {
        Some(flag) => flag,
        None => crate::is_released(&version.directory, None)?,
    };
    let lock_text = fs::read_to_string(version.directory.join("wire.lock")).unwrap_or_default();
    let lock_positions = WireLock::entry_positions(&lock_text);
    Loaded::new(
        &version.directory,
        version.model,
        &version.lock,
        released,
        version.locations,
        &lock_positions,
    )
}

/// Whether `path` is a version directory whose lockfile is missing, rather than a WIT root.
///
/// A root contains `v<major>.<minor>/` directories. A version directory contains WIT source
/// (normally `contract.wit`), a `deps/` tree, or is itself named `v<major>.<minor>`. Without this
/// distinction a forgotten `wire.lock` would be reported as “no version directories”.
fn looks_like_version_directory(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    if path.join("contract.wit").is_file() || path.join("deps").is_dir() {
        return true;
    }
    if let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str)
        && matches!(crate::parse_version_directory(name), Ok(Some(_)))
    {
        return true;
    }
    fs::read_dir(path).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .path()
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("wit"))
        })
    })
}

fn load_version(path: &Path, released_override: Option<bool>) -> Result<Loaded> {
    ensure_version_directory(path)?;
    // `diff` reads the manifest beside the endpoint unless a caller (`compare_git_base` or
    // `emit::doc::plan_with_released`) has already validated the applicable manifest.
    let released = match released_override {
        Some(flag) => flag,
        None => crate::is_released(path, None)?,
    };
    let contract = load(path)?;
    let violations = subset::check_located_diagnostics(&contract);
    if !violations.is_empty() {
        let messages: Vec<String> = violations
            .iter()
            .map(|located| match &located.diagnostic.path {
                Some(path) => format!("{path}: {}", located.diagnostic.message),
                None => located.diagnostic.message.clone(),
            })
            .collect();
        bail!(ContractError::with_located_diagnostics(
            format!("{}: {}", path.display(), messages.join("; ")),
            violations,
        ));
    }
    let locations = contract.member_locations();
    let model = crate::model::build(&contract)?;
    ensure_directory_version(path, &model)?;
    let lock_path = path.join("wire.lock");
    crate::ensure_lockfile_not_symlink(&lock_path)?;
    let text = fs::read_to_string(&lock_path)
        .with_context(|| format!("failed to read {}", lock_path.display()))?;
    let lock = WireLock::parse(&text).map_err(|err| {
        let position = crate::error_diagnostics(&err)
            .and_then(|diags| diags.first())
            .and_then(|diag| diag.position);
        operational_error_at(
            codes::PARSE_ERROR,
            format!("in {}: {err:#}", lock_path.display()),
            lock_path.display().to_string(),
            position,
        )
    })?;
    let lock_positions = WireLock::entry_positions(&text);
    Loaded::new(path, model, &lock, released, locations, &lock_positions)
}

struct Builder<'a> {
    old: &'a Loaded,
    new: &'a Loaded,
    changes: Vec<(Change, DecodeVerdict)>,
}

#[derive(Clone, Copy, Default)]
struct Breaking {
    wire: bool,
    source: bool,
    api: bool,
}

impl Breaking {
    const NONE: Self = Self {
        wire: false,
        source: false,
        api: false,
    };
    const ALL: Self = Self {
        wire: true,
        source: true,
        api: true,
    };
    const SOURCE_API: Self = Self {
        wire: false,
        source: true,
        api: true,
    };
    const SOURCE: Self = Self {
        wire: false,
        source: true,
        api: false,
    };
    const WIRE: Self = Self {
        wire: true,
        source: false,
        api: false,
    };
}

/// How additions and removals of one member kind affect protobuf binary decode compatibility.
#[derive(Clone, Copy)]
enum MemberDecodeRule<'a> {
    /// Record fields and function parameters: absence is accepted only when the member's type is
    /// absent-ok on the wire (`option<T>` or non-byte `list<T>`). Unknown fields are ignored.
    WireFields {
        old_absent_ok: &'a BTreeSet<String>,
        new_absent_ok: &'a BTreeSet<String>,
    },
    /// Closed enum cases and variant cases: adding breaks `old_reads_new`, removing breaks
    /// `new_reads_old`.
    ClosedCases,
    /// Flags bits: proto3 bool defaults to `false` on absence and ignores unknown fields.
    Flags,
}

struct MemberComparison<'a> {
    element: ChangeElement,
    path: &'a str,
    parent: &'a str,
    api_on_reorder: bool,
    defaulted: &'a BTreeSet<String>,
    decode_rule: MemberDecodeRule<'a>,
}

impl Builder<'_> {
    fn push(
        &mut self,
        kind: ChangeKind,
        element: ChangeElement,
        path: impl Into<String>,
        number: Option<u32>,
        breaking: Breaking,
        decode: DecodeVerdict,
    ) {
        self.changes.push((
            Change {
                kind,
                element,
                path: path.into(),
                number,
                wire_breaking: breaking.wire,
                source_breaking: breaking.source,
                api_breaking: breaking.api,
            },
            decode,
        ));
    }

    fn compare_interfaces(&mut self) -> Result<()> {
        let old: BTreeMap<&str, &Interface> = self
            .old
            .model
            .interfaces
            .iter()
            .map(|interface| (interface.name.as_str(), interface))
            .collect();
        let new: BTreeMap<&str, &Interface> = self
            .new
            .model
            .interfaces
            .iter()
            .map(|interface| (interface.name.as_str(), interface))
            .collect();
        for (name, new_iface) in new.iter().filter(|(name, _)| !old.contains_key(*name)) {
            let decode = if new_iface.functions.is_empty() {
                DecodeVerdict::COMPATIBLE
            } else {
                DecodeVerdict {
                    new_reads_old: true,
                    old_reads_new: false,
                }
            };
            self.push(
                ChangeKind::Added { defaulted: false },
                ChangeElement::Interface,
                *name,
                None,
                Breaking::NONE,
                decode,
            );
        }
        for (name, old_iface) in old.iter().filter(|(name, _)| !new.contains_key(*name)) {
            let prefix = format!("{}/{name}", self.old.endpoint.package);
            let wire = !self.new.preserves_scope(self.old, &prefix);
            let decode = if old_iface.types.is_empty() && old_iface.functions.is_empty() {
                DecodeVerdict::COMPATIBLE
            } else {
                DecodeVerdict {
                    new_reads_old: false,
                    old_reads_new: true,
                }
            };
            self.push(
                ChangeKind::Removed,
                ChangeElement::Interface,
                *name,
                None,
                Breaking {
                    wire,
                    source: true,
                    api: true,
                },
                decode,
            );
        }
        for (name, old_interface) in &old {
            let Some(new_interface) = new.get(name) else {
                continue;
            };
            self.compare_stability(
                ChangeElement::Interface,
                *name,
                &old_interface.stability,
                &new_interface.stability,
            );
            self.compare_types(old_interface, new_interface)?;
            self.compare_functions(old_interface, new_interface)?;
        }
        Ok(())
    }

    fn compare_types(&mut self, old: &Interface, new: &Interface) -> Result<()> {
        let old_types: BTreeMap<&str, &TypeDef> = old
            .types
            .iter()
            .map(|def| (def.name.as_str(), def))
            .collect();
        let new_types: BTreeMap<&str, &TypeDef> = new
            .types
            .iter()
            .map(|def| (def.name.as_str(), def))
            .collect();
        for name in new_types
            .keys()
            .filter(|name| !old_types.contains_key(*name))
        {
            self.push(
                ChangeKind::Added { defaulted: false },
                ChangeElement::Type,
                format!("{}/{}", old.name, name),
                None,
                Breaking::NONE,
                DecodeVerdict::COMPATIBLE,
            );
        }
        for name in old_types
            .keys()
            .filter(|name| !new_types.contains_key(*name))
        {
            let prefix = format!("{}/{}/{name}", self.old.endpoint.package, old.name);
            let wire = !self.new.preserves_scope(self.old, &prefix);
            let decode = DecodeVerdict {
                new_reads_old: false,
                old_reads_new: true,
            };
            self.push(
                ChangeKind::Removed,
                ChangeElement::Type,
                format!("{}/{}", old.name, name),
                None,
                Breaking {
                    wire,
                    source: true,
                    api: true,
                },
                decode,
            );
        }
        for (name, old_def) in &old_types {
            let Some(new_def) = new_types.get(name) else {
                continue;
            };
            self.compare_type(old, old_def, new_def)?;
            self.compare_stability(
                ChangeElement::Type,
                format!("{}/{}", old.name, name),
                &old_def.stability,
                &new_def.stability,
            );
        }
        Ok(())
    }

    fn compare_type(&mut self, interface: &Interface, old: &TypeDef, new: &TypeDef) -> Result<()> {
        let path = format!("{}/{}", interface.name, old.name);
        let parent = format!("{}/{}", interface.name, old.name);
        let old_kind = type_kind_name(&old.kind);
        let new_kind = type_kind_name(&new.kind);
        if old_kind != new_kind {
            // A kind change is wire-breaking even when every old number stays reserved: the
            // members that refer to the type keep the same canonical type name while their
            // encoding changes from an enum value or a bool field to a message (or back).
            self.push(
                ChangeKind::KindChanged {
                    from: old_kind.to_string(),
                    to: new_kind.to_string(),
                },
                ChangeElement::Type,
                path,
                None,
                Breaking {
                    wire: true,
                    source: true,
                    api: true,
                },
                DecodeVerdict::INCOMPATIBLE,
            );
            return Ok(());
        }
        match (&old.kind, &new.kind) {
            (TypeDefKind::Record(old_fields), TypeDefKind::Record(new_fields)) => {
                self.compare_record(interface, old, old_fields, new_fields)
            }
            (TypeDefKind::Variant(old_cases), TypeDefKind::Variant(new_cases)) => self
                .compare_cases(
                    interface,
                    old,
                    old_cases,
                    new_cases,
                    ChangeElement::VariantCase,
                ),
            (TypeDefKind::Enum(old_cases), TypeDefKind::Enum(new_cases)) => self.compare_cases(
                interface,
                old,
                old_cases,
                new_cases,
                ChangeElement::EnumCase,
            ),
            (TypeDefKind::Flags(old_flags), TypeDefKind::Flags(new_flags)) => {
                let old_names: Vec<String> =
                    old_flags.iter().map(|flag| flag.name.clone()).collect();
                let new_names: Vec<String> =
                    new_flags.iter().map(|flag| flag.name.clone()).collect();
                self.compare_members(
                    &member_pairs(self.old, &parent, &old_names)?,
                    &member_pairs(self.new, &parent, &new_names)?,
                    &MemberComparison {
                        element: ChangeElement::Flag,
                        path: &path,
                        parent: &parent,
                        api_on_reorder: false,
                        defaulted: &BTreeSet::new(),
                        decode_rule: MemberDecodeRule::Flags,
                    },
                )
            }
            _ => {
                let old_shape = type_shape(&old.kind, TypeRenderStyle::Compact);
                let new_shape = type_shape(&new.kind, TypeRenderStyle::Compact);
                if old_shape != new_shape {
                    self.push(
                        ChangeKind::Retyped {
                            from: old_shape,
                            to: new_shape,
                        },
                        ChangeElement::Type,
                        path,
                        None,
                        Breaking::SOURCE_API,
                        DecodeVerdict::INCOMPATIBLE,
                    );
                }
                Ok(())
            }
        }
    }

    fn compare_record(
        &mut self,
        interface: &Interface,
        old: &TypeDef,
        old_fields: &[crate::model::Field],
        new_fields: &[crate::model::Field],
    ) -> Result<()> {
        let path = format!("{}/{}", interface.name, old.name);
        let parent = format!("{}/{}", interface.name, old.name);
        let old_names: Vec<String> = old_fields.iter().map(|field| field.name.clone()).collect();
        let new_names: Vec<String> = new_fields.iter().map(|field| field.name.clone()).collect();
        let mut defaulted = BTreeSet::new();
        let mut old_absent_ok = BTreeSet::new();
        let mut new_absent_ok = BTreeSet::new();
        for field in old_fields {
            if is_absent_ok_on_wire(&self.old.model, &field.ty) {
                old_absent_ok.insert(field.name.clone());
            }
        }
        for field in new_fields {
            if default_expr(&self.new.model, &field.ty)?.is_some() {
                defaulted.insert(field.name.clone());
            }
            if is_absent_ok_on_wire(&self.new.model, &field.ty) {
                new_absent_ok.insert(field.name.clone());
            }
        }
        self.compare_members(
            &member_pairs(self.old, &parent, &old_names)?,
            &member_pairs(self.new, &parent, &new_names)?,
            &MemberComparison {
                element: ChangeElement::RecordField,
                path: &path,
                parent: &parent,
                api_on_reorder: false,
                defaulted: &defaulted,
                decode_rule: MemberDecodeRule::WireFields {
                    old_absent_ok: &old_absent_ok,
                    new_absent_ok: &new_absent_ok,
                },
            },
        )
    }

    fn compare_cases(
        &mut self,
        interface: &Interface,
        old: &TypeDef,
        old_cases: &[crate::model::Case],
        new_cases: &[crate::model::Case],
        element: ChangeElement,
    ) -> Result<()> {
        let path = format!("{}/{}", interface.name, old.name);
        let parent = format!("{}/{}", interface.name, old.name);
        let old_names: Vec<String> = old_cases.iter().map(|case| case.name.clone()).collect();
        let new_names: Vec<String> = new_cases.iter().map(|case| case.name.clone()).collect();
        self.compare_members(
            &member_pairs(self.old, &parent, &old_names)?,
            &member_pairs(self.new, &parent, &new_names)?,
            &MemberComparison {
                element,
                path: &path,
                parent: &parent,
                api_on_reorder: false,
                defaulted: &BTreeSet::new(),
                decode_rule: MemberDecodeRule::ClosedCases,
            },
        )
    }

    #[allow(clippy::too_many_lines)]
    fn compare_members(
        &mut self,
        old: &[(String, String)],
        new: &[(String, String)],
        spec: &MemberComparison<'_>,
    ) -> Result<()> {
        let MemberComparison {
            element,
            path,
            parent,
            api_on_reorder,
            defaulted,
            decode_rule,
        } = spec;
        let old_map: BTreeMap<&str, &str> = old
            .iter()
            .map(|(name, ty)| (name.as_str(), ty.as_str()))
            .collect();
        let new_map: BTreeMap<&str, &str> = new
            .iter()
            .map(|(name, ty)| (name.as_str(), ty.as_str()))
            .collect();
        let same_major =
            self.old.model.package.version.major == self.new.model.package.version.major;
        let reserved_numbers = self.old.reserved_numbers(parent);
        let old_live_numbers = self.old.live_numbers(parent);
        let new_live_numbers = self.new.live_numbers(parent);
        for name in new_map.keys().filter(|name| !old_map.contains_key(**name)) {
            let number = self.new.number(parent, name)?;
            // Reusing a number the old lockfile reserved or assigned to a different live member
            // within the same major changes the wire even though the member is newly added.
            let wire = same_major
                && (reserved_numbers.contains(&number)
                    || old_live_numbers
                        .get(&number)
                        .is_some_and(|old_name| old_name != *name));
            let decode = if wire {
                DecodeVerdict::INCOMPATIBLE
            } else {
                match decode_rule {
                    MemberDecodeRule::WireFields { new_absent_ok, .. } => DecodeVerdict {
                        new_reads_old: new_absent_ok.contains(*name),
                        old_reads_new: true,
                    },
                    MemberDecodeRule::ClosedCases => DecodeVerdict {
                        new_reads_old: true,
                        old_reads_new: false,
                    },
                    MemberDecodeRule::Flags => DecodeVerdict::COMPATIBLE,
                }
            };
            self.push(
                ChangeKind::Added {
                    defaulted: defaulted.contains(*name),
                },
                *element,
                format!("{path}.{name}"),
                Some(number),
                Breaking {
                    wire,
                    source: true,
                    api: true,
                },
                decode,
            );
        }
        for name in old_map.keys().filter(|name| !new_map.contains_key(**name)) {
            let number = self.old.number(parent, name)?;
            let key = format!("{}/{parent}.{name}", self.old.endpoint.package);
            let reused_by_new = same_major
                && new_live_numbers
                    .get(&number)
                    .is_some_and(|new_name| new_name != *name);
            let wire = !self.new.preserves(self.old, &key) || reused_by_new;
            let decode = if reused_by_new {
                DecodeVerdict::INCOMPATIBLE
            } else {
                match decode_rule {
                    MemberDecodeRule::WireFields { old_absent_ok, .. } => DecodeVerdict {
                        new_reads_old: true,
                        old_reads_new: old_absent_ok.contains(*name),
                    },
                    MemberDecodeRule::ClosedCases => DecodeVerdict {
                        new_reads_old: false,
                        old_reads_new: true,
                    },
                    MemberDecodeRule::Flags => DecodeVerdict::COMPATIBLE,
                }
            };
            self.push(
                ChangeKind::Removed,
                *element,
                format!("{path}.{name}"),
                Some(number),
                Breaking {
                    wire,
                    source: true,
                    api: true,
                },
                decode,
            );
        }
        for (name, old_ty) in &old_map {
            let Some(new_ty) = new_map.get(name) else {
                continue;
            };
            let old_number = self.old.number(parent, name)?;
            if old_ty != new_ty {
                self.push(
                    ChangeKind::Retyped {
                        from: (*old_ty).to_string(),
                        to: (*new_ty).to_string(),
                    },
                    *element,
                    format!("{path}.{name}"),
                    Some(self.new.number(parent, name)?),
                    Breaking::ALL,
                    DecodeVerdict::INCOMPATIBLE,
                );
            }
            let new_number = self.new.number(parent, name)?;
            if old_number != new_number {
                self.push(
                    ChangeKind::Renumbered {
                        from: old_number,
                        to: new_number,
                    },
                    *element,
                    format!("{path}.{name}"),
                    Some(new_number),
                    Breaking::WIRE,
                    DecodeVerdict::INCOMPATIBLE,
                );
            }
        }
        let before: Vec<String> = old
            .iter()
            .filter(|(name, _)| new_map.contains_key(name.as_str()))
            .map(|(name, _)| name.clone())
            .collect();
        let after: Vec<String> = new
            .iter()
            .filter(|(name, _)| old_map.contains_key(name.as_str()))
            .map(|(name, _)| name.clone())
            .collect();
        if before != after {
            self.push(
                ChangeKind::Reordered { before, after },
                element.order(),
                *path,
                None,
                Breaking {
                    wire: false,
                    source: true,
                    api: *api_on_reorder,
                },
                DecodeVerdict::COMPATIBLE,
            );
        }
        Ok(())
    }

    fn compare_stability(
        &mut self,
        element: ChangeElement,
        path: impl Into<String>,
        old: &crate::model::Stability,
        new: &crate::model::Stability,
    ) {
        if old != new {
            self.push(
                ChangeKind::StabilityChanged {
                    from: format_stability(old),
                    to: format_stability(new),
                },
                element,
                path,
                None,
                Breaking::NONE,
                DecodeVerdict::COMPATIBLE,
            );
        }
    }

    fn compare_function(
        &mut self,
        interface: &str,
        name: &str,
        old: &crate::model::Function,
        new: &crate::model::Function,
    ) -> Result<()> {
        let path = format!("{interface}.{name}");
        self.compare_stability(
            ChangeElement::Function,
            &path,
            &old.stability,
            &new.stability,
        );
        let parent = format!("{interface}#{name}");
        let old_params: Vec<(String, String)> = old
            .params
            .iter()
            .map(|param| {
                Ok((
                    param.name.clone(),
                    self.old.member_type(&parent, &param.name)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let new_params: Vec<(String, String)> = new
            .params
            .iter()
            .map(|param| {
                Ok((
                    param.name.clone(),
                    self.new.member_type(&parent, &param.name)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut old_absent_ok = BTreeSet::new();
        let mut new_absent_ok = BTreeSet::new();
        for param in &old.params {
            if is_absent_ok_on_wire(&self.old.model, &param.ty) {
                old_absent_ok.insert(param.name.clone());
            }
        }
        for param in &new.params {
            if is_absent_ok_on_wire(&self.new.model, &param.ty) {
                new_absent_ok.insert(param.name.clone());
            }
        }
        self.compare_members(
            &old_params,
            &new_params,
            &MemberComparison {
                element: ChangeElement::Parameter,
                path: &path,
                parent: &parent,
                api_on_reorder: true,
                defaulted: &BTreeSet::new(),
                decode_rule: MemberDecodeRule::WireFields {
                    old_absent_ok: &old_absent_ok,
                    new_absent_ok: &new_absent_ok,
                },
            },
        )?;
        let old_result = self.old.member_type(interface, name)?;
        let new_result = self.new.member_type(interface, name)?;
        if old_result != new_result {
            self.push(
                ChangeKind::Retyped {
                    from: old_result,
                    to: new_result,
                },
                ChangeElement::FunctionResult,
                path.clone(),
                Some(self.new.number(interface, name)?),
                Breaking::ALL,
                DecodeVerdict::INCOMPATIBLE,
            );
        }
        let old_method = self.old.number(interface, name)?;
        let new_method = self.new.number(interface, name)?;
        if old_method != new_method {
            self.push(
                ChangeKind::Renumbered {
                    from: old_method,
                    to: new_method,
                },
                ChangeElement::MethodId,
                path,
                Some(new_method),
                Breaking::WIRE,
                DecodeVerdict::INCOMPATIBLE,
            );
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn compare_functions(&mut self, old: &Interface, new: &Interface) -> Result<()> {
        let old_map: BTreeMap<&str, &crate::model::Function> = old
            .functions
            .iter()
            .map(|function| (function.name.as_str(), function))
            .collect();
        let new_map: BTreeMap<&str, &crate::model::Function> = new
            .functions
            .iter()
            .map(|function| (function.name.as_str(), function))
            .collect();
        let same_major =
            self.old.model.package.version.major == self.new.model.package.version.major;
        let reserved_methods = self.old.reserved_numbers(&old.name);
        let old_live_methods = self.old.live_numbers(&old.name);
        let new_live_methods = self.new.live_numbers(&old.name);
        for name in new_map.keys().filter(|name| !old_map.contains_key(*name)) {
            let number = self.new.number(&old.name, name)?;
            let wire = same_major
                && (reserved_methods.contains(&number)
                    || old_live_methods
                        .get(&number)
                        .is_some_and(|old_name| old_name != *name));
            let decode = if wire {
                DecodeVerdict::INCOMPATIBLE
            } else {
                DecodeVerdict {
                    new_reads_old: true,
                    old_reads_new: false,
                }
            };
            self.push(
                ChangeKind::Added { defaulted: false },
                ChangeElement::Function,
                format!("{}.{}", old.name, name),
                Some(number),
                Breaking {
                    wire,
                    source: true,
                    api: true,
                },
                decode,
            );
        }
        for name in old_map.keys().filter(|name| !new_map.contains_key(*name)) {
            let number = self.old.number(&old.name, name)?;
            let method = format!("{}/{}.{name}", self.old.endpoint.package, old.name);
            let params = format!("{}/{}#{name}", self.old.endpoint.package, old.name);
            let reused_by_new = same_major
                && new_live_methods
                    .get(&number)
                    .is_some_and(|new_name| new_name != *name);
            let decode = if reused_by_new {
                DecodeVerdict::INCOMPATIBLE
            } else {
                DecodeVerdict {
                    new_reads_old: false,
                    old_reads_new: true,
                }
            };
            self.push(
                ChangeKind::Removed,
                ChangeElement::Function,
                format!("{}.{}", old.name, name),
                Some(number),
                Breaking {
                    wire: !self.new.preserves(self.old, &method)
                        || !self.new.preserves_scope(self.old, &params)
                        || reused_by_new,
                    source: true,
                    api: true,
                },
                decode,
            );
        }
        for (name, old_function) in &old_map {
            let Some(new_function) = new_map.get(name) else {
                continue;
            };
            self.compare_function(&old.name, name, old_function, new_function)?;
        }
        let before: Vec<String> = old
            .functions
            .iter()
            .filter(|function| new_map.contains_key(function.name.as_str()))
            .map(|function| function.name.clone())
            .collect();
        let after: Vec<String> = new
            .functions
            .iter()
            .filter(|function| old_map.contains_key(function.name.as_str()))
            .map(|function| function.name.clone())
            .collect();
        if before != after {
            self.push(
                ChangeKind::Reordered { before, after },
                ChangeElement::FunctionOrder,
                old.name.clone(),
                None,
                Breaking::SOURCE,
                DecodeVerdict::COMPATIBLE,
            );
        }
        Ok(())
    }

    fn compare_worlds(&mut self) {
        let old: BTreeMap<&str, &crate::model::World> = self
            .old
            .model
            .worlds
            .iter()
            .map(|world| (world.name.as_str(), world))
            .collect();
        let new: BTreeMap<&str, &crate::model::World> = self
            .new
            .model
            .worlds
            .iter()
            .map(|world| (world.name.as_str(), world))
            .collect();
        for name in new.keys().filter(|name| !old.contains_key(*name)) {
            self.push(
                ChangeKind::Added { defaulted: false },
                ChangeElement::World,
                *name,
                None,
                Breaking::NONE,
                DecodeVerdict::COMPATIBLE,
            );
        }
        for name in old.keys().filter(|name| !new.contains_key(*name)) {
            self.push(
                ChangeKind::Removed,
                ChangeElement::World,
                *name,
                None,
                Breaking::SOURCE,
                DecodeVerdict::COMPATIBLE,
            );
        }
        for (name, old_world) in &old {
            let Some(new_world) = new.get(name) else {
                continue;
            };
            self.compare_stability(
                ChangeElement::World,
                *name,
                &old_world.stability,
                &new_world.stability,
            );
            self.compare_world_side(
                name,
                "import",
                ChangeElement::WorldImport,
                &old_world.imports,
                &new_world.imports,
            );
            self.compare_world_side(
                name,
                "export",
                ChangeElement::WorldExport,
                &old_world.exports,
                &new_world.exports,
            );
        }
    }

    fn compare_world_side(
        &mut self,
        world: &str,
        side: &str,
        element: ChangeElement,
        old: &[WorldInterface],
        new: &[WorldInterface],
    ) {
        let old_names: BTreeSet<String> = old.iter().map(WorldInterface::qualified_name).collect();
        let old_by_name: BTreeMap<String, &WorldInterface> = old
            .iter()
            .map(|interface| (interface.qualified_name(), interface))
            .collect();
        let new_by_name: BTreeMap<String, &WorldInterface> = new
            .iter()
            .map(|interface| (interface.qualified_name(), interface))
            .collect();
        let new_names: BTreeSet<String> = new.iter().map(WorldInterface::qualified_name).collect();
        for interface in new
            .iter()
            .filter(|i| !old_names.contains(&i.qualified_name()))
        {
            self.push(
                ChangeKind::Added { defaulted: false },
                element,
                format!("{world}/{side}/{}", interface.qualified_name()),
                None,
                Breaking::SOURCE,
                DecodeVerdict::COMPATIBLE,
            );
        }
        for interface in old
            .iter()
            .filter(|i| !new_names.contains(&i.qualified_name()))
        {
            self.push(
                ChangeKind::Removed,
                element,
                format!("{world}/{side}/{}", interface.qualified_name()),
                None,
                Breaking::SOURCE,
                DecodeVerdict::COMPATIBLE,
            );
        }
        for (name, old_interface) in &old_by_name {
            if let Some(new_interface) = new_by_name.get(name) {
                self.compare_stability(
                    element,
                    format!("{world}/{side}/{name}"),
                    &old_interface.stability,
                    &new_interface.stability,
                );
            }
        }
        let before: Vec<String> = old
            .iter()
            .filter(|interface| new_names.contains(&interface.qualified_name()))
            .map(WorldInterface::qualified_name)
            .collect();
        let after: Vec<String> = new
            .iter()
            .filter(|interface| old_names.contains(&interface.qualified_name()))
            .map(WorldInterface::qualified_name)
            .collect();
        if before != after {
            self.push(
                ChangeKind::Reordered { before, after },
                element.order(),
                format!("{world}/{side}"),
                None,
                Breaking::SOURCE,
                DecodeVerdict::COMPATIBLE,
            );
        }
    }

    fn compare_reserved_entries(&mut self) {
        if self.old.model.package.version.major != self.new.model.package.version.major {
            return;
        }
        let pkg_prefix = format!("{}/", self.old.endpoint.package);
        for (key, old_entry) in &self.old.entries {
            if !old_entry.reserved {
                continue;
            }
            let path = key.strip_prefix(&pkg_prefix).unwrap_or(key).to_string();
            let reused_by_live = key.rsplit_once('.').is_some_and(|(parent, _)| {
                let prefix = format!("{parent}.");
                self.new.entries.iter().any(|(new_key, entry)| {
                    !entry.reserved
                        && new_key.starts_with(&prefix)
                        && !new_key[prefix.len()..].contains('.')
                        && entry.number == old_entry.number
                })
            });
            let removed_or_renumbered_decode = if reused_by_live {
                DecodeVerdict::INCOMPATIBLE
            } else {
                DecodeVerdict::COMPATIBLE
            };
            match self.new.entries.get(key) {
                None => {
                    self.push(
                        ChangeKind::Removed,
                        ChangeElement::ReservedEntry,
                        path,
                        Some(old_entry.number),
                        Breaking::WIRE,
                        removed_or_renumbered_decode,
                    );
                }
                Some(new_entry) if !new_entry.reserved => {
                    self.push(
                        ChangeKind::Removed,
                        ChangeElement::ReservedEntry,
                        path,
                        Some(old_entry.number),
                        Breaking::WIRE,
                        removed_or_renumbered_decode,
                    );
                }
                Some(new_entry) => {
                    if new_entry.number != old_entry.number {
                        self.push(
                            ChangeKind::Renumbered {
                                from: old_entry.number,
                                to: new_entry.number,
                            },
                            ChangeElement::ReservedEntry,
                            path.clone(),
                            Some(new_entry.number),
                            Breaking::WIRE,
                            removed_or_renumbered_decode,
                        );
                    }
                    if new_entry.kind != old_entry.kind || new_entry.ty != old_entry.ty {
                        self.push(
                            ChangeKind::Retyped {
                                from: format_lock_entry_shape(old_entry),
                                to: format_lock_entry_shape(new_entry),
                            },
                            ChangeElement::ReservedEntry,
                            path,
                            Some(new_entry.number),
                            Breaking::WIRE,
                            DecodeVerdict::COMPATIBLE,
                        );
                    }
                }
            }
        }
    }
}

fn format_lock_entry_shape(entry: &crate::lock::Entry) -> String {
    match &entry.ty {
        Some(ty) => format!("{} {ty}", entry.kind),
        None => entry.kind.to_string(),
    }
}
fn format_stability(stability: &crate::model::Stability) -> String {
    match stability {
        crate::model::Stability::Unknown => "unknown".to_string(),
        crate::model::Stability::Unstable { feature } => {
            format!("unstable(feature={feature})")
        }
        crate::model::Stability::Stable { since } => format!("stable(since={since})"),
        crate::model::Stability::Deprecated {
            since,
            feature,
            deprecated,
            ..
        } => {
            let origin = match (since, feature) {
                (Some(since), None) => format!("since={since}"),
                (None, Some(feature)) => format!("feature={feature}"),
                (Some(since), Some(feature)) => format!("since={since},feature={feature}"),
                (None, None) => "origin=unknown".to_string(),
            };
            format!("deprecated({origin},version={deprecated})")
        }
    }
}

fn member_pairs(loaded: &Loaded, parent: &str, names: &[String]) -> Result<Vec<(String, String)>> {
    names
        .iter()
        .map(|name| Ok((name.clone(), loaded.member_type(parent, name)?)))
        .collect()
}

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn create() -> Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};

        static SEQ: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        fs::create_dir_all(&base)
            .with_context(|| format!("failed to create temporary root {}", base.display()))?;
        let pid = std::process::id();
        for _ in 0..128 {
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let candidate = base.join(format!("witforge-git-base-{pid}-{seq}"));
            match fs::create_dir(&candidate) {
                Ok(()) => return Ok(Self(candidate)),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(err) => {
                    return Err(operational_error(
                        codes::IO_ERROR,
                        format!(
                            "diff --git-base: failed to create temporary directory `{}`: {err}",
                            candidate.display()
                        ),
                        Some(candidate.display().to_string()),
                    ));
                }
            }
        }
        Err(operational_error(
            codes::IO_ERROR,
            format!(
                "diff --git-base: could not allocate a unique temporary directory under `{}`",
                base.display()
            ),
            Some(base.display().to_string()),
        ))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn validate_git_base_arg(git_base: &str) -> Result<()> {
    if git_base.trim().is_empty() {
        return Err(operational_error(
            codes::CLI_USAGE,
            "diff --git-base: revision must not be empty",
            None,
        ));
    }
    if git_base.starts_with('-') {
        return Err(operational_error(
            codes::CLI_USAGE,
            format!("diff --git-base: invalid revision `{git_base}` (must not start with `-`)"),
            None,
        ));
    }
    if git_base.chars().any(char::is_control) {
        return Err(operational_error(
            codes::CLI_USAGE,
            format!("diff --git-base: invalid revision `{git_base}` (contains control characters)"),
            None,
        ));
    }
    Ok(())
}

fn run_git_diff(dir: &Path, args: &[&str]) -> Result<std::process::Output> {
    std::process::Command::new("git")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .arg("-c")
        .arg("core.quotePath=false")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                operational_error(
                    codes::IO_ERROR,
                    "diff --git-base: `git` executable not found on PATH",
                    None,
                )
            } else {
                anyhow::Error::new(err).context("failed to run `git`")
            }
        })
}

fn to_git_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn ensure_no_symlinks_in_target_dir(dir: &Path, skip_dot_git: bool) -> Result<()> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)
        .with_context(|| format!("failed to read target directory {}", dir.display()))?
    {
        entries.push(
            entry.with_context(|| format!("failed to read target directory {}", dir.display()))?,
        );
    }
    entries.sort_by(|left, right| {
        left.file_name()
            .as_encoded_bytes()
            .cmp(right.file_name().as_encoded_bytes())
    });
    for entry in entries {
        if skip_dot_git && entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        if meta.file_type().is_symlink() {
            return Err(operational_error(
                codes::GUARD_REFUSED,
                format!(
                    "diff --git-base: target directory contains symlink `{}`; witforge refuses \
                     symlinks",
                    path.display()
                ),
                Some(path.display().to_string()),
            ));
        }
        if meta.is_dir() {
            ensure_no_symlinks_in_target_dir(&path, false)?;
        }
    }
    Ok(())
}

fn verify_git_target_is_tree(
    repo: &Path,
    commit: &str,
    git_base: &str,
    rel_git: &str,
) -> Result<()> {
    let output = run_git_diff(repo, &["ls-tree", "-z", commit, "--", rel_git])?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(operational_error(
            codes::IO_ERROR,
            format!(
                "diff --git-base: `git ls-tree` failed for `{git_base}:{rel_git}`: {}",
                stderr.trim()
            ),
            None,
        ));
    }
    let mut found: Option<&str> = None;
    for raw in output.stdout.split(|&byte| byte == 0) {
        if raw.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(raw).map_err(|_| {
            operational_error(
                codes::IO_ERROR,
                format!("diff --git-base: non-UTF-8 path in `{git_base}:{rel_git}`"),
                None,
            )
        })?;
        if let Some((meta, path)) = text.split_once('\t')
            && path == rel_git
        {
            found = Some(meta);
            break;
        }
    }
    let Some(meta) = found else {
        return Err(operational_error(
            codes::IO_ERROR,
            format!("diff --git-base: path `{rel_git}` does not exist at revision `{git_base}`"),
            None,
        ));
    };
    if meta.starts_with("120000 ") {
        return Err(operational_error(
            codes::GUARD_REFUSED,
            format!(
                "diff --git-base: `{git_base}:{rel_git}` is a symlink; witforge refuses symlinks"
            ),
            None,
        ));
    }
    if meta.starts_with("160000 ") {
        return Err(operational_error(
            codes::GUARD_REFUSED,
            format!(
                "diff --git-base: `{git_base}:{rel_git}` is a git submodule; expected a directory"
            ),
            None,
        ));
    }
    if !meta.starts_with("040000 tree ") {
        return Err(operational_error(
            codes::IO_ERROR,
            format!("diff --git-base: `{git_base}:{rel_git}` is not a directory"),
            None,
        ));
    }
    Ok(())
}

fn validate_git_rel_path(rel_path: &str, display_base: &str) -> Result<PathBuf> {
    if rel_path.is_empty()
        || rel_path.starts_with('/')
        || rel_path.contains('\\')
        || rel_path.contains('\0')
    {
        return Err(operational_error(
            codes::GUARD_REFUSED,
            format!("diff --git-base: unsafe relative path `{rel_path}` in `{display_base}`"),
            None,
        ));
    }
    let mut out = PathBuf::new();
    for segment in rel_path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(operational_error(
                codes::GUARD_REFUSED,
                format!("diff --git-base: unsafe relative path `{rel_path}` in `{display_base}`"),
                None,
            ));
        }
        let seg_path = Path::new(segment);
        let mut comps = seg_path.components();
        match (comps.next(), comps.next()) {
            (Some(std::path::Component::Normal(part)), None) => out.push(part),
            _ => {
                return Err(operational_error(
                    codes::GUARD_REFUSED,
                    format!(
                        "diff --git-base: unsafe relative path `{rel_path}` in `{display_base}`"
                    ),
                    None,
                ));
            }
        }
    }
    Ok(out)
}

fn collect_git_tree_blobs(
    repo: &Path,
    tree_spec: &str,
    display_base: &str,
    skip_dot_git: bool,
) -> Result<Vec<(PathBuf, String)>> {
    let output = run_git_diff(repo, &["ls-tree", "-r", "-z", tree_spec])?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(operational_error(
            codes::IO_ERROR,
            format!(
                "diff --git-base: `git ls-tree` failed for `{display_base}`: {}",
                stderr.trim()
            ),
            None,
        ));
    }
    let mut entries = Vec::new();
    for raw in output.stdout.split(|&byte| byte == 0) {
        if raw.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(raw).map_err(|_| {
            operational_error(
                codes::IO_ERROR,
                format!("diff --git-base: non-UTF-8 path in `{display_base}`"),
                None,
            )
        })?;
        let Some((meta, rel_path)) = text.split_once('\t') else {
            return Err(operational_error(
                codes::IO_ERROR,
                format!("diff --git-base: unexpected `git ls-tree` entry in `{display_base}`"),
                None,
            ));
        };
        if skip_dot_git && (rel_path == ".git" || rel_path.starts_with(".git/")) {
            continue;
        }
        let mut meta_parts = meta.split(' ');
        let mode = meta_parts.next().unwrap_or_default();
        let kind = meta_parts.next().unwrap_or_default();
        let oid = meta_parts.next().unwrap_or_default();
        if mode == "120000" {
            return Err(operational_error(
                codes::GUARD_REFUSED,
                format!(
                    "diff --git-base: tree at `{display_base}` contains symlink `{rel_path}`; \
                     witforge refuses symlinks"
                ),
                None,
            ));
        }
        if mode == "160000" {
            return Err(operational_error(
                codes::GUARD_REFUSED,
                format!(
                    "diff --git-base: tree at `{display_base}` contains git submodule `{rel_path}`"
                ),
                None,
            ));
        }
        if kind != "blob" || (mode != "100644" && mode != "100755") || oid.is_empty() {
            return Err(operational_error(
                codes::GUARD_REFUSED,
                format!(
                    "diff --git-base: tree at `{display_base}` contains unsupported entry \
                     `{rel_path}` (mode `{mode}`, type `{kind}`)"
                ),
                None,
            ));
        }
        let rel_buf = validate_git_rel_path(rel_path, display_base)?;
        entries.push((rel_buf, oid.to_string()));
    }
    Ok(entries)
}

#[allow(clippy::too_many_lines)]
fn validate_git_sibling_released(
    repo: &Path,
    commit: &str,
    git_base: &str,
    parent_dir_git: Option<&str>,
    released_rel: &str,
    target_dir_name: &str,
) -> Result<bool> {
    let Some(oid) = inspect_git_sibling_released(repo, commit, git_base, released_rel)? else {
        return Ok(false);
    };
    let manifest_label = format!("{git_base}:{released_rel}");
    let out = run_git_diff(repo, &["cat-file", "blob", &oid])?;
    if !out.status.success() {
        return Err(operational_error(
            codes::IO_ERROR,
            format!(
                "diff --git-base: failed to read released manifest `{manifest_label}`: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            Some(manifest_label),
        ));
    }
    let text = String::from_utf8(out.stdout).map_err(|_| {
        operational_error(
            codes::PARSE_ERROR,
            format!("diff --git-base: released manifest `{manifest_label}` is not valid UTF-8"),
            Some(manifest_label.clone()),
        )
    })?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut seen = BTreeSet::new();
    let mut target_is_released = false;
    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = raw.trim_matches([' ', '\t', '\r', '\x0b', '\x0c']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((entry_name, _is_retired)) = parse_manifest_entry(line) else {
            return Err(operational_error_at(
                codes::PARSE_ERROR,
                format!(
                    "line {line_no}: released entry `{line}` is not a plain directory name \
                     (expected `<name>` or `<name> retired` where `<name>` matches \
                     `^[A-Za-z0-9][A-Za-z0-9._-]*$`)"
                ),
                manifest_label,
                Some(line_position(line_no, raw)),
            ));
        };
        if !seen.insert(entry_name.to_string()) {
            return Err(operational_error_at(
                codes::PARSE_ERROR,
                format!("line {line_no}: duplicate released entry `{entry_name}`"),
                manifest_label,
                Some(line_position(line_no, raw)),
            ));
        }
        let entry_rel = match parent_dir_git {
            Some(parent) => format!("{parent}/{entry_name}"),
            None => entry_name.to_string(),
        };
        let entry_label = format!("{git_base}:{entry_rel}");
        validate_git_released_directory(
            repo,
            commit,
            &entry_rel,
            &entry_label,
            entry_name,
            line_no,
        )?;
        if entry_name == target_dir_name {
            target_is_released = true;
        }
    }
    Ok(target_is_released)
}

fn validate_git_released_directory(
    repo: &Path,
    commit: &str,
    entry_rel: &str,
    entry_label: &str,
    entry_name: &str,
    line_no: usize,
) -> Result<()> {
    let top = run_git_diff(repo, &["ls-tree", "-z", commit, "--", entry_rel])?;
    if !top.status.success() {
        return Err(operational_error(
            codes::IO_ERROR,
            format!("line {line_no}: released directory `{entry_name}` does not exist"),
            Some(entry_label.to_string()),
        ));
    }
    let mut found_tree = false;
    for raw in top.stdout.split(|&byte| byte == 0) {
        if raw.is_empty() {
            continue;
        }
        let Ok(text) = std::str::from_utf8(raw) else {
            continue;
        };
        let Some((meta, path)) = text.split_once('\t') else {
            continue;
        };
        if path != entry_rel {
            continue;
        }
        let mut parts = meta.split(' ');
        let mode = parts.next().unwrap_or_default();
        let kind = parts.next().unwrap_or_default();
        if mode == "120000" {
            return Err(operational_error(
                codes::GUARD_REFUSED,
                format!(
                    "line {line_no}: released directory `{entry_name}` is a symlink; witforge \
                     refuses symlinked version directories"
                ),
                Some(entry_label.to_string()),
            ));
        }
        if kind == "tree" {
            found_tree = true;
        }
    }
    if !found_tree {
        return Err(operational_error(
            codes::IO_ERROR,
            format!("line {line_no}: released directory `{entry_name}` does not exist"),
            Some(entry_label.to_string()),
        ));
    }
    let spec = format!("{commit}:{entry_rel}");
    let blobs = collect_git_tree_blobs(repo, &spec, entry_label, false)?;
    let has_lock = blobs.iter().any(|(rel, _)| rel == Path::new("wire.lock"));
    if !has_lock {
        return Err(operational_error(
            codes::IO_ERROR,
            format!("line {line_no}: released directory `{entry_name}` has no wire.lock"),
            Some(format!("{entry_label}/wire.lock")),
        ));
    }
    Ok(())
}

fn inspect_git_sibling_released(
    repo: &Path,
    commit: &str,
    git_base: &str,
    released_rel: &str,
) -> Result<Option<String>> {
    let output = run_git_diff(repo, &["ls-tree", "-z", commit, "--", released_rel])?;
    if !output.status.success() {
        return Ok(None);
    }
    for raw in output.stdout.split(|&byte| byte == 0) {
        if raw.is_empty() {
            continue;
        }
        let Ok(text) = std::str::from_utf8(raw) else {
            continue;
        };
        let Some((meta, path)) = text.split_once('\t') else {
            continue;
        };
        if path != released_rel {
            continue;
        }
        let mut meta_parts = meta.split(' ');
        let mode = meta_parts.next().unwrap_or_default();
        let kind = meta_parts.next().unwrap_or_default();
        let oid = meta_parts.next().unwrap_or_default();
        if mode == "120000" {
            return Err(operational_error(
                codes::GUARD_REFUSED,
                format!(
                    "diff --git-base: released manifest `{git_base}:{released_rel}` is a symlink; \
                     witforge refuses symlinks"
                ),
                None,
            ));
        }
        if kind == "blob" && (mode == "100644" || mode == "100755") && !oid.is_empty() {
            return Ok(Some(oid.to_string()));
        }
    }
    Ok(None)
}

#[allow(clippy::too_many_lines)]
fn materialize_git_blobs(repo: &Path, blobs: &[(PathBuf, String)]) -> Result<()> {
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    use std::process::{Command, Stdio};

    if blobs.is_empty() {
        return Ok(());
    }
    let mut child = Command::new("git")
        .arg("cat-file")
        .arg("--batch")
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                operational_error(
                    codes::IO_ERROR,
                    "diff --git-base: `git` executable not found on PATH",
                    None,
                )
            } else {
                anyhow::Error::new(err).context("failed to run `git cat-file --batch`")
            }
        })?;
    let mut stdin = child
        .stdin
        .take()
        .context("failed to open stdin for `git cat-file --batch`")?;
    let stdout = child
        .stdout
        .take()
        .context("failed to open stdout for `git cat-file --batch`")?;
    let mut stderr = child
        .stderr
        .take()
        .context("failed to open stderr for `git cat-file --batch`")?;

    let read_result: Result<()> = std::thread::scope(|scope| {
        let feeder = scope.spawn(move || -> std::io::Result<()> {
            for (_, oid) in blobs {
                stdin.write_all(oid.as_bytes())?;
                stdin.write_all(b"\n")?;
            }
            stdin.flush()?;
            drop(stdin);
            Ok(())
        });

        let mut reader = BufReader::new(stdout);
        let mut header = String::new();
        for (dest, oid) in blobs {
            header.clear();
            let read_len = reader
                .read_line(&mut header)
                .context("failed to read header from `git cat-file --batch`")?;
            if read_len == 0 {
                return Err(operational_error(
                    codes::IO_ERROR,
                    format!("diff --git-base: unexpected EOF reading git blob `{oid}`"),
                    None,
                ));
            }
            let trimmed = header.trim_end_matches(['\r', '\n']);
            let mut parts = trimmed.split(' ');
            let _got_oid = parts.next();
            let kind = parts.next().unwrap_or_default();
            let size_str = parts.next().unwrap_or_default();
            if kind != "blob" {
                return Err(operational_error(
                    codes::IO_ERROR,
                    format!("diff --git-base: git object `{oid}` is not a blob (`{trimmed}`)"),
                    None,
                ));
            }
            let size: usize = size_str.parse().map_err(|_| {
                operational_error(
                    codes::IO_ERROR,
                    format!("diff --git-base: invalid blob size in `{trimmed}`"),
                    None,
                )
            })?;
            let mut buf = vec![0u8; size];
            reader
                .read_exact(&mut buf)
                .with_context(|| format!("failed to read {size} bytes for git blob `{oid}`"))?;
            let mut nl = [0u8; 1];
            reader
                .read_exact(&mut nl)
                .context("failed to read trailing newline from `git cat-file --batch`")?;
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::write(dest, &buf).with_context(|| format!("failed to write {}", dest.display()))?;
        }

        match feeder.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(anyhow::Error::new(err)
                .context("failed to write object IDs to `git cat-file --batch`")),
            Err(_) => Err(operational_error(
                codes::INTERNAL_ERROR,
                "diff --git-base: git blob writer thread panicked",
                None,
            )),
        }
    });

    let mut err_text = String::new();
    let _ = stderr.read_to_string(&mut err_text);
    let status = child
        .wait()
        .context("failed to wait for `git cat-file --batch`")?;
    read_result?;
    if !status.success() {
        return Err(operational_error(
            codes::IO_ERROR,
            format!(
                "diff --git-base: `git cat-file --batch` failed: {}",
                err_text.trim()
            ),
            None,
        ));
    }
    Ok(())
}

fn rewrite_git_display_path(
    path: &Path,
    materialized_dir: &Path,
    git_base: &str,
    rel_git: &str,
) -> Option<String> {
    let tail = path.strip_prefix(materialized_dir).ok()?;
    let tail_git = to_git_path(tail);
    Some(match (rel_git.is_empty(), tail_git.is_empty()) {
        (true, true) => format!("{git_base}:."),
        (true, false) => format!("{git_base}:{tail_git}"),
        (false, true) => format!("{git_base}:{rel_git}"),
        (false, false) => format!("{git_base}:{rel_git}/{tail_git}"),
    })
}

fn redact_git_base_string(
    text: &str,
    materialized_dir: &Path,
    temp_root: &Path,
    git_base: &str,
    rel_git: &str,
) -> String {
    let display_base = if rel_git.is_empty() {
        format!("{git_base}:.")
    } else {
        format!("{git_base}:{rel_git}")
    };
    let mat_str = materialized_dir.display().to_string();
    let mat_slash = format!("{mat_str}/");
    let mat_back = format!("{mat_str}\\");
    let prefix_with_sep = if rel_git.is_empty() {
        format!("{git_base}:")
    } else {
        format!("{git_base}:{rel_git}/")
    };
    let mut out = text
        .replace(&mat_slash, &prefix_with_sep)
        .replace(&mat_back, &prefix_with_sep)
        .replace(&mat_str, &display_base);
    let temp_str = temp_root.display().to_string();
    if out.contains(&temp_str) {
        let temp_slash = format!("{temp_str}/");
        let temp_back = format!("{temp_str}\\");
        out = out
            .replace(&temp_slash, &format!("{git_base}:"))
            .replace(&temp_back, &format!("{git_base}:"))
            .replace(&temp_str, git_base);
    }
    out
}

fn redact_git_base_error(
    err: &anyhow::Error,
    materialized_dir: &Path,
    temp_root: &Path,
    git_base: &str,
    rel_git: &str,
) -> anyhow::Error {
    let redact_str =
        |s: &str| redact_git_base_string(s, materialized_dir, temp_root, git_base, rel_git);
    let redact_path = |p: &str| {
        rewrite_git_display_path(Path::new(p), materialized_dir, git_base, rel_git)
            .unwrap_or_else(|| redact_str(p))
    };
    let redact_located = |item: &LocatedDiagnostic| {
        let mut copy = item.clone();
        copy.file = copy.file.as_deref().map(redact_path);
        copy.diagnostic.message = redact_str(&copy.diagnostic.message);
        copy.diagnostic.path = copy.diagnostic.path.as_deref().map(redact_path);
        for rel in &mut copy.related {
            rel.file = redact_path(&rel.file);
            rel.message = redact_str(&rel.message);
        }
        copy
    };
    let redact_diag = |diag: &Diagnostic| {
        let mut copy = diag.clone();
        copy.message = redact_str(&copy.message);
        copy.path = copy.path.as_deref().map(redact_path);
        copy
    };

    let redacted_msg = redact_str(&format!("{err:#}"));
    if is_contract_error(err) {
        if let Some(located) = error_located_diagnostics(err) {
            let items: Vec<LocatedDiagnostic> = located.iter().map(redact_located).collect();
            return anyhow::Error::new(ContractError::with_located_diagnostics(
                redacted_msg,
                items,
            ));
        }
        if let Some(diags) = error_diagnostics(err) {
            let items: Vec<Diagnostic> = diags.iter().map(redact_diag).collect();
            return anyhow::Error::new(ContractError::with_diagnostics(redacted_msg, items));
        }
        return anyhow::Error::new(ContractError::new(redacted_msg));
    }
    if err
        .chain()
        .any(<dyn std::error::Error>::is::<DiagnosticError>)
    {
        if let Some(located) = error_located_diagnostics(err) {
            let items: Vec<LocatedDiagnostic> = located.iter().map(redact_located).collect();
            return anyhow::Error::new(DiagnosticError::with_located(redacted_msg, items));
        }
        if let Some(diags) = error_diagnostics(err) {
            let items: Vec<Diagnostic> = diags.iter().map(redact_diag).collect();
            return anyhow::Error::new(DiagnosticError::new(redacted_msg, items));
        }
    }
    anyhow::anyhow!(redacted_msg)
}
