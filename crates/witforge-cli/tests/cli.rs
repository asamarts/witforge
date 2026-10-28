//! Smoke tests for the command-line interface.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::lock::HEADER;

fn witforge() -> Command {
    Command::new(env!("CARGO_BIN_EXE_witforge"))
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../witforge/tests/fixtures/ok/plugin.wit")
        .canonicalize()
        .unwrap()
}

fn temp_contract(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("witforge-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::copy(fixture(), dir.join("contract.wit")).unwrap();
    dir
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn versions(name: &str) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../witforge/tests/fixtures/versions");
    let destination =
        std::env::temp_dir().join(format!("witforge-versions-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&destination);
    copy_tree(&source, &destination);
    destination
}

#[test]
fn help_version_and_subcommand_help() {
    let version = witforge().arg("--version").output().unwrap();
    assert!(version.status.success());
    assert!(
        String::from_utf8_lossy(&version.stdout).contains(env!("CARGO_PKG_VERSION")),
        "{version:?}"
    );

    for args in [&["--help"][..], &["check", "--help"][..], &["help"][..]] {
        let output = witforge().args(args).output().unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .to_lowercase()
                .contains("usage: witforge"),
            "{args:?}: {output:?}"
        );
    }
}

#[test]
fn lock_then_check_succeeds() {
    let dir = temp_contract("lock-check");
    let lock = witforge().arg("lock").arg(&dir).output().unwrap();
    assert!(
        lock.status.success(),
        "lock failed: {}",
        String::from_utf8_lossy(&lock.stderr)
    );
    assert!(dir.join("wire.lock").is_file());

    let check = witforge().arg("check").arg(&dir).output().unwrap();
    assert!(
        check.status.success(),
        "check failed: {}",
        String::from_utf8_lossy(&check.stderr)
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn alint_contract_check_succeeds() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../witforge/tests/fixtures/contracts/alint-plugin")
        .canonicalize()
        .unwrap();
    let check = witforge().arg("check").arg(&dir).output().unwrap();
    assert!(
        check.status.success(),
        "check failed: {}",
        String::from_utf8_lossy(&check.stderr)
    );
}

#[test]
fn check_reports_subset_violations_without_a_model_failure() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../witforge/tests/fixtures/refused/resource")
        .canonicalize()
        .unwrap();
    let check = witforge().arg("check").arg(&dir).output().unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        stderr.contains("resources and handles are not supported"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("contract does not satisfy the subset"),
        "the CLI should report the violations, not the model-build error: {stderr}"
    );
}

#[test]
fn lock_does_not_write_when_a_live_type_changes() {
    let dir = temp_contract("lock-type-change");
    let first = witforge().arg("lock").arg(&dir).output().unwrap();
    assert!(first.status.success(), "{first:?}");
    let path = dir.join("wire.lock");
    let original = fs::read_to_string(&path).unwrap();
    let changed = original.replace(
        "example:plugin/types/span.start = 1 field u64",
        "example:plugin/types/span.start = 1 field u16",
    );
    assert_ne!(changed, original);
    fs::write(&path, &changed).unwrap();

    let second = witforge().arg("lock").arg(&dir).output().unwrap();
    assert!(!second.status.success(), "{second:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), changed);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_reports_an_unlocked_member() {
    let dir = temp_contract("unlocked");
    let check = witforge().arg("check").arg(&dir).output().unwrap();
    assert_eq!(check.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&check.stderr).contains("has no wire number"),
        "{check:?}"
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn first_lock_succeeds_and_check_accepts_it() {
    let dir = temp_contract("first");
    let lock = witforge()
        .arg("lock")
        .arg(&dir)
        .arg("--first")
        .output()
        .unwrap();
    assert!(lock.status.success(), "{lock:?}");
    let contents = fs::read_to_string(dir.join("wire.lock")).unwrap();
    assert!(contents.starts_with(&format!("{HEADER}\n")));
    let check = witforge().arg("check").arg(&dir).output().unwrap();
    assert!(check.status.success(), "{check:?}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn lock_and_check_accept_an_against_baseline() {
    let dir = temp_contract("against-ok");
    let first = witforge()
        .arg("lock")
        .arg(&dir)
        .arg("--first")
        .output()
        .unwrap();
    assert!(first.status.success(), "{first:?}");
    let previous = dir
        .parent()
        .unwrap()
        .join(format!("previous-ok-{}.lock", std::process::id()));
    fs::copy(dir.join("wire.lock"), &previous).unwrap();
    let lock = witforge()
        .arg("lock")
        .arg(&dir)
        .arg("--against")
        .arg(&previous)
        .output()
        .unwrap();
    assert!(lock.status.success(), "{lock:?}");
    let check = witforge()
        .arg("check")
        .arg(&dir)
        .arg("--against")
        .arg(&previous)
        .output()
        .unwrap();
    assert!(check.status.success(), "{check:?}");
    fs::remove_file(previous).unwrap();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn first_and_against_are_mutually_exclusive() {
    let dir = temp_contract("conflict");
    let output = witforge()
        .args([
            "lock",
            dir.to_str().unwrap(),
            "--first",
            "--against",
            "previous.lock",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot be used"),
        "{output:?}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn against_requires_the_current_lockfile() {
    let dir = temp_contract("against-missing");
    let previous = dir
        .parent()
        .unwrap()
        .join(format!("previous-{}.lock", std::process::id()));
    fs::write(&previous, format!("{HEADER}\n")).unwrap();
    let output = witforge()
        .arg("lock")
        .arg(&dir)
        .arg("--against")
        .arg(&previous)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(!dir.join("wire.lock").exists());
    fs::remove_file(previous).unwrap();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn lock_refuses_a_released_directory() {
    let dir = temp_contract("released");
    fs::write(dir.join("wire.lock"), format!("{HEADER}\n")).unwrap();
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    let manifest = dir
        .parent()
        .unwrap()
        .join(format!("released-{}.txt", std::process::id()));
    fs::write(&manifest, format!("{name}\n")).unwrap();
    let output = witforge()
        .arg("lock")
        .arg(&dir)
        .arg("--released")
        .arg(&manifest)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("released"),
        "{output:?}"
    );
    assert_eq!(
        fs::read_to_string(dir.join("wire.lock")).unwrap(),
        format!("{HEADER}\n")
    );
    fs::remove_file(manifest).unwrap();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn evolution_fixture_check_and_lock_against_are_clean() {
    let root = versions("clean");
    let v01 = root.join("wit/v0.1");
    let v02 = root.join("wit/v0.2");
    let baseline = v01.join("wire.lock");
    let check = witforge()
        .arg("check")
        .arg(&v02)
        .arg("--against")
        .arg(&baseline)
        .output()
        .unwrap();
    assert!(check.status.success(), "{check:?}");
    let before = fs::read(v02.join("wire.lock")).unwrap();
    let lock = witforge()
        .arg("lock")
        .arg(&v02)
        .arg("--against")
        .arg(&baseline)
        .output()
        .unwrap();
    assert!(lock.status.success(), "{lock:?}");
    assert_eq!(fs::read(v02.join("wire.lock")).unwrap(), before);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn released_v0_1_refuses_locking() {
    let root = versions("released");
    let v01 = root.join("wit/v0.1");
    let output = witforge().arg("lock").arg(&v01).output().unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("released"),
        "{output:?}"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn reintroduced_overlay_is_refused_without_writing() {
    let root = versions("reintroduced");
    let v01 = root.join("wit/v0.1");
    let v02 = root.join("wit/v0.2");
    fs::copy(
        root.join("reintroduced/contract.wit"),
        v02.join("contract.wit"),
    )
    .unwrap();
    let before = fs::read(v02.join("wire.lock")).unwrap();
    let check = witforge()
        .arg("check")
        .arg(&v02)
        .arg("--against")
        .arg(v01.join("wire.lock"))
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        stderr.contains(
            "example:evolve/types/user.removed was removed and its name and number 4 stay reserved"
        ),
        "{stderr}"
    );
    let lock = witforge()
        .arg("lock")
        .arg(&v02)
        .arg("--against")
        .arg(v01.join("wire.lock"))
        .output()
        .unwrap();
    assert_eq!(lock.status.code(), Some(2), "{lock:?}");
    assert_eq!(fs::read(v02.join("wire.lock")).unwrap(), before);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn retyped_draft_is_updated_by_against() {
    let root = versions("retyped");
    let v01 = root.join("wit/v0.1");
    let v02 = root.join("wit/v0.2");
    fs::copy(root.join("retyped/contract.wit"), v02.join("contract.wit")).unwrap();
    let lock = witforge()
        .arg("lock")
        .arg(&v02)
        .arg("--against")
        .arg(v01.join("wire.lock"))
        .output()
        .unwrap();
    assert!(lock.status.success(), "{lock:?}");
    let text = fs::read_to_string(v02.join("wire.lock")).unwrap();
    assert!(
        text.contains("example:evolve/types/user.new-field = 5 field u16"),
        "{text}"
    );
    let check = witforge()
        .arg("check")
        .arg(&v02)
        .arg("--against")
        .arg(v01.join("wire.lock"))
        .output()
        .unwrap();
    assert!(check.status.success(), "{check:?}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn first_mode_drops_stale_drafts() {
    let root = versions("first");
    let v02 = root.join("wit/v0.2");
    let lock = witforge()
        .arg("lock")
        .arg(&v02)
        .arg("--first")
        .output()
        .unwrap();
    assert!(lock.status.success(), "{lock:?}");
    let stdout = String::from_utf8_lossy(&lock.stdout);
    for key in [
        "example:evolve/types/user.old-field",
        "example:evolve/types/user.rename-me",
        "example:evolve/types/user.removed",
    ] {
        assert!(stdout.contains(key), "{stdout}");
    }
    let text = fs::read_to_string(v02.join("wire.lock")).unwrap();
    assert!(!text.contains("old-field"), "{text}");
    assert!(!text.contains("rename-me"), "{text}");
    assert!(!text.contains("removed"), "{text}");
    fs::remove_dir_all(&root).unwrap();
}
