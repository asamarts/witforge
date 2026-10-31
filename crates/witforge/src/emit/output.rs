//! Generated-output ownership: expected files, atomic-free writes, and `--check` drift.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// A set of generated files that share one ownership-header line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFiles {
    header: String,
    files: BTreeMap<String, String>,
}

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
    /// Files without the ownership header are never touched.
    ///
    /// # Errors
    ///
    /// Returns an I/O error from creating, reading, removing, or writing files.
    pub fn write(&self, out: &Path) -> Result<()> {
        fs::create_dir_all(out)
            .with_context(|| format!("failed to create output root {}", out.display()))?;
        let expected: std::collections::BTreeSet<&str> =
            self.files.keys().map(String::as_str).collect();
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
                fs::remove_file(&path)
                    .with_context(|| format!("failed to remove stale {}", path.display()))?;
            }
        }
        for (name, contents) in &self.files {
            let path = out.join(name);
            fs::write(&path, contents)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
        Ok(())
    }

    /// Compare the expected files with `out`.
    ///
    /// Returns one message per missing, changed, or extra generated file. Files without the
    /// ownership header are ignored.
    ///
    /// # Errors
    ///
    /// Returns an error when the output root is missing or cannot be read.
    pub fn check(&self, out: &Path) -> Result<Vec<String>> {
        if !out.is_dir() {
            bail!("output root {} does not exist", out.display());
        }
        let mut messages = Vec::new();
        for (name, expected) in &self.files {
            let path = out.join(name);
            match fs::read(&path) {
                Ok(actual) if actual == expected.as_bytes() => {}
                Ok(_) => messages.push(format!("changed generated file `{}`", path.display())),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    messages.push(format!("missing generated file `{}`", path.display()));
                }
                Err(err) => {
                    return Err(err).with_context(|| format!("failed to read {}", path.display()));
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

/// Refuse an output root that is the WIT root or inside it.
///
/// # Errors
///
/// Returns an error when the output path is equal to or below the WIT root.
pub fn ensure_outside_wit_root(wit_root: &Path, out: &Path) -> Result<()> {
    let root = std::path::absolute(wit_root)
        .with_context(|| format!("failed to resolve {}", wit_root.display()))?;
    let out =
        std::path::absolute(out).with_context(|| format!("failed to resolve {}", out.display()))?;
    if out == root || out.starts_with(&root) {
        bail!(
            "output root {} must not be the WIT root or inside it ({})",
            out.display(),
            root.display()
        );
    }
    Ok(())
}

/// A resolved output root used for diagnostics.
#[must_use]
pub fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}
