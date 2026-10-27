//! Smoke tests for the command-line interface.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
            String::from_utf8_lossy(&output.stdout).contains("usage: witforge"),
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
