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

        // Advance through leading whitespace and `/* ... */` block comments on this line so
        // `/* doc */ package ex:foo@0.1.0;` and multi-line comment endings before `package` are
        // handled accurately.
        let bytes = body.as_bytes();
        let mut i = 0usize;
        let mut code_start = None;
        while i < bytes.len() {
            if in_block_comment {
                if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
            } else if bytes[i] == b' ' || bytes[i] == b'\t' {
                i += 1;
            } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
                break;
            } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                in_block_comment = true;
                i += 2;
            } else {
                code_start = Some(i);
                break;
            }
        }

        let mut replaced = None;
        if let Some(start) = code_start {
            let code_tail = &body[start..];
            if (code_tail.starts_with("package ") || code_tail.starts_with("package\t"))
                && let Some(rel_pos) = code_tail.find(&old_suffix)
            {
                let pos = start + rel_pos;
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
            // Track any block comment opened or closed later on the same line.
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

/// Append `entry` to `manifest_path` atomically, refusing symlinks and preserving LF line endings
/// and a final newline.
pub(crate) fn append_released_entry(manifest_path: &Path, entry: &str) -> Result<()> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    if let Ok(meta) = fs::symlink_metadata(manifest_path)
        && meta.file_type().is_symlink()
    {
        return Err(op_error(
            codes::GUARD_REFUSED,
            format!(
                "released manifest `{}` is a symlink; commit a regular file",
                manifest_path.display()
            ),
            Some(manifest_path.display().to_string()),
        ));
    }
    let existing_perms = fs::metadata(manifest_path).ok().map(|m| m.permissions());
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

    let parent = manifest_path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = manifest_path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("released");
    let pid = std::process::id();
    let mut attempt = 0u32;
    let (tmp_path, mut tmp_file) = loop {
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".witforge.{file_name}.{pid}.{seq}.tmp"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists && attempt < 64 => {
                attempt += 1;
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed to create temporary file beside {}",
                        manifest_path.display()
                    )
                });
            }
        }
    };
    let write_res = (|| -> Result<()> {
        tmp_file
            .write_all(contents.as_bytes())
            .and_then(|()| tmp_file.sync_all())
            .with_context(|| format!("failed to write {}", tmp_path.display()))?;
        drop(tmp_file);
        if let Some(perms) = existing_perms {
            let _ = fs::set_permissions(&tmp_path, perms);
        }
        fs::rename(&tmp_path, manifest_path).with_context(|| {
            format!(
                "failed to replace {} with {}",
                manifest_path.display(),
                tmp_path.display()
            )
        })?;
        Ok(())
    })();
    if let Err(err) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_package_version_handles_inline_and_closing_block_comments() {
        let input =
            "/* header */ package ex:demo@0.1.0;\ninterface a {\n    use ex:dep/t@0.1.0.{x};\n}\n";
        let got = rewrite_package_version(input, "0.1.0", "0.2.0");
        assert!(got.contains("/* header */ package ex:demo@0.2.0;"), "{got}");
        assert!(got.contains("use ex:dep/t@0.1.0.{x};"), "{got}");

        let multiline = "/*\n package ex:fake@0.1.0;\n */ package ex:demo@0.1.0;\r\n";
        let got2 = rewrite_package_version(multiline, "0.1.0", "1.0.0");
        assert!(got2.contains(" package ex:fake@0.1.0;\n"), "{got2}");
        assert!(got2.contains(" */ package ex:demo@1.0.0;\r\n"), "{got2}");
    }
}
