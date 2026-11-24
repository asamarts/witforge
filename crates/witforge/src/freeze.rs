//! Released-directory freeze verification backed by Git history.
//!
//! Reads the full history of `<WIT_ROOT>/released` (every commit that changed it, merges and
//! renames included) to prove that:
//! - every entry any committed version of `released` listed is still listed (deleting `released`
//!   counts as removing every entry);
//! - every currently listed directory is valid, distinct, exists, is neither a symlink nor holds
//!   any symlink (tracked or in the working tree), has a `wire.lock` tracked by Git under that
//!   exact spelling, has no untracked files (ignored ones included), and matches the content stored
//!   in every commit whose manifest listed it;
//! - when a WIT root was moved with `git mv`, the manifest's earlier rename history is checked
//!   against the earlier paths (copies are not followed).
//!
//! Shallow clones, missing `git`, and directories outside any Git repository are operational
//! errors (exit `2`), never silent passes. Every freeze violation is collected and returned so the
//! caller sees all offending paths at once (exit `1`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::diagnostic::{Diagnostic, LocatedDiagnostic, Position, codes};
use crate::{line_position, operational_error, parse_manifest_entry};

/// Options controlling [`check_root`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FreezeOptions {
    /// Optional lower-bound Git revision bounding the manifest history scan.
    ///
    /// When set, the check evaluates `<base>` itself (if `<base>:<manifest>` exists) and every
    /// commit in `<base>..HEAD` that touched `<manifest>`.
    pub base: Option<String>,
}

/// Result of checking one WIT root's released-directory freeze.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreezeReport {
    /// Repository-relative WIT root using `/` separators (`.` when the root is the repo top).
    pub root: String,
    /// Repository-relative manifest path using `/` separators.
    pub manifest: String,
    /// Number of historical release comparisons completed.
    pub checks: usize,
    /// True when no `released` manifest exists on disk or in the scanned Git history.
    pub no_manifest: bool,
    /// Every freeze violation found across manifest hygiene and Git history.
    pub violations: Vec<LocatedDiagnostic>,
}

impl FreezeReport {
    /// Whether the freeze check passed with zero violations.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Verify the released-directory freeze invariant for one WIT root using the default
/// `<WIT_ROOT>/released` manifest.
///
/// # Errors
///
/// Returns an operational [`crate::DiagnosticError`] (exit `2` in the CLI) when `wit_root` is not
/// a directory, `git` is missing from `PATH`, `wit_root` is outside any Git repository, the
/// repository is a shallow clone, `--base` does not name a valid commit, or a `git` command fails
/// unexpectedly.
pub fn check_root(wit_root: &Path, options: &FreezeOptions) -> Result<FreezeReport> {
    check_root_with_manifest(wit_root, None, options)
}

/// Verify the released-directory freeze invariant for one WIT root with an optional explicit
/// released-version manifest path (defaults to `<WIT_ROOT>/released` when `manifest` is `None`).
///
/// Also validates retirement ordering (retired versions must form a contiguous oldest prefix of
/// version directories under `wit_root` and leave at least one live version).
///
/// # Errors
///
/// Returns an operational [`crate::DiagnosticError`] (exit `2` in the CLI) when `wit_root` is not
/// a directory, an explicitly supplied `manifest` does not exist on disk or in Git history or lies
/// outside the repository, `git` is missing from `PATH`, `wit_root` is outside any Git repository,
/// the repository is a shallow clone, `--base` does not name a valid commit, or a `git` command
/// fails unexpectedly.
#[allow(clippy::too_many_lines)]
pub fn check_root_with_manifest(
    wit_root: &Path,
    manifest: Option<&Path>,
    options: &FreezeOptions,
) -> Result<FreezeReport> {
    if !wit_root.is_dir() {
        return Err(operational_error(
            codes::IO_ERROR,
            format!(
                "release freeze: WIT root `{}` is not a directory",
                wit_root.display()
            ),
            Some(wit_root.display().to_string()),
        ));
    }
    let abs_root = fs::canonicalize(wit_root)
        .with_context(|| format!("failed to resolve WIT root {}", wit_root.display()))?;

    let top_output = run_git(&abs_root, &["rev-parse", "--show-toplevel"])?;
    if !top_output.status.success() {
        return Err(operational_error(
            codes::GUARD_REFUSED,
            format!(
                "release freeze: `{}` is not inside a git repository",
                wit_root.display()
            ),
            Some(wit_root.display().to_string()),
        ));
    }
    let top_text = String::from_utf8_lossy(&top_output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    let repo = fs::canonicalize(Path::new(&top_text))
        .with_context(|| format!("failed to resolve git repository root `{top_text}`"))?;

    let rel_root = abs_root.strip_prefix(&repo).map_err(|_| {
        operational_error(
            codes::GUARD_REFUSED,
            format!(
                "release freeze: `{}` is not inside the repository {}",
                wit_root.display(),
                repo.display()
            ),
            Some(wit_root.display().to_string()),
        )
    })?;
    let rel_slashes = to_git_path(rel_root);
    let (root_label, prefix) = if rel_slashes.is_empty() {
        (".".to_string(), String::new())
    } else {
        (rel_slashes.clone(), format!("{rel_slashes}/"))
    };
    let (manifest_rel, manifest_in_root, explicit_manifest) = match manifest {
        Some(explicit) => {
            let resolved = resolve_manifest_in_repo(explicit, &repo)?;
            let in_root = resolved
                .strip_prefix(&prefix)
                .is_some_and(|rest| !rest.contains('/'));
            (resolved, in_root, Some(explicit))
        }
        None => (format!("{prefix}released"), true, None),
    };
    let manifest = manifest_rel;

    // Shallow clones cannot prove the full history of `released`.
    let shallow_out = git_stdout(&repo, &["rev-parse", "--is-shallow-repository"])?;
    if shallow_out.trim() == "true" {
        return Err(operational_error(
            codes::GUARD_REFUSED,
            "release freeze: the repository is a shallow clone; the check needs the manifest's \
             full history (in GitHub Actions, set `fetch-depth: 0` on `actions/checkout`)",
            Some(wit_root.display().to_string()),
        ));
    }

    let base_commit = if let Some(base) = &options.base {
        let spec = format!("{base}^{{commit}}");
        let out = run_git(&repo, &["rev-parse", "--verify", "--quiet", &spec])?;
        if !out.status.success() {
            return Err(operational_error(
                codes::CLI_USAGE,
                format!("release freeze: invalid --base revision `{base}`"),
                None,
            ));
        }
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    };

    let has_head = run_git(&repo, &["rev-parse", "--verify", "--quiet", "HEAD"])?
        .status
        .success();

    let log_commits = if has_head {
        manifest_commits(&repo, &manifest, base_commit.as_deref())?
    } else {
        Vec::new()
    };

    let manifest_disk = repo.join(&manifest);
    let manifest_meta = fs::symlink_metadata(&manifest_disk).ok();
    let manifest_is_symlink = manifest_meta
        .as_ref()
        .is_some_and(|meta| meta.file_type().is_symlink());
    let manifest_is_file = manifest_meta
        .as_ref()
        .is_some_and(|meta| meta.file_type().is_file());

    if manifest_meta.is_none() && log_commits.is_empty() {
        if let Some(explicit) = explicit_manifest {
            return Err(operational_error(
                codes::IO_ERROR,
                format!(
                    "release freeze: released manifest `{}` does not exist",
                    explicit.display()
                ),
                Some(explicit.display().to_string()),
            ));
        }
        return Ok(FreezeReport {
            root: root_label,
            manifest,
            checks: 0,
            no_manifest: true,
            violations: Vec::new(),
        });
    }

    let mut violations: Vec<LocatedDiagnostic> = Vec::new();
    let mut current_entries: Vec<(usize, String, String)> = Vec::new();
    if manifest_is_symlink {
        push_violation(
            &mut violations,
            &repo,
            &manifest,
            None,
            format!("released manifest `{manifest}` is a symlink; commit a regular file"),
        );
    } else if manifest_is_file {
        let text = fs::read_to_string(&manifest_disk).with_context(|| {
            format!(
                "failed to read released manifest {}",
                manifest_disk.display()
            )
        })?;
        current_entries = parse_manifest_entries_with_lines(&text);
    } else if manifest_meta.is_some() {
        push_violation(
            &mut violations,
            &repo,
            &manifest,
            None,
            format!("released manifest `{manifest}` is not a regular file"),
        );
    }

    // 1. Validate every entry in the current working-tree manifest.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    // Map current listed entry name -> is_retired, plus source line position for diagnostics.
    let mut current_entries_map: BTreeMap<String, bool> = BTreeMap::new();
    let mut entry_positions: BTreeMap<String, Option<Position>> = BTreeMap::new();
    for (line_no, raw_line, trimmed_line) in &current_entries {
        let pos = Some(line_position(*line_no, raw_line));
        let Some((entry, is_retired)) = parse_manifest_entry(trimmed_line) else {
            push_violation(
                &mut violations,
                &repo,
                &manifest,
                pos,
                format!(
                    "invalid released entry `{trimmed_line}` in {manifest}: expected `<name>` or \
                     `<name> retired` where `<name>` matches `^[A-Za-z0-9][A-Za-z0-9._-]*$`"
                ),
            );
            continue;
        };
        let entry = entry.to_string();
        if !seen.insert(entry.clone()) {
            push_violation(
                &mut violations,
                &repo,
                &manifest,
                pos,
                format!("duplicate released entry `{entry}` in {manifest}"),
            );
            continue;
        }
        current_entries_map.insert(entry.clone(), is_retired);
        entry_positions.insert(entry.clone(), pos);

        let dir_rel = format!("{prefix}{entry}");
        let dir_disk = repo.join(&dir_rel);
        match fs::symlink_metadata(&dir_disk) {
            Err(_) => {
                push_violation(
                    &mut violations,
                    &repo,
                    &dir_rel,
                    None,
                    format!("released directory `{dir_rel}` does not exist"),
                );
                continue;
            }
            Ok(meta) if meta.file_type().is_symlink() => {
                push_violation(
                    &mut violations,
                    &repo,
                    &dir_rel,
                    None,
                    format!(
                        "released directory `{dir_rel}` is a symlink; release the directory itself"
                    ),
                );
                continue;
            }
            Ok(meta) if !meta.is_dir() => {
                push_violation(
                    &mut violations,
                    &repo,
                    &dir_rel,
                    None,
                    format!("released directory `{dir_rel}` does not exist"),
                );
                continue;
            }
            Ok(_) => {}
        }

        let lock_rel = format!("{dir_rel}/wire.lock");
        let lock_disk = dir_disk.join("wire.lock");
        let lock_is_regular =
            fs::symlink_metadata(&lock_disk).is_ok_and(|meta| meta.file_type().is_file());
        if lock_is_regular {
            let tracked = git_nul_paths(&repo, &["ls-files", "-z", "--", &lock_rel])?;
            if tracked.len() != 1 || tracked[0] != lock_rel {
                push_violation(
                    &mut violations,
                    &repo,
                    &lock_rel,
                    None,
                    format!(
                        "released directory `{dir_rel}` has no wire.lock tracked by git under \
                         that exact spelling"
                    ),
                );
            }
        } else {
            push_violation(
                &mut violations,
                &repo,
                &lock_rel,
                None,
                format!("released directory `{dir_rel}` has no wire.lock"),
            );
        }

        // Check both git-indexed symlinks and working-tree symlinks inside the directory.
        let mut symlinks = tracked_symlinks(&repo, &dir_rel)?;
        collect_disk_symlinks(&dir_disk, &dir_rel, &mut symlinks)?;
        for link_path in symlinks {
            push_violation(
                &mut violations,
                &repo,
                &link_path,
                None,
                format!("released directory `{dir_rel}` holds symlinks: {link_path}"),
            );
        }

        // Check untracked files, including gitignored ones.
        let dir_slash = format!("{dir_rel}/");
        let untracked = git_nul_paths(&repo, &["ls-files", "--others", "-z", "--", &dir_slash])?;
        for path in untracked {
            push_violation(
                &mut violations,
                &repo,
                &path,
                None,
                format!("released directory `{dir_rel}` has untracked files: {path}"),
            );
        }
    }

    // 1b. Enforce retirement order: retired versions must form a contiguous oldest prefix of
    // version directories under `wit_root` and leave at least one live version.
    if current_entries_map.values().any(|&retired| retired)
        && let Ok(versions) = crate::discover_version_directories(&abs_root)
    {
        let mut first_live: Option<String> = None;
        for (_, _, dir) in &versions {
            let name = dir
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default();
            if current_entries_map.get(name) == Some(&true) {
                if let Some(ref older_live) = first_live {
                    push_violation(
                        &mut violations,
                        &repo,
                        &manifest,
                        entry_positions.get(name).copied().flatten(),
                        format!(
                            "cannot retire `{name}` while older version `{older_live}` is still \
                             live; only the oldest versions of a WIT root may be retired"
                        ),
                    );
                }
            } else if first_live.is_none() {
                first_live = Some(name.to_string());
            }
        }
        if first_live.is_none() && !versions.is_empty() {
            push_violation(
                &mut violations,
                &repo,
                &manifest,
                None,
                format!(
                    "every version directory under {root_label} is retired; at least one live \
                     version is required"
                ),
            );
        }
    }

    // 2. Walk the manifest's commit history at its current path.
    let mut checks = 0usize;
    let mut reported_removals: BTreeSet<(String, String)> = BTreeSet::new();
    let mut reported_unretire: BTreeSet<(String, String)> = BTreeSet::new();
    let mut reported_diffs: BTreeSet<(String, String)> = BTreeSet::new();

    for commit in &log_commits {
        let Some(blob) = git_show_file(&repo, commit, &manifest)? else {
            continue;
        };
        let short = short_sha(commit);
        for (entry, was_retired) in parse_manifest_history_entries(&blob) {
            let Some(&currently_retired) = current_entries_map.get(&entry) else {
                if reported_removals.insert((manifest.clone(), entry.clone())) {
                    push_violation(
                        &mut violations,
                        &repo,
                        &manifest,
                        None,
                        format!(
                            "released entry `{entry}` (listed in {short}) was removed from \
                             {manifest}"
                        ),
                    );
                }
                continue;
            };
            if was_retired
                && !currently_retired
                && reported_unretire.insert((manifest.clone(), entry.clone()))
            {
                push_violation(
                    &mut violations,
                    &repo,
                    &manifest,
                    None,
                    format!(
                        "released entry `{entry}` (retired in {short}) was un-retired in \
                         {manifest}; retirement is permanent"
                    ),
                );
            }
            let dir_rel = format!("{prefix}{entry}");
            let changed = git_diff_paths(&repo, commit, &dir_rel)?;
            if !changed.is_empty() {
                for changed_path in changed {
                    if reported_diffs.insert((dir_rel.clone(), changed_path.clone())) {
                        push_violation(
                            &mut violations,
                            &repo,
                            &changed_path,
                            None,
                            format!(
                                "released directory `{dir_rel}` changed since its release in \
                                 {short}: `{changed_path}`"
                            ),
                        );
                    }
                }
            }
            checks += 1;
        }
    }

    // 3. Walk earlier rename paths of the manifest (stop at copies).
    if has_head && !log_commits.is_empty() {
        let earlier_paths = follow_renamed_manifests(&repo, &manifest, base_commit.as_deref())?;
        for old_manifest in earlier_paths {
            let old_prefix = if manifest_in_root {
                match old_manifest.rsplit_once('/') {
                    Some((dir, _)) => format!("{dir}/"),
                    None => String::new(),
                }
            } else {
                prefix.clone()
            };
            let old_commits = manifest_commits(&repo, &old_manifest, base_commit.as_deref())?;
            for commit in &old_commits {
                let Some(blob) = git_show_file(&repo, commit, &old_manifest)? else {
                    continue;
                };
                let short = short_sha(commit);
                for (entry, was_retired) in parse_manifest_history_entries(&blob) {
                    let Some(&currently_retired) = current_entries_map.get(&entry) else {
                        if reported_removals.insert((old_manifest.clone(), entry.clone())) {
                            push_violation(
                                &mut violations,
                                &repo,
                                &manifest,
                                None,
                                format!(
                                    "released entry `{entry}` (listed in {short} at \
                                     {old_manifest}) was removed from {manifest}"
                                ),
                            );
                        }
                        continue;
                    };
                    if was_retired
                        && !currently_retired
                        && reported_unretire.insert((old_manifest.clone(), entry.clone()))
                    {
                        push_violation(
                            &mut violations,
                            &repo,
                            &manifest,
                            None,
                            format!(
                                "released entry `{entry}` (retired in {short} at \
                                 {old_manifest}) was un-retired in {manifest}; retirement is \
                                 permanent"
                            ),
                        );
                    }
                    let old_dir = format!("{old_prefix}{entry}");
                    let cur_dir = format!("{prefix}{entry}");
                    let Some(released_tree) = git_tree_oid(&repo, &format!("{commit}:{old_dir}"))?
                    else {
                        push_violation(
                            &mut violations,
                            &repo,
                            &cur_dir,
                            None,
                            format!("released directory `{old_dir}` is missing from {short}"),
                        );
                        continue;
                    };
                    let Some(head_tree) = git_tree_oid(&repo, &format!("HEAD:{cur_dir}"))? else {
                        push_violation(
                            &mut violations,
                            &repo,
                            &cur_dir,
                            None,
                            format!("released directory `{cur_dir}` is not committed"),
                        );
                        continue;
                    };
                    let mut changed_files = BTreeSet::new();
                    if released_tree != head_tree {
                        for rel in git_diff_tree_paths(&repo, &released_tree, &head_tree)? {
                            changed_files.insert(format!("{cur_dir}/{rel}"));
                        }
                        if changed_files.is_empty() {
                            changed_files.insert(cur_dir.clone());
                        }
                    }
                    for path in git_diff_paths(&repo, "HEAD", &cur_dir)? {
                        changed_files.insert(path);
                    }
                    for changed_path in changed_files {
                        if reported_diffs.insert((cur_dir.clone(), changed_path.clone())) {
                            push_violation(
                                &mut violations,
                                &repo,
                                &changed_path,
                                None,
                                format!(
                                    "released directory `{cur_dir}` changed since its release as \
                                     `{old_dir}` in {short}: `{changed_path}`"
                                ),
                            );
                        }
                    }
                    checks += 1;
                }
            }
        }
    }

    Ok(FreezeReport {
        root: root_label,
        manifest,
        checks,
        no_manifest: false,
        violations,
    })
}

fn resolve_manifest_in_repo(explicit: &Path, repo: &Path) -> Result<String> {
    let Some(file_name) = explicit.file_name() else {
        return Err(operational_error(
            codes::IO_ERROR,
            format!(
                "release freeze: invalid released manifest path `{}`",
                explicit.display()
            ),
            Some(explicit.display().to_string()),
        ));
    };
    let parent = explicit
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let abs_parent = fs::canonicalize(parent).map_err(|err| {
        operational_error(
            codes::IO_ERROR,
            format!(
                "release freeze: released manifest `{}` cannot be resolved: {err}",
                explicit.display()
            ),
            Some(explicit.display().to_string()),
        )
    })?;
    let abs_manifest = abs_parent.join(file_name);
    let rel_manifest = abs_manifest.strip_prefix(repo).map_err(|_| {
        operational_error(
            codes::GUARD_REFUSED,
            format!(
                "release freeze: released manifest `{}` is not inside the repository {}",
                explicit.display(),
                repo.display()
            ),
            Some(explicit.display().to_string()),
        )
    })?;
    Ok(to_git_path(rel_manifest))
}

fn push_violation(
    violations: &mut Vec<LocatedDiagnostic>,
    repo: &Path,
    repo_rel_path: &str,
    position: Option<Position>,
    message: String,
) {
    let full = repo.join(repo_rel_path);
    let file = full.display().to_string();
    let mut diag = Diagnostic::error(codes::RELEASE_FREEZE_VIOLATION, message)
        .with_path(repo_rel_path.to_string());
    if let Some(pos) = position {
        diag = diag.with_position(pos);
    }
    violations.push(LocatedDiagnostic::from_diagnostic(diag).with_file(file));
}

fn short_sha(commit: &str) -> &str {
    let end = commit.len().min(12);
    &commit[..end]
}

fn to_git_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn parse_manifest_entries_with_lines(text: &str) -> Vec<(usize, String, String)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let trimmed = raw.trim_matches([' ', '\t', '\r', '\x0b', '\x0c']);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        out.push((index + 1, raw.to_string(), trimmed.to_string()));
    }
    out
}

fn parse_manifest_history_entries(text: &str) -> Vec<(String, bool)> {
    parse_manifest_entries_with_lines(text)
        .into_iter()
        .map(|(_, _, line)| match parse_manifest_entry(&line) {
            Some((name, retired)) => (name.to_string(), retired),
            None => (line, false),
        })
        .collect()
}

fn run_git(dir: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
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
                    "release freeze: `git` executable not found on PATH",
                    None,
                )
            } else {
                anyhow::Error::new(err).context("failed to run `git`")
            }
        })
}

fn git_stdout(dir: &Path, args: &[&str]) -> Result<String> {
    let output = run_git(dir, args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(operational_error(
            codes::IO_ERROR,
            format!(
                "release freeze: `git {}` failed: {}",
                args.join(" "),
                stderr.trim()
            ),
            None,
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_nul_paths(dir: &Path, args: &[&str]) -> Result<Vec<String>> {
    let output = run_git(dir, args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(operational_error(
            codes::IO_ERROR,
            format!(
                "release freeze: `git {}` failed: {}",
                args.join(" "),
                stderr.trim()
            ),
            None,
        ));
    }
    Ok(output
        .stdout
        .split(|&byte| byte == 0)
        .filter(|slice| !slice.is_empty())
        .map(|slice| String::from_utf8_lossy(slice).replace('\\', "/"))
        .collect())
}

fn manifest_commits(repo: &Path, manifest: &str, base: Option<&str>) -> Result<Vec<String>> {
    let mut commits = Vec::new();
    let range;
    let args: Vec<&str> = if let Some(base_sha) = base {
        if git_show_file(repo, base_sha, manifest)?.is_some() {
            commits.push(base_sha.to_string());
        }
        range = format!("{base_sha}..HEAD");
        vec![
            "log",
            "--full-history",
            "--reverse",
            "--format=%H",
            &range,
            "--",
            manifest,
        ]
    } else {
        vec![
            "log",
            "--full-history",
            "--reverse",
            "--format=%H",
            "--",
            manifest,
        ]
    };
    let text = git_stdout(repo, &args)?;
    for line in text.lines() {
        let sha = line.trim();
        if !sha.is_empty() && !commits.iter().any(|existing| existing == sha) {
            commits.push(sha.to_string());
        }
    }
    Ok(commits)
}

fn git_show_file(repo: &Path, commit: &str, path: &str) -> Result<Option<String>> {
    let spec = format!("{commit}:{path}");
    let exists = run_git(repo, &["cat-file", "-e", &spec])?;
    if !exists.status.success() {
        return Ok(None);
    }
    let output = run_git(repo, &["show", &spec])?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(operational_error(
            codes::IO_ERROR,
            format!("release freeze: failed to read `{spec}`: {}", stderr.trim()),
            None,
        ));
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
}

fn git_tree_oid(repo: &Path, spec: &str) -> Result<Option<String>> {
    let output = run_git(repo, &["rev-parse", "--verify", "--quiet", spec])?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

fn git_diff_paths(repo: &Path, commit: &str, dir_rel: &str) -> Result<Vec<String>> {
    let output = run_git(
        repo,
        &[
            "diff",
            "--no-renames",
            "--name-status",
            "-z",
            commit,
            "--",
            dir_rel,
        ],
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(operational_error(
            codes::IO_ERROR,
            format!(
                "release freeze: `git diff {commit} -- {dir_rel}` failed: {}",
                stderr.trim()
            ),
            None,
        ));
    }
    Ok(parse_no_rename_name_status_z(&output.stdout))
}

fn git_diff_tree_paths(repo: &Path, old_tree: &str, new_tree: &str) -> Result<Vec<String>> {
    let output = run_git(
        repo,
        &[
            "diff-tree",
            "-r",
            "--no-renames",
            "--name-status",
            "-z",
            old_tree,
            new_tree,
        ],
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(operational_error(
            codes::IO_ERROR,
            format!("release freeze: `git diff-tree` failed: {}", stderr.trim()),
            None,
        ));
    }
    Ok(parse_no_rename_name_status_z(&output.stdout))
}

fn parse_no_rename_name_status_z(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut parts = bytes.split(|&byte| byte == 0);
    while let Some(status) = parts.next() {
        if status.is_empty() {
            continue;
        }
        if let Some(path) = parts.next()
            && !path.is_empty()
        {
            out.push(String::from_utf8_lossy(path).replace('\\', "/"));
        }
    }
    out
}

fn tracked_symlinks(repo: &Path, dir_rel: &str) -> Result<BTreeSet<String>> {
    let dir_slash = format!("{dir_rel}/");
    let output = run_git(repo, &["ls-files", "-s", "-z", "--", &dir_slash])?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(operational_error(
            codes::IO_ERROR,
            format!(
                "release freeze: `git ls-files -s` failed: {}",
                stderr.trim()
            ),
            None,
        ));
    }
    let mut links = BTreeSet::new();
    for entry in output.stdout.split(|&byte| byte == 0) {
        if entry.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(entry);
        if let Some((meta, path)) = text.split_once('\t')
            && meta.starts_with("120000 ")
        {
            links.insert(path.replace('\\', "/"));
        }
    }
    Ok(links)
}

fn collect_disk_symlinks(dir_disk: &Path, dir_rel: &str, out: &mut BTreeSet<String>) -> Result<()> {
    for entry in fs::read_dir(dir_disk)
        .with_context(|| format!("failed to read released directory {}", dir_disk.display()))?
    {
        let entry = entry
            .with_context(|| format!("failed to read released directory {}", dir_disk.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_rel = format!("{dir_rel}/{name}");
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", entry.path().display()))?;
        if file_type.is_symlink() {
            out.insert(child_rel);
        } else if file_type.is_dir() {
            collect_disk_symlinks(&entry.path(), &child_rel, out)?;
        }
    }
    Ok(())
}

/// Follow `git mv` renames of `manifest`, stopping as soon as Git reports a copy (`C...`).
fn follow_renamed_manifests(
    repo: &Path,
    manifest: &str,
    base: Option<&str>,
) -> Result<BTreeSet<String>> {
    let range;
    let args: Vec<&str> = if let Some(base_sha) = base {
        range = format!("{base_sha}..HEAD");
        vec![
            "log",
            "--follow",
            "--name-status",
            "-z",
            "--format=",
            &range,
            "--",
            manifest,
        ]
    } else {
        vec![
            "log",
            "--follow",
            "--name-status",
            "-z",
            "--format=",
            "--",
            manifest,
        ]
    };
    let output = run_git(repo, &args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(operational_error(
            codes::IO_ERROR,
            format!(
                "release freeze: `git log --follow` failed: {}",
                stderr.trim()
            ),
            None,
        ));
    }
    let mut earlier = BTreeSet::new();
    let mut parts = output.stdout.split(|&byte| byte == 0);
    while let Some(raw_status) = parts.next() {
        let status_text = String::from_utf8_lossy(raw_status);
        let status = status_text.trim_matches(['\r', '\n']);
        if status.is_empty() {
            continue;
        }
        if status.starts_with('R') {
            if let Some(old_path) = parts.next() {
                let _new_path = parts.next();
                let old = String::from_utf8_lossy(old_path).replace('\\', "/");
                if !old.is_empty() && old != manifest {
                    earlier.insert(old);
                }
            }
        } else if status.starts_with('C') {
            break;
        } else {
            let _path = parts.next();
        }
    }
    Ok(earlier)
}
