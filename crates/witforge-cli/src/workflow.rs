//! Configuration discovery and workflow helpers (`new-version`, `release`, directory classification).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use witforge::diagnostic::codes;

use crate::op_error;

/// Default configuration file name searched upwards from the current working directory.
pub(crate) const CONFIG_FILE_NAME: &str = "witforge.toml";

/// Walk upwards from `start_dir` for the nearest `witforge.toml`, stopping after checking the
/// enclosing Git repository root (a directory containing `.git`) or at the filesystem root.
pub(crate) fn discover_config_from(start_dir: &Path) -> Option<PathBuf> {
    let mut cursor = Some(start_dir);
    while let Some(dir) = cursor {
        let candidate = dir.join(CONFIG_FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if dir.join(".git").exists() {
            break;
        }
        cursor = dir.parent();
    }
    None
}

/// Discover `witforge.toml` starting from the current working directory.
pub(crate) fn discover_config() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    discover_config_from(&cwd)
}

/// Classification of a directory passed to `witforge check`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckTargetKind {
    /// Holds WIT source / `wire.lock` / `deps/` directly and no `v<major>.<minor>` children.
    VersionDir,
    /// Holds one or more `v<major>.<minor>` directories and no direct WIT source/lockfile.
    WitRoot,
    /// Holds both direct WIT source/lockfile and `v<major>.<minor>` directories (ambiguous).
    Both,
    /// Holds neither direct WIT source/lockfile nor `v<major>.<minor>` directories.
    Neither,
}

/// Classify an existing directory as a single version directory, a WIT root, both, or neither.
pub(crate) fn classify_check_dir(path: &Path) -> Result<CheckTargetKind> {
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
        if is_dir && name == "deps" {
            has_version_sources = true;
            continue;
        }
        if !is_dir
            && entry_path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wit"))
        {
            has_version_sources = true;
            continue;
        }
        if crate::version_directory(&entry_path).is_some() {
            has_version_dirs = true;
        }
    }
    Ok(match (has_version_sources, has_version_dirs) {
        (true, false) => CheckTargetKind::VersionDir,
        (false, true) => CheckTargetKind::WitRoot,
        (true, true) => CheckTargetKind::Both,
        (false, false) => CheckTargetKind::Neither,
    })
}

/// Create the next minor (`v<major>.<minor+1>`) or next major (`v<major+1>.0`) version directory
/// under `wit_root` by copying WIT files, `deps/`, and (for a minor only) `wire.lock`, rewriting
/// the `package ...@<old>;` version header in every root `.wit` file.
#[allow(clippy::too_many_lines)]
pub(crate) fn create_new_version(
    wit_root: &Path,
    major_bump: bool,
) -> Result<(PathBuf, PathBuf, bool)> {
    if !wit_root.is_dir() {
        return Err(op_error(
            codes::IO_ERROR,
            format!("WIT root `{}` is not a directory", wit_root.display()),
            Some(wit_root.display().to_string()),
        ));
    }
    let versions = witforge::discover_version_directories(wit_root)?;
    let (latest_major, latest_minor, latest_dir) = versions
        .last()
        .expect("discover_version_directories never returns empty")
        .clone();
    witforge::ensure_version_directory(&latest_dir)?;

    let (target_major, target_minor) = if major_bump {
        (latest_major + 1, 0)
    } else {
        (latest_major, latest_minor + 1)
    };
    let target_name = format!("v{target_major}.{target_minor}");
    let target_dir = wit_root.join(&target_name);
    if fs::symlink_metadata(&target_dir).is_ok() {
        return Err(op_error(
            codes::GUARD_REFUSED,
            format!(
                "target version directory `{}` already exists",
                target_dir.display()
            ),
            Some(target_dir.display().to_string()),
        ));
    }

    let source_lock = latest_dir.join("wire.lock");
    if !major_bump && !source_lock.is_file() {
        return Err(op_error(
            codes::IO_ERROR,
            format!(
                "latest version directory `{}` has no `wire.lock`; lock `{}` before starting a \
                 new minor version",
                latest_dir.display(),
                latest_dir.display()
            ),
            Some(source_lock.display().to_string()),
        ));
    }

    let old_ver = format!("{latest_major}.{latest_minor}.0");
    let new_ver = format!("{target_major}.{target_minor}.0");

    fs::create_dir(&target_dir)
        .with_context(|| format!("failed to create {}", target_dir.display()))?;
    let populate = (|| -> Result<()> {
        let mut entries: Vec<PathBuf> = fs::read_dir(&latest_dir)
            .with_context(|| format!("failed to read {}", latest_dir.display()))?
            .map(|entry| entry.map(|item| item.path()))
            .collect::<std::io::Result<Vec<_>>>()
            .with_context(|| format!("failed to read {}", latest_dir.display()))?;
        entries.sort();

        for entry_path in entries {
            let meta = fs::symlink_metadata(&entry_path)
                .with_context(|| format!("failed to inspect {}", entry_path.display()))?;
            if meta.file_type().is_symlink() {
                return Err(op_error(
                    codes::GUARD_REFUSED,
                    format!(
                        "version directory `{}` contains symlink `{}`; new-version refuses to \
                         copy symlinks",
                        latest_dir.display(),
                        entry_path.display()
                    ),
                    Some(entry_path.display().to_string()),
                ));
            }
            let Some(name) = entry_path.file_name().and_then(std::ffi::OsStr::to_str) else {
                continue;
            };
            if meta.is_file()
                && entry_path
                    .extension()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("wit"))
            {
                let text = fs::read_to_string(&entry_path)
                    .with_context(|| format!("failed to read {}", entry_path.display()))?;
                let rewritten = rewrite_package_version(&text, &old_ver, &new_ver);
                fs::write(target_dir.join(name), rewritten).with_context(|| {
                    format!("failed to write {}", target_dir.join(name).display())
                })?;
            } else if meta.is_dir() && name == "deps" {
                copy_dir_no_symlinks(&entry_path, &target_dir.join("deps"))?;
            }
        }

        if !major_bump {
            fs::copy(&source_lock, target_dir.join("wire.lock")).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_lock.display(),
                    target_dir.join("wire.lock").display()
                )
            })?;
        }
        Ok(())
    })();

    if let Err(err) = populate {
        let _ = fs::remove_dir_all(&target_dir);
        return Err(err);
    }

    Ok((latest_dir, target_dir, !major_bump))
}

/// Rewrite `@<old_ver>` to `@<new_ver>` in top-level `package ...@<old_ver>;` declarations while
/// leaving comments, `use` imports, and line endings untouched.
pub(crate) fn rewrite_package_version(text: &str, old_ver: &str, new_ver: &str) -> String {
    let old_suffix = format!("@{old_ver}");
    let new_suffix = format!("@{new_ver}");
    let mut out = String::with_capacity(text.len() + 8);
    let mut in_block_comment = false;

    for raw_line in text.split_inclusive('\n') {
        let (line_body, newline) = if let Some(stripped) = raw_line.strip_suffix("\r\n") {
            (stripped, "\r\n")
        } else if let Some(stripped) = raw_line.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (raw_line, "")
        };
        let (bom, body) = if out.is_empty() {
            match line_body.strip_prefix('\u{feff}') {
                Some(rest) => ("\u{feff}", rest),
                None => ("", line_body),
            }
        } else {
            ("", line_body)
        };

        let mut replaced = None;
        if !in_block_comment {
            let trimmed = body.trim_start_matches([' ', '\t']);
            if (trimmed.starts_with("package ") || trimmed.starts_with("package\t"))
                && let Some(pos) = body.find(&old_suffix)
            {
                let after = &body[pos + old_suffix.len()..];
                let boundary_ok = after
                    .chars()
                    .next()
                    .is_none_or(|ch| matches!(ch, ';' | '{' | ' ' | '\t' | '/'));
                if boundary_ok {
                    let mut updated = String::with_capacity(body.len() + new_suffix.len());
                    updated.push_str(&body[..pos]);
                    updated.push_str(&new_suffix);
                    updated.push_str(after);
                    replaced = Some(updated);
                }
            }
        }

        // Track multi-line `/* ... */` block comments across lines (outside `//` line comments).
        let bytes = body.as_bytes();
        let mut i = 0usize;
        while i + 1 < bytes.len() {
            if in_block_comment {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
            } else if bytes[i] == b'/' && bytes[i + 1] == b'/' {
                break;
            } else if bytes[i] == b'/' && bytes[i + 1] == b'*' {
                in_block_comment = true;
                i += 2;
                continue;
            }
            i += 1;
        }

        out.push_str(bom);
        match replaced {
            Some(updated) => out.push_str(&updated),
            None => out.push_str(body),
        }
        out.push_str(newline);
    }
    out
}

fn copy_dir_no_symlinks(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;
    let mut entries: Vec<PathBuf> = fs::read_dir(source)
        .with_context(|| format!("failed to read {}", source.display()))?
        .map(|entry| entry.map(|item| item.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to read {}", source.display()))?;
    entries.sort();
    for entry_path in entries {
        let meta = fs::symlink_metadata(&entry_path)
            .with_context(|| format!("failed to inspect {}", entry_path.display()))?;
        if meta.file_type().is_symlink() {
            return Err(op_error(
                codes::GUARD_REFUSED,
                format!(
                    "dependency tree contains symlink `{}`; new-version refuses to copy symlinks",
                    entry_path.display()
                ),
                Some(entry_path.display().to_string()),
            ));
        }
        let target = destination.join(
            entry_path
                .file_name()
                .context("directory entry missing file name")?,
        );
        if meta.is_dir() {
            copy_dir_no_symlinks(&entry_path, &target)?;
        } else {
            fs::copy(&entry_path, &target).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    entry_path.display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

/// Append `entry` to `manifest_path`, preserving LF line endings and a final newline.
pub(crate) fn append_released_entry(manifest_path: &Path, entry: &str) -> Result<()> {
    let mut contents = if manifest_path.exists() {
        fs::read_to_string(manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?
            .replace("\r\n", "\n")
    } else {
        String::new()
    };
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(entry);
    contents.push('\n');
    fs::write(manifest_path, contents.as_bytes())
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;
    Ok(())
}
