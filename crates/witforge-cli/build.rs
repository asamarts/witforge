//! Build script for `witforge-cli`: records Git build identity for `witforge --version`.
//!
//! - Exact clean release tag `v<CARGO_PKG_VERSION>` -> `<CARGO_PKG_VERSION>`
//! - Any other Git checkout -> `<CARGO_PKG_VERSION> (<short-sha>)` or
//!   `<CARGO_PKG_VERSION> (<short-sha>-dirty)`
//! - No Git metadata (crates.io install, source archive, `cargo package` verification) ->
//!   `<CARGO_PKG_VERSION>` without failing.
//! - Never embeds build timestamps so builds remain reproducible.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let pkg_version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let manifest_dir =
        env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);

    let build_version = git_build_version(&manifest_dir, &pkg_version).unwrap_or(pkg_version);
    println!("cargo:rustc-env=WITFORGE_BUILD_VERSION={build_version}");
}

fn git_build_version(manifest_dir: &Path, pkg_version: &str) -> Option<String> {
    // Only read Git metadata when this package's own Cargo.toml is tracked in the Git repo.
    // This keeps `cargo package` verification builds (extracted under `target/package/`) and
    // source-tarball / crates.io builds free of parent-repo Git metadata.
    let tracked = Command::new("git")
        .args(["ls-files", "--error-unmatch", "Cargo.toml"])
        .current_dir(manifest_dir)
        .output()
        .ok()?;
    if !tracked.status.success() {
        return None;
    }

    let git_dir_raw = git_stdout(manifest_dir, &["rev-parse", "--git-dir"])?;
    let git_common_raw = git_stdout(manifest_dir, &["rev-parse", "--git-common-dir"])?;
    let git_dir = resolve_path(manifest_dir, &git_dir_raw);
    let git_common_dir = resolve_path(manifest_dir, &git_common_raw);

    emit_rerun_watches(manifest_dir, &git_dir, &git_common_dir);

    let sha = git_stdout(manifest_dir, &["rev-parse", "--short", "HEAD"])?;
    if sha.is_empty() {
        return None;
    }

    let status_out = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .current_dir(manifest_dir)
        .output()
        .ok()?;
    if !status_out.status.success() {
        return None;
    }
    let dirty = !status_out.stdout.iter().all(u8::is_ascii_whitespace);

    let expected_tag = format!("v{pkg_version}");
    let tags = git_stdout(manifest_dir, &["tag", "--points-at", "HEAD"]).unwrap_or_default();
    let on_release_tag = tags.lines().any(|line| line.trim() == expected_tag);

    if on_release_tag && !dirty {
        Some(pkg_version.to_string())
    } else if dirty {
        Some(format!("{pkg_version} ({sha}-dirty)"))
    } else {
        Some(format!("{pkg_version} ({sha})"))
    }
}

fn git_stdout(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|s| s.trim().to_string())
}

fn resolve_path(base: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_relative() {
        base.join(path)
    } else {
        path
    }
}

fn emit_rerun_watches(manifest_dir: &Path, git_dir: &Path, git_common_dir: &Path) {
    let head = git_dir.join("HEAD");
    if head.exists() {
        println!("cargo:rerun-if-changed={}", head.display());
        if let Ok(head_text) = fs::read_to_string(&head)
            && let Some(ref_rel) = head_text.trim().strip_prefix("ref:")
        {
            let ref_path = git_common_dir.join(ref_rel.trim());
            if ref_path.exists() {
                println!("cargo:rerun-if-changed={}", ref_path.display());
            }
        }
    }
    for candidate in [
        git_dir.join("index"),
        git_common_dir.join("packed-refs"),
        manifest_dir.join("Cargo.toml"),
        manifest_dir.join("build.rs"),
    ] {
        if candidate.exists() {
            println!("cargo:rerun-if-changed={}", candidate.display());
        }
    }
    watch_rs_tree(&manifest_dir.join("src"));
    watch_rs_tree(&manifest_dir.join("../witforge/src"));
}

fn watch_rs_tree(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            watch_rs_tree(&path);
        } else if path.extension().and_then(std::ffi::OsStr::to_str) == Some("rs") {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}
