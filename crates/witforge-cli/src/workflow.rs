//! Configuration discovery and workflow helpers (`init`, `new-version`, `release`, `retire`).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use witforge::diagnostic::codes;
use witforge::workflow as contract_workflow;

use crate::op_error;

/// Default configuration file name searched upwards from the current working directory.
pub(crate) const CONFIG_FILE_NAME: &str = "witforge.toml";

/// Reserved WIT keywords (`wit-parser` 0.263.0) that cannot appear unescaped as a package
/// namespace or name in `package <namespace>:<name>@0.1.0;`.
const WIT_KEYWORDS: &[&str] = &[
    "_",
    "as",
    "async",
    "bool",
    "borrow",
    "char",
    "constructor",
    "enum",
    "error-context",
    "export",
    "f32",
    "f64",
    "flags",
    "from",
    "func",
    "future",
    "import",
    "include",
    "interface",
    "list",
    "map",
    "option",
    "own",
    "package",
    "record",
    "resource",
    "result",
    "s16",
    "s32",
    "s64",
    "s8",
    "static",
    "stream",
    "string",
    "tuple",
    "type",
    "u16",
    "u32",
    "u64",
    "u8",
    "use",
    "variant",
    "with",
    "world",
];

fn is_wit_keyword(ident: &str) -> bool {
    WIT_KEYWORDS.binary_search(&ident).is_ok()
}

/// Validate a WIT package name `<namespace>:<name>` (kebab-case ASCII identifiers).
fn parse_package_id(raw: &str) -> Result<(String, String)> {
    let Some((namespace, name)) = raw.split_once(':') else {
        return Err(op_error(
            codes::CLI_USAGE,
            format!(
                "invalid `--package` value `{raw}`; expected `<namespace>:<name>` (for example \
                 `example:hello`)"
            ),
            None,
        ));
    };
    if name.contains(':') || !is_wit_kebab_ident(namespace) || !is_wit_kebab_ident(name) {
        return Err(op_error(
            codes::CLI_USAGE,
            format!(
                "invalid `--package` value `{raw}`; namespace and name must be lowercase ASCII \
                 kebab-case identifiers (for example `example:hello`)"
            ),
            None,
        ));
    }
    if is_wit_keyword(namespace) || is_wit_keyword(name) {
        return Err(op_error(
            codes::CLI_USAGE,
            format!(
                "invalid `--package` value `{raw}`; `{}` is a reserved WIT keyword",
                if is_wit_keyword(namespace) {
                    namespace
                } else {
                    name
                }
            ),
            None,
        ));
    }
    Ok((namespace.to_string(), name.to_string()))
}

fn is_wit_kebab_ident(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    text.split('-').all(|word| {
        let mut bytes = word.bytes();
        let Some(first) = bytes.next() else {
            return false;
        };
        first.is_ascii_lowercase() && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    })
}

fn clean_lexical_path(path: &Path) -> PathBuf {
    use std::path::Component;
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
    parts.into_iter().collect()
}

fn derive_package_name(dir: &Path) -> String {
    let cleaned = clean_lexical_path(dir);
    let candidate = if let Some(name) = cleaned
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
    {
        name.to_string()
    } else if let Ok(canon) = fs::canonicalize(dir)
        && let Some(name) = canon.file_name().and_then(std::ffi::OsStr::to_str)
    {
        name.to_string()
    } else if let Ok(cwd) = std::env::current_dir()
        && let Some(name) = cwd.file_name().and_then(std::ffi::OsStr::to_str)
    {
        name.to_string()
    } else {
        "contract".to_string()
    };

    let mut segments = Vec::new();
    let mut current = String::new();
    for ch in candidate.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            segments.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    let valid_segments: Vec<String> = segments
        .into_iter()
        .filter(|seg| seg.bytes().next().is_some_and(|b| b.is_ascii_lowercase()))
        .collect();
    if valid_segments.is_empty() {
        "contract".to_string()
    } else {
        let joined = valid_segments.join("-");
        if is_wit_keyword(&joined) {
            format!("{joined}-contract")
        } else {
            joined
        }
    }
}

/// Scaffold a new witforge contract project under `dir` with `wit/v0.1/contract.wit`, an initial
/// `wit/v0.1/wire.lock`, and `witforge.toml`, refusing to overwrite any existing file.
#[allow(clippy::too_many_lines)]
pub(crate) fn init_project(
    dir: &Path,
    package: Option<&str>,
    serde: bool,
) -> Result<(String, PathBuf)> {
    let (namespace, name) = match package {
        Some(raw) => parse_package_id(raw)?,
        None => ("example".to_string(), derive_package_name(dir)),
    };
    let package_id = format!("{namespace}:{name}");

    // Strip trailing slashes so `symlink_metadata` does not follow a symlinked target directory.
    let norm_dir: PathBuf = dir.components().collect();
    if let Ok(meta) = fs::symlink_metadata(&norm_dir)
        && (meta.file_type().is_symlink() || !meta.is_dir())
    {
        return Err(op_error(
            codes::GUARD_REFUSED,
            format!(
                "target path `{}` is a symlink or not a directory",
                dir.display()
            ),
            Some(dir.display().to_string()),
        ));
    }

    let wit_dir = norm_dir.join("wit");
    let v01_dir = wit_dir.join("v0.1");
    let contract_path = v01_dir.join("contract.wit");
    let lock_path = v01_dir.join("wire.lock");
    let config_path = norm_dir.join(CONFIG_FILE_NAME);

    for check_dir in [&wit_dir, &v01_dir] {
        if let Ok(meta) = fs::symlink_metadata(check_dir)
            && (meta.file_type().is_symlink() || !meta.is_dir())
        {
            return Err(op_error(
                codes::GUARD_REFUSED,
                format!("`{}` is a symlink or not a directory", check_dir.display()),
                Some(check_dir.display().to_string()),
            ));
        }
    }

    for target in [&contract_path, &lock_path, &config_path] {
        if fs::symlink_metadata(target).is_ok() {
            return Err(op_error(
                codes::GUARD_REFUSED,
                format!(
                    "refusing to overwrite existing `{}`; remove it or choose another directory",
                    target.display()
                ),
                Some(target.display().to_string()),
            ));
        }
    }

    // Refuse initializing into a non-empty `wit/` or `wit/v0.1/` directory.
    if let Ok(mut entries) = fs::read_dir(&v01_dir)
        && entries.next().transpose()?.is_some()
    {
        return Err(op_error(
            codes::GUARD_REFUSED,
            format!(
                "refusing to initialize into non-empty `{}`; choose an empty directory",
                v01_dir.display()
            ),
            Some(v01_dir.display().to_string()),
        ));
    }
    if let Ok(mut entries) = fs::read_dir(&wit_dir)
        && let Some(first) = entries.next().transpose()?
    {
        return Err(op_error(
            codes::GUARD_REFUSED,
            format!(
                "refusing to initialize into non-empty `{}` (`{}` already exists)",
                wit_dir.display(),
                first.path().display()
            ),
            Some(first.path().display().to_string()),
        ));
    }

    // Record every missing directory from `v01_dir` upwards so rollback leaves no stray ancestors.
    let mut dirs_to_create = Vec::new();
    let mut cursor = Some(v01_dir.as_path());
    while let Some(path) = cursor {
        if path.as_os_str().is_empty() || fs::symlink_metadata(path).is_ok() {
            break;
        }
        dirs_to_create.push(path.to_path_buf());
        cursor = path.parent();
    }

    let starter_wit = format!(
        "/// Starter contract for `{package_id}`.\n\
         package {package_id}@0.1.0;\n\
         \n\
         /// Core contract types and functions.\n\
         interface api {{\n\
             /// One example message.\n\
             record message {{\n\
                 /// Stable identifier.\n\
                 id: u64,\n\
                 /// Message text.\n\
                 text: string,\n\
             }}\n\
         \n\
             /// Echo a message.\n\
             echo: func(input: message) -> message;\n\
         }}\n\
         \n\
         /// Contract world.\n\
         world service {{\n\
             export api;\n\
         }}\n"
    );

    let serde_line = if serde {
        "serde = true\n"
    } else {
        "# serde = true\n"
    };
    let starter_toml = format!(
        "version = 1\n\
         \n\
         [project.{name}]\n\
         root = \"wit\"\n\
         \n\
         [project.{name}.rust]\n\
         out = \"generated/rust\"\n\
         {serde_line}\
         \n\
         [project.{name}.proto]\n\
         out = \"generated/proto\"\n\
         \n\
         [project.{name}.schema]\n\
         out = \"generated/schema\"\n\
         \n\
         [project.{name}.glue]\n\
         out = \"generated/glue\"\n\
         plain-root = \"crate::generated::rust\"\n\
         proto-root = \"crate::generated::proto\"\n"
    );

    let populate = (|| -> Result<()> {
        fs::create_dir_all(&v01_dir)
            .with_context(|| format!("failed to create {}", v01_dir.display()))?;
        write_atomic_manifest(&contract_path, &starter_wit)?;
        let lock_options = contract_workflow::VersionOptions::new(v01_dir.clone())
            .with_baseline(contract_workflow::Baseline::First);
        let outcome = contract_workflow::lock(&lock_options)?;
        if !outcome.is_ok() {
            return Err(op_error(
                codes::CLI_USAGE,
                format!(
                    "failed to initialize starter contract for `{package_id}`: {}",
                    outcome
                        .report
                        .findings
                        .first()
                        .map_or("contract check failed", |f| f.human.as_str())
                ),
                Some(contract_path.display().to_string()),
            ));
        }
        outcome.write()?;
        write_atomic_manifest(&config_path, &starter_toml)?;
        Ok(())
    })();

    if let Err(err) = populate {
        let _ = fs::remove_file(&config_path);
        let _ = fs::remove_file(&lock_path);
        let _ = fs::remove_file(&contract_path);
        for created in &dirs_to_create {
            let _ = fs::remove_dir(created);
        }
        return Err(err);
    }

    Ok((package_id, config_path))
}

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

/// Parse a canonical `v<major>.<minor>` version directory name.
fn parse_version_dir_name(raw: &str) -> Option<(u64, u64)> {
    let (major, minor) = raw.strip_prefix('v')?.split_once('.')?;
    let canonical = |digits: &str| {
        !digits.is_empty()
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && (digits == "0" || !digits.starts_with('0'))
    };
    if !canonical(major) || !canonical(minor) {
        return None;
    }
    Some((major.parse().ok()?, minor.parse().ok()?))
}

pub(crate) fn ensure_manifest_belongs_to_wit_root(
    manifest_path: &Path,
    wit_root: &Path,
) -> Result<()> {
    let manifest_file = manifest_path.display().to_string();
    let normalized: PathBuf = manifest_path.components().collect();
    if normalized.file_name().is_none() {
        return Err(op_error(
            codes::GUARD_REFUSED,
            format!("released manifest `{manifest_file}` is not a file path"),
            Some(manifest_file),
        ));
    }
    let parent = normalized
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canon_parent = fs::canonicalize(parent).map_err(|err| {
        op_error(
            codes::GUARD_REFUSED,
            format!(
                "released manifest `{manifest_file}` parent `{}` cannot be resolved: {err}",
                parent.display()
            ),
            Some(manifest_file.clone()),
        )
    })?;
    if canon_parent != wit_root {
        return Err(op_error(
            codes::GUARD_REFUSED,
            format!(
                "released manifest `{manifest_file}` belongs to `{}`, not `{}`",
                canon_parent.display(),
                wit_root.display()
            ),
            Some(manifest_file),
        ));
    }
    Ok(())
}

/// Create the next minor (`v<major>.<minor+1>`) or next major (`v<major+1>.0`) version directory
/// under `wit_root` by copying WIT files, `deps/`, and (for a minor only) `wire.lock`, rewriting
/// the `package ...@<old>;` version header in every root `.wit` file.
///
/// When `from_version` is `Some("v<major>.<minor>")`, clones that explicit source version instead
/// of the globally latest version (allowing maintenance minors on an earlier major line).
#[allow(clippy::too_many_lines)]
pub(crate) fn create_new_version(
    wit_root: &Path,
    major_bump: bool,
    from_version: Option<&str>,
    released_manifest: Option<(&Path, bool)>,
) -> Result<(PathBuf, PathBuf, bool)> {
    if !wit_root.is_dir() {
        return Err(op_error(
            codes::IO_ERROR,
            format!("WIT root `{}` is not a directory", wit_root.display()),
            Some(wit_root.display().to_string()),
        ));
    }
    let versions = witforge::discover_version_directories(wit_root)?;
    let (latest_major, latest_minor, latest_dir) = if let Some(raw_from) = from_version {
        let Some((want_major, want_minor)) = parse_version_dir_name(raw_from) else {
            return Err(op_error(
                codes::CLI_USAGE,
                format!(
                    "invalid `--from` version `{raw_from}`; expected `v<major>.<minor>` (for \
                     example `v0.2`)"
                ),
                None,
            ));
        };
        let Some(found) = versions
            .iter()
            .find(|(major, minor, _)| *major == want_major && *minor == want_minor)
        else {
            return Err(op_error(
                codes::IO_ERROR,
                format!(
                    "source version `{raw_from}` was not found under `{}`",
                    wit_root.display()
                ),
                Some(wit_root.display().to_string()),
            ));
        };
        found.clone()
    } else {
        versions
            .last()
            .expect("discover_version_directories never returns empty")
            .clone()
    };
    witforge::ensure_version_directory(&latest_dir)?;

    let max_major = versions
        .iter()
        .map(|(major, _, _)| *major)
        .max()
        .expect("discover_version_directories never returns empty");

    // Validate the released manifest unconditionally (including on `--major`) so a missing
    // explicit/configured manifest, symlink, cross-root manifest, or malformed manifest is never
    // silently ignored.
    let canon_wit_root = fs::canonicalize(wit_root)
        .with_context(|| format!("failed to resolve WIT root {}", wit_root.display()))?;
    let (manifest_path, explicit_manifest) = match released_manifest {
        Some((path, explicit)) => (path.to_path_buf(), explicit),
        None => (wit_root.join("released"), false),
    };
    let normalized_manifest: PathBuf = manifest_path.components().collect();
    let manifest_meta = match fs::symlink_metadata(&normalized_manifest) {
        Ok(meta) => Some(meta),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if explicit_manifest {
                return Err(op_error(
                    codes::IO_ERROR,
                    format!(
                        "released manifest `{}` does not exist",
                        manifest_path.display()
                    ),
                    Some(manifest_path.display().to_string()),
                ));
            }
            None
        }
        Err(err) => {
            return Err(op_error(
                codes::IO_ERROR,
                format!(
                    "failed to inspect released manifest `{}`: {err}",
                    manifest_path.display()
                ),
                Some(manifest_path.display().to_string()),
            ));
        }
    };
    if let Some(meta) = &manifest_meta
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
    if explicit_manifest || manifest_meta.is_some() {
        ensure_manifest_belongs_to_wit_root(&manifest_path, &canon_wit_root)?;
    }
    let loaded_manifest = if manifest_meta.is_some() {
        Some(witforge::ReleasedManifest::load(&manifest_path)?)
    } else {
        None
    };

    if !major_bump {
        let highest_in_major = versions
            .iter()
            .filter(|(major, _, _)| *major == latest_major)
            .map(|(_, minor, _)| *minor)
            .max()
            .expect("source major exists in versions");
        if latest_minor < highest_in_major {
            return Err(op_error(
                codes::GUARD_REFUSED,
                format!(
                    "cannot create a minor bump from `v{latest_major}.{latest_minor}` because \
                     `v{latest_major}.{highest_in_major}` already exists; use `--from \
                     v{latest_major}.{highest_in_major}` (or pass `--major`)"
                ),
                Some(latest_dir.display().to_string()),
            ));
        }
        // Refuse resurrecting a completely retired major line.
        if let Some(manifest) = &loaded_manifest {
            let all_major_retired = versions
                .iter()
                .filter(|(major, _, _)| *major == latest_major)
                .all(|(major, minor, _)| manifest.is_retired(&format!("v{major}.{minor}")));
            if all_major_retired {
                return Err(op_error(
                    codes::GUARD_REFUSED,
                    format!(
                        "cannot create a new minor on major {latest_major}: every version of \
                         major {latest_major} is retired in {}",
                        manifest_path.display()
                    ),
                    Some(manifest_path.display().to_string()),
                ));
            }
        }
    }

    let (target_major, target_minor) = if major_bump {
        let next_major = max_major.checked_add(1).ok_or_else(|| {
            op_error(
                codes::CLI_USAGE,
                format!("major version `{max_major}` overflows u64"),
                None,
            )
        })?;
        (next_major, 0)
    } else {
        let next_minor = latest_minor.checked_add(1).ok_or_else(|| {
            op_error(
                codes::CLI_USAGE,
                format!("minor version `{latest_minor}` overflows u64"),
                None,
            )
        })?;
        (latest_major, next_minor)
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
                "source version directory `{}` has no `wire.lock`; lock `{}` before starting a \
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

        let mut rewritten_headers = 0usize;
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
                let (rewritten, count) =
                    rewrite_package_version_with_count(&text, &old_ver, &new_ver);
                rewritten_headers += count;
                fs::write(target_dir.join(name), rewritten).with_context(|| {
                    format!("failed to write {}", target_dir.join(name).display())
                })?;
            } else if meta.is_dir() && name == "deps" {
                copy_dir_no_symlinks(&entry_path, &target_dir.join("deps"))?;
            }
        }
        if rewritten_headers == 0 {
            return Err(op_error(
                codes::PARSE_ERROR,
                format!(
                    "no `package ...@{old_ver};` header was found in `{}` to rewrite to `{new_ver}`",
                    latest_dir.display()
                ),
                Some(latest_dir.display().to_string()),
            ));
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
/// leaving comments (including nested `/* ... /* ... */ ... */` block comments), `use` imports,
/// and line endings untouched.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn rewrite_package_version(text: &str, old_ver: &str, new_ver: &str) -> String {
    rewrite_package_version_with_count(text, old_ver, new_ver).0
}

#[allow(clippy::too_many_lines)]
pub(crate) fn rewrite_package_version_with_count(
    text: &str,
    old_ver: &str,
    new_ver: &str,
) -> (String, usize) {
    let old_suffix = format!("@{old_ver}");
    let new_suffix = format!("@{new_ver}");
    let old_bytes = old_suffix.as_bytes();
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + 8);
    let mut last_copied = 0usize;
    let mut i = if text.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    };
    let mut comment_depth = 0usize;
    let mut in_line_comment = false;
    let mut in_string = false;
    let mut brace_depth = 0usize;
    let mut in_package_header = false;
    let mut pending_replace: Option<(usize, usize)> = None;
    let mut count = 0usize;

    let is_ident_byte = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'%');

    while i < bytes.len() {
        if in_line_comment {
            if bytes[i] == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if comment_depth > 0 {
            if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                comment_depth += 1;
                i += 2;
            } else if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                comment_depth -= 1;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if in_string {
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            in_line_comment = true;
            i += 2;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            comment_depth = 1;
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if bytes[i] == b'{' {
            // Nested `package other:dep@0.1.0 { ... }` block: do not rewrite.
            in_package_header = false;
            pending_replace = None;
            brace_depth += 1;
            i += 1;
            continue;
        }
        if bytes[i] == b'}' {
            in_package_header = false;
            pending_replace = None;
            brace_depth = brace_depth.saturating_sub(1);
            i += 1;
            continue;
        }
        if in_package_header {
            if bytes[i] == b';' {
                if let Some((start, end)) = pending_replace.take() {
                    out.push_str(&text[last_copied..start]);
                    out.push_str(&new_suffix);
                    last_copied = end;
                    count += 1;
                }
                in_package_header = false;
                i += 1;
                continue;
            }
            if pending_replace.is_none() && bytes[i..].starts_with(old_bytes) {
                let end = i + old_bytes.len();
                let boundary_ok = bytes
                    .get(end)
                    .is_none_or(|&b| matches!(b, b';' | b' ' | b'\t' | b'\r' | b'\n' | b'/'));
                if boundary_ok {
                    pending_replace = Some((i, end));
                    i = end;
                    continue;
                }
            }
            i += 1;
            continue;
        }
        if brace_depth == 0
            && bytes[i..].starts_with(b"package")
            && (i == 0 || !is_ident_byte(bytes[i - 1]))
        {
            let after_kw = i + b"package".len();
            let kw_end_ok = bytes
                .get(after_kw)
                .is_none_or(|&b| matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'/'));
            if kw_end_ok {
                in_package_header = true;
                pending_replace = None;
                i = after_kw;
                continue;
            }
        }
        i += 1;
    }

    out.push_str(&text[last_copied..]);
    (out, count)
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
        } else if meta.is_file() {
            fs::copy(&entry_path, &target).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    entry_path.display(),
                    target.display()
                )
            })?;
        } else {
            return Err(op_error(
                codes::GUARD_REFUSED,
                format!(
                    "dependency tree contains non-regular file `{}`; new-version refuses to copy special files",
                    entry_path.display()
                ),
                Some(entry_path.display().to_string()),
            ));
        }
    }
    Ok(())
}

/// Rewrite an active `<entry>` line in a `released` manifest to `<entry> retired`, preserving
/// comments, blank lines, ordering, and LF line endings.
pub(crate) fn retire_released_entry_text(raw: &str, entry: &str) -> Result<String> {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let normalized = raw.replace("\r\n", "\n");
    let mut lines: Vec<String> = Vec::new();
    let mut updated = false;
    let body = normalized.strip_suffix('\n').unwrap_or(&normalized);
    for line in body.split('\n') {
        let trimmed = line.trim_matches([' ', '\t', '\r', '\x0b', '\x0c']);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            lines.push(line.to_string());
            continue;
        }
        let tokens: Vec<&str> = trimmed
            .split([' ', '\t'])
            .filter(|part| !part.is_empty())
            .collect();
        if tokens.len() == 1 && tokens[0] == entry {
            updated = true;
            lines.push(format!("{entry} retired"));
        } else {
            lines.push(line.to_string());
        }
    }
    if !updated {
        return Err(op_error(
            codes::IO_ERROR,
            format!("entry `{entry}` was not found as a live release line"),
            None,
        ));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    Ok(out)
}

/// Append `entry` to `manifest_path` atomically, refusing symlinks and preserving LF line endings
/// and a final newline.
pub(crate) fn append_released_entry(manifest_path: &Path, entry: &str) -> Result<()> {
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
    write_atomic_manifest(manifest_path, &contents)
}

/// Atomically replace `manifest_path` with `contents`, refusing symlinks and preserving existing
/// file permissions.
pub(crate) fn write_atomic_manifest(manifest_path: &Path, contents: &str) -> Result<()> {
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

        // Nested WIT block comments and multi-line package headers.
        let nested = "/* outer /* inner package ex:nope@0.1.0; */ still comment package ex:nope2@0.1.0; */\n\
                      package\n  /* inline */ ex:demo@0.1.0;\n\
                      interface a { use ex:dep/t@0.1.0.{x}; }\n";
        let (got3, count3) = rewrite_package_version_with_count(nested, "0.1.0", "0.2.0");
        assert_eq!(count3, 1, "{got3}");
        assert!(got3.contains("inner package ex:nope@0.1.0;"), "{got3}");
        assert!(
            got3.contains("still comment package ex:nope2@0.1.0;"),
            "{got3}"
        );
        assert!(
            got3.contains("package\n  /* inline */ ex:demo@0.2.0;"),
            "{got3}"
        );
        assert!(got3.contains("use ex:dep/t@0.1.0.{x};"), "{got3}");

        // Multi-byte UTF-8 before `package`, string literals, and nested `package { ... }` blocks.
        let tricky = "// café 🚀\n\
                      package acme:demo@0.1.0;\n\
                      interface api {\n\
                          @external-id(\"package acme:demo@0.1.0;\")\n\
                          record r { id: u32 }\n\
                      }\n\
                      package nested:dep@0.1.0 {\n\
                          interface x {}\n\
                      }\n";
        let (got4, count4) = rewrite_package_version_with_count(tricky, "0.1.0", "0.2.0");
        assert_eq!(count4, 1, "{got4}");
        assert!(got4.contains("package acme:demo@0.2.0;"), "{got4}");
        assert!(
            got4.contains("@external-id(\"package acme:demo@0.1.0;\")"),
            "{got4}"
        );
        assert!(got4.contains("package nested:dep@0.1.0 {"), "{got4}");
    }

    #[test]
    fn parse_package_id_and_derive_package_name_reject_keywords_and_handle_trailing_slash() {
        for kw in [
            "record",
            "future",
            "stream",
            "error-context",
            "from",
            "with",
            "async",
        ] {
            assert!(parse_package_id(&format!("acme:{kw}")).is_err(), "{kw}");
            assert!(parse_package_id(&format!("{kw}:demo")).is_err(), "{kw}");
        }
        assert!(parse_package_id("acme:1bad").is_err());
        assert!(parse_package_id("acme:demo-2").is_err());
        assert!(parse_package_id("Acme:demo").is_err());
        assert_eq!(
            parse_package_id("acme:demo-v2").unwrap(),
            ("acme".to_string(), "demo-v2".to_string())
        );
        assert_eq!(
            parse_package_id("acme:demo2").unwrap(),
            ("acme".to_string(), "demo2".to_string())
        );
        assert_eq!(derive_package_name(Path::new("foo/bar/")), "bar");
        assert_eq!(derive_package_name(Path::new("future")), "future-contract");
        assert_eq!(
            retire_released_entry_text("\u{feff}v0.1\nv0.2\n", "v0.1").unwrap(),
            "v0.1 retired\nv0.2\n"
        );
    }
}
