//! `witforge diff` coverage for every change category and compatibility view.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn witforge() -> Command {
    Command::new(env!("CARGO_BIN_EXE_witforge"))
}

fn workspace(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("witforge-diff-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn contract(root: &Path, version: &str, body: &str) {
    let dir = root.join(version);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("contract.wit"), body).unwrap();
}

fn run(args: &[&str]) -> Output {
    witforge().args(args).output().unwrap()
}

fn lock(root: &Path, version: &str) {
    let output = run(&["lock", &root.join(version).display().to_string(), "--first"]);
    assert!(
        output.status.success(),
        "lock {version}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn diff(old: &Path, new: &Path, extra: &[&str]) -> Output {
    let mut command = witforge();
    command.arg("diff").arg(old).arg(new).args(extra);
    command.output().unwrap()
}

#[test]
fn diff_reports_every_change_category_and_distinguishes_compatibility() {
    let root = workspace("categories");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types {
    record task { id: u8, title: string, legacy: u8 }
    enum status { pending, done }
}
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types {
    record task { id: u64, title: string, note: option<string> }
    enum status { pending, failed }
}
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    lock(&root, "new/v0.2");

    let output = diff(&old, &new, &["--json"]);
    assert!(
        output.status.success(),
        "diff failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let changes = report["changes"].as_array().unwrap();
    assert!(changes.iter().any(|change| {
        change["kind"] == "added"
            && change["defaulted"] == true
            && change["path"] == "types/task.note"
            && change["wire_breaking"] == false
            && change["source_breaking"] == true
            && change["api_breaking"] == true
    }));
    assert!(changes.iter().any(|change| {
        change["kind"] == "removed"
            && change["path"] == "types/task.legacy"
            && change["wire_breaking"] == true
    }));
    assert!(changes.iter().any(|change| {
        change["kind"] == "retyped"
            && change["path"] == "types/task.id"
            && change["from"] == "u8"
            && change["to"] == "u64"
            && change["wire_breaking"] == true
    }));
    assert!(
        changes
            .iter()
            .any(|change| { change["kind"] == "removed" && change["path"] == "types/status.done" })
    );
    assert!(
        changes
            .iter()
            .any(|change| { change["kind"] == "added" && change["path"] == "types/status.failed" })
    );
    assert_eq!(report["summary"]["breaking"], 5);
    // The new lock was started fresh, so the two removals lost their old entries as well as
    // the explicit field retype.
    assert_eq!(report["summary"]["wire_breaking"], 3);

    let human = diff(&old, &new, &[]);
    assert!(human.status.success());
    let text = String::from_utf8_lossy(&human.stdout);
    for expected in [
        "added-defaulted",
        "removed",
        "retyped",
        "wire:BREAKING",
        "generated API:",
        "result: BREAKING",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }

    let checked = diff(&old, &new, &["--check"]);
    assert_eq!(checked.status.code(), Some(1), "{checked:?}");
    assert!(
        String::from_utf8_lossy(&checked.stderr).contains("--check failed"),
        "{checked:?}"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_reports_a_pure_reorder_as_source_breaking_only() {
    let root = workspace("reorder");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types {
    record task { first: u8, second: string }
}
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types {
    record task { second: string, first: u8 }
}
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    // Keep the wire numbers and order section from the old lockfile: a reorder is a source
    // change, not a renumbering, and `diff` reads the model for declaration order.
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let reorder = report["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["kind"] == "reordered")
        .expect("reordered change");
    assert_eq!(reorder["element"], "record field order");
    assert_eq!(reorder["wire_breaking"], false);
    assert_eq!(reorder["source_breaking"], true);
    assert_eq!(reorder["api_breaking"], false);
    assert_eq!(report["summary"]["wire_breaking"], 0);
    assert_eq!(report["summary"]["source_breaking"], 1);

    let checked = diff(&old, &new, &["--check"]);
    assert_eq!(checked.status.code(), Some(1));

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_accepts_roots_and_a_compatible_additive_change() {
    let root = workspace("additive");
    contract(
        &root,
        "old/wit/v0.1",
        r"
package example:tasks@0.1.0;
interface types {
    record task { id: u64 }
}
",
    );
    contract(
        &root,
        "new/wit/v0.2",
        r"
package example:tasks@0.2.0;
interface types {
    record task { id: u64 }
}
interface metadata {
    record info { name: string }
}
",
    );
    lock(&root, "old/wit/v0.1");
    lock(&root, "new/wit/v0.2");

    let output = diff(&root.join("old/wit"), &root.join("new/wit"), &["--check"]);
    assert!(
        output.status.success(),
        "additive diff should pass --check: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("added"), "{text}");
    assert!(text.contains("result: COMPATIBLE"), "{text}");
    assert!(!text.contains("result: BREAKING"), "{text}");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_rejects_a_live_member_without_a_wire_number() {
    let root = workspace("unlocked");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types {
    record task { id: u64 }
}
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types {
    record task { id: u64, note: string }
}
",
    );
    lock(&root, "old/v0.1");
    lock(&root, "new/v0.2");
    // Remove the new member's entry, as a draft that was never locked.
    let path = root.join("new/v0.2/wire.lock");
    let text = fs::read_to_string(&path).unwrap();
    let filtered = format!(
        "{}\n",
        text.lines()
            .filter(|line| !line.starts_with("example:tasks/types/task.note ="))
            .collect::<Vec<_>>()
            .join("\n")
    );
    fs::write(&path, filtered).unwrap();

    let output = diff(&root.join("old/v0.1"), &root.join("new/v0.2"), &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("has no wire number"),
        "{output:?}"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_marks_a_reserved_removal_as_wire_compatible() {
    let root = workspace("reserved-removal");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types {
    record task { id: u64, legacy: string }
}
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types {
    record task { id: u64 }
}
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();
    let output = run(&[
        "lock",
        &new.display().to_string(),
        "--against",
        &old.join("wire.lock").display().to_string(),
    ]);
    assert!(
        output.status.success(),
        "lock --against: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let removed = report["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["kind"] == "removed" && change["path"] == "types/task.legacy")
        .expect("removed field");
    assert_eq!(removed["wire_breaking"], false);
    assert_eq!(removed["source_breaking"], true);
    assert_eq!(removed["api_breaking"], true);
    assert_eq!(report["summary"]["wire_breaking"], 0);

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_reports_function_parameter_reordering_as_source_and_api_breaking() {
    let root = workspace("parameter-reorder");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface service {
    call: func(first: u8, second: string);
}
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface service {
    call: func(second: string, first: u8);
}
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let reorder = report["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["kind"] == "reordered" && change["element"] == "parameter order")
        .expect("parameter reorder");
    assert_eq!(reorder["wire_breaking"], false);
    assert_eq!(reorder["source_breaking"], true);
    assert_eq!(reorder["api_breaking"], true);

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_reports_a_function_result_retype_as_wire_and_api_breaking() {
    let root = workspace("function-result");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface service {
    call: func() -> u8;
}
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface service {
    call: func() -> u64;
}
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    lock(&root, "new/v0.2");

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let retyped = report["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["kind"] == "retyped" && change["element"] == "function result")
        .expect("function result retype");
    assert_eq!(retyped["from"], "u8");
    assert_eq!(retyped["to"], "u64");
    assert_eq!(retyped["wire_breaking"], true);
    assert_eq!(retyped["source_breaking"], true);
    assert_eq!(retyped["api_breaking"], true);

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_reports_a_type_kind_change() {
    let root = workspace("kind-change");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types {
    record value { id: u8 }
}
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types {
    enum value { id }
}
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    lock(&root, "new/v0.2");

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let changed = report["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["kind"] == "kind-changed")
        .expect("kind change");
    assert_eq!(changed["path"], "types/value");
    assert_eq!(changed["from"], "record");
    assert_eq!(changed["to"], "enum");
    assert_eq!(changed["wire_breaking"], true);

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_reports_renumbering_as_wire_only() {
    let root = workspace("renumber");
    contract(
        &root,
        "old/v0.1",
        r"
package example:renumber@0.1.0;
interface types {
    record task { first: u8, second: u8 }
}
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:renumber@0.2.0;
interface types {
    record task { first: u8, second: u8 }
}
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();
    let lock_path = new.join("wire.lock");
    let text = fs::read_to_string(&lock_path)
        .unwrap()
        .replace(
            "example:renumber/types/task.first = 1 field u8",
            "example:renumber/types/task.first = 2 field u8",
        )
        .replace(
            "example:renumber/types/task.second = 2 field u8",
            "example:renumber/types/task.second = 1 field u8",
        );
    fs::write(&lock_path, text).unwrap();

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let changes = report["changes"].as_array().unwrap();
    let renumbered: Vec<_> = changes
        .iter()
        .filter(|change| change["kind"] == "renumbered")
        .collect();
    assert_eq!(renumbered.len(), 2);
    assert!(
        renumbered
            .iter()
            .all(|change| change["wire_breaking"] == true
                && change["source_breaking"] == false
                && change["api_breaking"] == false)
    );
    assert_eq!(report["summary"]["wire_breaking"], 2);
    assert_eq!(report["summary"]["source_breaking"], 0);

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_reports_world_imports_and_exports() {
    let root = workspace("worlds");
    contract(
        &root,
        "old/v0.1",
        r"
package example:worlds@0.1.0;
interface i { record r { x: u8 } }
interface j { record s { y: u8 } }
world w { export i; }
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:worlds@0.2.0;
interface i { record r { x: u8 } }
interface j { record s { y: u8 } }
world w { import j; export i; }
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    lock(&root, "new/v0.2");

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let changes = report["changes"].as_array().unwrap();
    let added = changes
        .iter()
        .find(|change| change["kind"] == "added" && change["element"] == "world import")
        .expect("world import addition");
    assert_eq!(added["path"], "w/import/j");
    assert_eq!(added["wire_breaking"], false);
    assert_eq!(added["source_breaking"], true);
    assert_eq!(added["api_breaking"], false);

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_refuses_different_package_names() {
    let root = workspace("packages");
    contract(
        &root,
        "old/v0.1",
        r"
package example:one@0.1.0;
interface i { record r { x: u8 } }
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:two@0.2.0;
interface i { record r { x: u8 } }
",
    );
    lock(&root, "old/v0.1");
    lock(&root, "new/v0.2");

    let output = diff(&root.join("old/v0.1"), &root.join("new/v0.2"), &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("example:one"), "{stderr}");
    assert!(stderr.contains("example:two"), "{stderr}");

    fs::remove_dir_all(&root).unwrap();
}
