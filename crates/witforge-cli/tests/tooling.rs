//! Runs the repository's script and Action test suites from `cargo test`.
//!
//! The release scripts are tested by shell scripts and the GitHub Action by `node --test`. CI
//! runs both directly as well; running them here gives each suite a test name, so that a plan
//! task whose work they cover can cite it as evidence.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `bash`, or Git's on Windows. `None` where Windows has no Git Bash: the scripts are not
/// meant to run there.
fn bash() -> Option<PathBuf> {
    if cfg!(windows) {
        let git_bash = Path::new(r"C:\Program Files\Git\bin\bash.exe");
        git_bash.is_file().then(|| git_bash.to_path_buf())
    } else {
        Some(PathBuf::from("bash"))
    }
}

fn assert_success(label: &str, command: &mut Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{label} failed ({})\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn release_version_reference_script_passes_its_cases() {
    let Some(bash) = bash() else {
        return;
    };
    assert_success(
        "ci/test-release-version.sh",
        Command::new(bash)
            .current_dir(repository())
            .arg("ci/test-release-version.sh"),
    );
}

fn run_node_test_suite(script_rel_path: &str) {
    let mut command = Command::new("node");
    command
        .current_dir(repository())
        .args(["--test", script_rel_path]);
    // CI always has Node, so a missing interpreter is a failure there. A contributor without
    // Node still gets a passing `cargo test`, and a line saying what did not run.
    if let Err(error) = Command::new("node").arg("--version").output() {
        assert!(
            error.kind() == ErrorKind::NotFound && std::env::var_os("CI").is_none(),
            "cannot run node: {error}"
        );
        eprintln!("node is not installed; {script_rel_path} did not run");
        return;
    }
    assert_success(&format!("node --test {script_rel_path}"), &mut command);
}

#[test]
fn action_unit_tests_pass_under_node() {
    run_node_test_suite("action/test.js");
}

#[test]
fn site_script_unit_tests_pass_under_node() {
    run_node_test_suite("site/scripts/test-scripts.mjs");
}
