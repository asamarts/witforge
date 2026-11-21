//! Generated-output ownership: expected files, atomic replacement, and `--check` drift.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read as _, Write as _};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};

/// A set of generated files that share one ownership-header line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFiles {
    header: String,
    files: BTreeMap<String, String>,
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl GeneratedFiles {
    /// Create a set using `header` as the generated-file ownership marker.
    #[must_use]
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            header: header.into(),
            files: BTreeMap::new(),
        }
    }

    /// Add one expected root-relative file.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is not a simple file name.
    pub fn insert(&mut self, name: impl Into<String>, contents: impl Into<String>) -> Result<()> {
        let name = name.into();
        let path = Path::new(&name);
        if name.is_empty()
            || name == "."
            || name == ".."
            || path.is_absolute()
            || path.components().count() != 1
        {
            bail!("generated file name `{name}` must be a simple name");
        }
        self.files.insert(name, contents.into());
        Ok(())
    }

    /// The ownership header.
    #[must_use]
    pub fn header(&self) -> &str {
        &self.header
    }

    /// The expected file names and contents.
    #[must_use]
    pub fn files(&self) -> &BTreeMap<String, String> {
        &self.files
    }

    /// Write all expected files and remove stale generated files in `out`.
    ///
    /// Each expected file is replaced atomically through a temporary sibling. Stale generated
    /// files are removed only after all expected writes succeed. Files without the ownership
    /// header are never touched. Before anything is written or removed, every expected path that
    /// already exists must be a regular file (not a symlink) that starts with the header;
    /// otherwise nothing is changed and an error names the path.
    ///
    /// # Errors
    ///
    /// Returns an error when an expected path is a symlink, is not a regular file, or is a file
    /// without the header, and an I/O error from creating, syncing, renaming, reading, removing,
    /// or writing files.
    pub fn write(&self, out: &Path) -> Result<()> {
        fs::create_dir_all(out)
            .with_context(|| format!("failed to create output root {}", out.display()))?;
        for name in self.files.keys() {
            let path = out.join(name);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => bail!(
                    "refusing to write `{}`: it is a symlink, and generation never writes through \
                     one",
                    path.display()
                ),
                Ok(metadata) if !metadata.is_file() => bail!(
                    "refusing to write `{}`: it exists and is not a regular file",
                    path.display()
                ),
                Ok(_) if !self.is_generated(&path)? => bail!(
                    "refusing to overwrite `{}`: it does not start with witforge's header, so it \
                     is not generated output; move it or choose another output root",
                    path.display()
                ),
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(err)
                        .with_context(|| format!("failed to inspect {}", path.display()));
                }
            }
        }
        let expected: std::collections::BTreeSet<&str> =
            self.files.keys().map(String::as_str).collect();
        let mut stale = Vec::new();
        for entry in fs::read_dir(out)
            .with_context(|| format!("failed to read output root {}", out.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if !expected.contains(name.as_str()) && self.is_generated(&path)? {
                stale.push(path);
            }
        }
        for (name, contents) in &self.files {
            write_atomic(&out.join(name), contents.as_bytes())?;
        }
        for path in stale {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove stale {}", path.display()))?;
        }
        Ok(())
    }

    /// Compare the expected files with `out`.
    ///
    /// Returns one message per missing, changed, or extra generated file, and per expected path
    /// that [`GeneratedFiles::write`] would refuse: a symlink, something other than a regular
    /// file, or a file without the ownership header. Only regular files are read, so a FIFO or
    /// a device at an expected path cannot block the check. Files without the ownership header
    /// at other paths are ignored.
    ///
    /// # Errors
    ///
    /// Returns an error when the output root is missing or cannot be read, or a file cannot be
    /// inspected or read.
    pub fn check(&self, out: &Path) -> Result<Vec<String>> {
        if !out.is_dir() {
            bail!("output root {} does not exist", out.display());
        }
        let mut messages = Vec::new();
        for (name, expected) in &self.files {
            let path = out.join(name);
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    messages.push(format!("missing generated file `{}`", path.display()));
                    continue;
                }
                Err(err) => {
                    return Err(err)
                        .with_context(|| format!("failed to inspect {}", path.display()));
                }
            };
            if metadata.file_type().is_symlink() {
                messages.push(format!(
                    "generated file `{}` is a symlink; generation refuses to write through it",
                    path.display()
                ));
            } else if !metadata.is_file() {
                messages.push(format!(
                    "generated file `{}` is not a regular file; generation refuses to replace it",
                    path.display()
                ));
            } else if !self.is_generated(&path)? {
                messages.push(format!(
                    "`{}` does not start with witforge's header, so generation refuses to \
                     overwrite it",
                    path.display()
                ));
            } else {
                let actual = fs::read(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                if actual != expected.as_bytes() {
                    messages.push(format!("changed generated file `{}`", path.display()));
                }
            }
        }
        for entry in fs::read_dir(out)
            .with_context(|| format!("failed to read output root {}", out.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if self.files.contains_key(&name) {
                continue;
            }
            if self.is_generated(&path)? {
                messages.push(format!("extra generated file `{}`", path.display()));
            }
        }
        messages.sort();
        Ok(messages)
    }

    /// Whether the file at `path` starts with the ownership header as its whole first line.
    ///
    /// Only the header's length plus a line ending is read, as bytes, so files that are not
    /// UTF-8 text (a `.DS_Store`, an image) are recognised as unowned rather than failing.
    fn is_generated(&self, path: &Path) -> Result<bool> {
        let header = self.header.as_bytes();
        let limit = u64::try_from(header.len() + 2).unwrap_or(u64::MAX);
        let mut start = Vec::new();
        File::open(path)
            .and_then(|file| file.take(limit).read_to_end(&mut start))
            .with_context(|| format!("failed to read {}", path.display()))?;
        let Some(rest) = start.strip_prefix(header) else {
            return Ok(false);
        };
        Ok(rest.is_empty() || rest.starts_with(b"\n") || rest.starts_with(b"\r\n"))
    }
}

/// Replace `path` with `contents` through a temporary file in the same directory.
///
/// A failed write leaves the previous file untouched and removes the temporary file. The
/// temporary name is hidden and unique per process/call.
fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .with_context(|| format!("output path `{}` has no file name", path.display()))?;
    let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{}.{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        sequence
    ));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .with_context(|| format!("failed to create {}", temporary.display()))?;
        file.write_all(contents)
            .with_context(|| format!("failed to write {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync {}", temporary.display()))?;
        fs::rename(&temporary, path)
            .with_context(|| format!("failed to replace {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Maximum symbolic-link hops followed when resolving a dangling output path.
const MAX_SYMLINK_DEPTH: usize = 40;

/// Refuse an output root that is, lies inside, or contains the WIT root.
///
/// Both paths are resolved to canonical form before they are compared: the WIT root is
/// canonicalised, and each existing prefix component of the output root is canonicalised (which
/// follows symlinks, Windows junctions, and case-insensitive spellings). A `..` component after a
/// component that does not exist yet is refused rather than collapsed lexically, so a path such as
/// `missing/../witlink/v0.1` cannot bypass the guard.
///
/// This guard prevents accidental writes into the WIT tree; it does not defend against a local
/// attacker swapping a symlink between the check and the write.
///
/// # Errors
///
/// Returns an error when the WIT root cannot be resolved, the output path uses `..` after a
/// missing component, or the output path is equal to, inside, or an ancestor of the WIT root.
pub fn ensure_outside_wit_root(wit_root: &Path, out: &Path) -> Result<()> {
    let root = fs::canonicalize(wit_root)
        .with_context(|| format!("failed to resolve WIT root {}", wit_root.display()))?;
    let resolved = resolve_output_root(out)?;
    if paths_overlap(&resolved, &root) {
        bail!(
            "output root {} must not be the WIT root, inside it, or contain it ({})",
            out.display(),
            root.display()
        );
    }
    Ok(())
}

/// Refuse an output root that is, lies inside, or contains any directory listed in `manifest`.
///
/// `manifest_dir` is the directory that owns the `released` manifest (usually the WIT root, or
/// the parent of an explicit `--released` file). Uses the same canonical path resolution as
/// [`ensure_outside_wit_root`].
///
/// # Errors
///
/// Returns an error when the output path or a released directory cannot be resolved, or when the
/// output path overlaps any listed released directory.
pub fn ensure_outside_released(
    manifest_dir: &Path,
    manifest: &crate::ReleasedManifest,
    out: &Path,
) -> Result<()> {
    if manifest.directories().is_empty() {
        return Ok(());
    }
    let resolved = resolve_output_root(out)?;
    for entry in manifest.directories() {
        let released = manifest_dir.join(entry);
        let canonical = fs::canonicalize(&released).with_context(|| {
            format!(
                "failed to resolve released directory `{entry}` ({})",
                released.display()
            )
        })?;
        if paths_overlap(&resolved, &canonical) {
            bail!(
                "output root {} must not be, contain, or lie inside released directory `{entry}` \
                 ({})",
                out.display(),
                canonical.display()
            );
        }
    }
    Ok(())
}

/// Resolve an output root to a canonical absolute path without lexically collapsing `..` across
/// missing components.
///
/// Every existing component (including symlinks and Windows junctions) is canonicalised step by
/// step; a dangling symlink target is resolved recursively up to a hop limit. Any `..` component
/// that appears after a missing component is refused.
///
/// # Errors
///
/// Returns an error when the current directory or an existing prefix cannot be resolved, a
/// symbolic-link loop is encountered, or `..` appears after a missing path component.
pub fn resolve_output_root(path: &Path) -> Result<PathBuf> {
    resolve_components(path, path, 0)
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

fn resolve_components(original: &Path, path: &Path, depth: usize) -> Result<PathBuf> {
    if depth > MAX_SYMLINK_DEPTH {
        bail!(
            "failed to resolve {}: too many levels of symbolic links",
            original.display()
        );
    }
    let mut current = if path.is_relative() {
        let cwd = std::env::current_dir().context("failed to read the current directory")?;
        fs::canonicalize(&cwd)
            .with_context(|| format!("failed to resolve current directory {}", cwd.display()))?
    } else {
        PathBuf::new()
    };
    let mut missing_tail = false;

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                current.push(prefix.as_os_str());
            }
            Component::RootDir => {
                current.push(Component::RootDir.as_os_str());
                if let Ok(canon) = fs::canonicalize(&current) {
                    current = canon;
                }
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if missing_tail {
                    bail!(
                        "output root {} must not contain `..` after a missing directory component \
                         (it must not be the WIT root, inside it, or contain it)",
                        original.display()
                    );
                }
                current.push(Component::ParentDir);
                current = fs::canonicalize(&current)
                    .with_context(|| format!("failed to resolve {}", original.display()))?;
            }
            Component::Normal(name) => {
                current.push(name);
                if missing_tail {
                    continue;
                }
                match fs::symlink_metadata(&current) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        match fs::canonicalize(&current) {
                            Ok(canon) => current = canon,
                            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                                let target = fs::read_link(&current).with_context(|| {
                                    format!("failed to read symlink {}", current.display())
                                })?;
                                let next = if target.is_absolute() {
                                    target
                                } else {
                                    current
                                        .parent()
                                        .unwrap_or_else(|| Path::new("."))
                                        .join(target)
                                };
                                current = resolve_components(original, &next, depth + 1)?;
                                missing_tail = true;
                            }
                            Err(err) => {
                                return Err(err).with_context(|| {
                                    format!("failed to resolve {}", original.display())
                                });
                            }
                        }
                    }
                    Ok(_) => {
                        current = fs::canonicalize(&current)
                            .with_context(|| format!("failed to resolve {}", original.display()))?;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        missing_tail = true;
                    }
                    Err(err) => {
                        return Err(err)
                            .with_context(|| format!("failed to resolve {}", original.display()));
                    }
                }
            }
        }
    }
    if current.as_os_str().is_empty() {
        bail!("failed to resolve {}", original.display());
    }
    Ok(current)
}

/// A resolved output root used for diagnostics.
#[must_use]
pub fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_files_and_cleans_up_a_failed_replacement() {
        let root =
            std::env::temp_dir().join(format!("witforge-output-atomic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let target = root.join("file.rs");
        fs::write(&target, b"old").unwrap();
        write_atomic(&target, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");

        // Renaming a file over a directory fails after the temporary file is written; the
        // directory must remain and the temporary file must be removed.
        let directory = root.join("directory");
        fs::create_dir(&directory).unwrap();
        assert!(write_atomic(&directory, b"new").is_err());
        assert!(directory.is_dir());
        let temporaries = fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(temporaries, 0);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn generated_files_expose_their_header() {
        let files = GeneratedFiles::new("// @generated by witforge; do not edit.");
        assert_eq!(files.header(), "// @generated by witforge; do not edit.");
    }

    #[test]
    fn check_reports_a_missing_output_root() {
        let out =
            std::env::temp_dir().join(format!("witforge-output-missing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&out);
        let error = GeneratedFiles::new("// generated").check(&out).unwrap_err();
        assert!(format!("{error:#}").contains("does not exist"), "{error:#}");
    }
}
