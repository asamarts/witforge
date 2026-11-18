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

/// Refuse an output root that is the WIT root or inside it.
///
/// Both paths are resolved before they are compared: the WIT root is canonicalised, and so is the
/// longest existing prefix of the output root, with the rest normalised lexically. Neither `..`
/// components nor symlinks can reach the WIT root.
///
/// # Errors
///
/// Returns an error when the WIT root cannot be resolved or the output path is equal to or below
/// it.
pub fn ensure_outside_wit_root(wit_root: &Path, out: &Path) -> Result<()> {
    let root = fs::canonicalize(wit_root)
        .with_context(|| format!("failed to resolve WIT root {}", wit_root.display()))?;
    let resolved = resolve(out)?;
    if resolved.starts_with(&root) {
        bail!(
            "output root {} must not be the WIT root or inside it ({})",
            out.display(),
            root.display()
        );
    }
    Ok(())
}

/// Resolve `path` to an absolute path with no `.` or `..` components.
///
/// The longest prefix that exists is canonicalised, which follows its symlinks and `..`
/// components as the operating system would. The rest does not exist yet, so it contains no
/// symlinks and is normalised lexically.
fn resolve(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)
        .with_context(|| format!("failed to resolve {}", path.display()))?;
    for prefix in absolute.ancestors() {
        let Ok(mut resolved) = fs::canonicalize(prefix) else {
            continue;
        };
        let rest = absolute
            .strip_prefix(prefix)
            .expect("an ancestor is a prefix");
        for component in rest.components() {
            match component {
                Component::ParentDir => {
                    resolved.pop();
                }
                Component::Normal(name) => resolved.push(name),
                Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
            }
        }
        return Ok(resolved);
    }
    bail!("failed to resolve {}", path.display())
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
}
