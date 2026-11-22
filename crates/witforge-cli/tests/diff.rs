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

fn lock_against(root: &Path, version: &str, previous: &str) {
    let output = run(&[
        "lock",
        &root.join(version).display().to_string(),
        "--against",
        &root.join(previous).join("wire.lock").display().to_string(),
    ]);
    assert!(
        output.status.success(),
        "lock {version} --against {previous}: {}",
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

    // Root endpoints use the generator-grade loader, which refuses the stale order fingerprint
    // that the direct endpoint deliberately accepts in order to report the reorder.
    let roots = diff(&root.join("old"), &root.join("new"), &[]);
    assert_eq!(roots.status.code(), Some(1), "{roots:?}");
    assert!(
        String::from_utf8_lossy(&roots.stderr).contains("declaration order"),
        "{roots:?}"
    );

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
fn diff_refuses_stale_lockfiles_for_direct_version_endpoints() {
    let root = workspace("stale-lock");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types { record task { id: u8 } }
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types { record task { id: u8 } }
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    lock(&root, "new/v0.2");

    // The model says `u8`; the lockfile now claims `u64`. `check` rejects this, so `diff` must
    // reject it too instead of comparing the model and reporting a compatible result.
    let path = old.join("wire.lock");
    let text = fs::read_to_string(&path).unwrap();
    let stale = text.replace("= 1 field u8", "= 1 field u64");
    assert_ne!(text, stale, "fixture lockfile shape changed");
    fs::write(&path, stale).unwrap();

    let old_root = root.join("old");
    let new_root = root.join("new");
    for (label, old, new) in [
        ("version directories", old.as_path(), new.as_path()),
        ("WIT roots", old_root.as_path(), new_root.as_path()),
    ] {
        let output = diff(old, new, &[]);
        assert_eq!(output.status.code(), Some(1), "{label}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("changed kind or type") || stderr.contains("stale"),
            "{label}: {stderr}"
        );
    }

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_refuses_stale_draft_entries_in_direct_version_endpoints() {
    let root = workspace("stale-draft");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types { record task { id: u8, legacy: u8 } }
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types { record task { id: u8 } }
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    // Copy the old lockfile without reconciling: `legacy` is no longer a live member and was not
    // reserved, which is exactly the draft drift `witforge lock` would drop.
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();

    let output = diff(&old, &new, &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("stale"), "{stderr}");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_reports_guaranteed_down_conversion_failures() {
    let root = workspace("conversion-impact");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types {
    record task { id: u64, required: string, optional: option<string>, listed: list<u8> }
    record second { id: u64, also-required: u32 }
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
    record second { id: u64 }
}
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();
    lock_against(&root, "new/v0.2", "old/v0.1");

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let impacts = report["conversion_impacts"].as_array().unwrap();
    assert_eq!(impacts.len(), 2, "{report}");
    let task = impacts
        .iter()
        .find(|impact| impact["type_path"] == "types/task")
        .expect("task impact");
    assert_eq!(task["from_version"], "0.2.0");
    assert_eq!(task["to_version"], "0.1.0");
    assert_eq!(task["missing_member"], "required");
    assert_eq!(task["missing_path"], "types/task.required");
    let second = impacts
        .iter()
        .find(|impact| impact["type_path"] == "types/second")
        .expect("second impact");
    assert_eq!(second["missing_member"], "also-required");
    assert_eq!(second["missing_path"], "types/second.also-required");
    // Defaultable removals (`optional` and `listed`) are reconstructed, so they have no note.
    assert!(impacts.iter().all(|impact| {
        impact["missing_member"] != "optional" && impact["missing_member"] != "listed"
    }));
    // The notes are not changes: every removal is still one change and one breaking verdict.
    assert_eq!(report["summary"]["changes"], 4);
    assert_eq!(report["summary"]["breaking"], 4);
    assert_eq!(report["summary"]["wire_breaking"], 0);

    let human = diff(&old, &new, &[]);
    assert!(human.status.success(), "{human:?}");
    let text = String::from_utf8_lossy(&human.stdout);
    assert!(
        text.contains("note: new-to-old conversion for `types/task`"),
        "{text}"
    );
    assert!(text.contains("types/task.required"), "{text}");
    assert!(text.contains("0.2.0 -> 0.1.0"), "{text}");

    let diagnostics = diff(&old, &new, &["--format", "json"]);
    assert!(diagnostics.status.success(), "{diagnostics:?}");
    let envelope: serde_json::Value = serde_json::from_slice(&diagnostics.stdout).unwrap();
    let note = envelope["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| {
            diagnostic["code"] == "WF3003" && diagnostic["path"] == "types/task.required"
        })
        .expect("WF3003 diagnostic");
    assert_eq!(note["severity"], "note");

    let sarif = diff(&old, &new, &["--format", "sarif"]);
    assert!(sarif.status.success(), "{sarif:?}");
    let sarif: serde_json::Value = serde_json::from_slice(&sarif.stdout).unwrap();
    assert!(
        sarif["runs"][0]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["ruleId"] == "WF3003" && result["level"] == "note")
    );

    let checked = diff(&old, &new, &["--check"]);
    assert_eq!(checked.status.code(), Some(1), "{checked:?}");
    assert!(
        String::from_utf8_lossy(&checked.stderr).contains("4 breaking change(s)"),
        "{checked:?}"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_reports_a_rename_as_a_missing_old_member_only() {
    let root = workspace("conversion-impact-rename");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types {
    record item { old-name: string, optional: option<string> }
    variant event { kept, removed }
}
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types {
    record item { new-name: string }
    variant event { kept }
}
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();
    lock_against(&root, "new/v0.2", "old/v0.1");

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let impacts = report["conversion_impacts"].as_array().unwrap();
    assert_eq!(impacts.len(), 1, "{report}");
    assert_eq!(impacts[0]["type_path"], "types/item");
    assert_eq!(impacts[0]["missing_member"], "old-name");
    assert_eq!(impacts[0]["missing_path"], "types/item.old-name");
    // The defaultable `optional` removal and the value-dependent variant case removal are not
    // guaranteed failures.
    assert!(
        report["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|change| change["path"] == "types/event.removed")
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_does_not_note_named_defaultable_removals() {
    let root = workspace("defaultable-named");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types {
    type maybe = option<string>;
    type items = list<u8>;
    record task { id: u64, alias-removed: maybe, list-removed: items }
}
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types { record task { id: u64 } }
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();
    lock_against(&root, "new/v0.2", "old/v0.1");

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["conversion_impacts"],
        serde_json::json!([]),
        "{report}"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_notes_the_first_missing_required_member() {
    let root = workspace("conversion-impact-first");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types { record task { keep: u64, first: string, second: u32 } }
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types { record task { keep: u64 } }
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();
    lock_against(&root, "new/v0.2", "old/v0.1");

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let impacts = report["conversion_impacts"].as_array().unwrap();
    assert_eq!(impacts.len(), 1, "{report}");
    assert_eq!(impacts[0]["missing_member"], "first");
    assert_eq!(impacts[0]["missing_path"], "types/task.first");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_notes_contract_level_failures_for_non_adjacent_versions() {
    let root = workspace("conversion-impact-non-adjacent");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types { record task { id: u64, required: string } }
",
    );
    contract(
        &root,
        "new/v0.3",
        r"
package example:tasks@0.3.0;
interface types { record task { id: u64 } }
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.3");
    lock(&root, "old/v0.1");
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();
    lock_against(&root, "new/v0.3", "old/v0.1");

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let impacts = report["conversion_impacts"].as_array().unwrap();
    assert_eq!(impacts.len(), 1, "{report}");
    assert_eq!(impacts[0]["from_version"], "0.3.0");
    assert_eq!(impacts[0]["to_version"], "0.1.0");
    assert_eq!(impacts[0]["missing_path"], "types/task.required");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_accepts_a_non_canonical_but_current_lockfile() {
    let root = workspace("non-canonical");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types { record task { id: u64 } }
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types { record task { id: u64 } }
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    lock(&root, "new/v0.2");

    // `check` is the canonical-form gate; `diff` accepts a current lockfile that a hand edit
    // made non-canonical.
    let path = old.join("wire.lock");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("# hand-written comment\n");
    fs::write(&path, text).unwrap();
    let checked = run(&["check", &old.display().to_string()]);
    assert_eq!(checked.status.code(), Some(1), "{checked:?}");
    assert!(
        String::from_utf8_lossy(&checked.stderr).contains("not in the form"),
        "{checked:?}"
    );

    let output = diff(&old, &new, &[]);
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("result: COMPATIBLE"),
        "{output:?}"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_refuses_a_reversed_version_order() {
    let root = workspace("reversed");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types { record task { id: u64 } }
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types { record task { id: u64, note: option<string> } }
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();
    lock_against(&root, "new/v0.2", "old/v0.1");

    let old_root = root.join("old");
    let new_root = root.join("new");
    for (label, old, new) in [
        ("version directories", new.as_path(), old.as_path()),
        ("WIT roots", new_root.as_path(), old_root.as_path()),
    ] {
        let output = diff(old, new, &[]);
        assert_eq!(output.status.code(), Some(2), "{label}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("cannot diff version 0.2.0 against older version 0.1.0"),
            "{label}: {stderr}"
        );
        assert!(stderr.contains("older contract first"), "{label}: {stderr}");
        assert!(stderr.contains("run `witforge diff "), "{label}: {stderr}");
    }

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_reports_a_missing_lockfile_on_a_direct_endpoint() {
    let root = workspace("missing-lock");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types { record task { id: u64 } }
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types { record task { id: u64 } }
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "new/v0.2");

    let output = diff(&old, &new, &[]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("has no `wire.lock`"), "{stderr}");
    assert!(stderr.contains("--first"), "{stderr}");
    assert!(stderr.contains("--against"), "{stderr}");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_allows_equal_versions() {
    let root = workspace("equal");
    contract(
        &root,
        "left/v0.1",
        r"
package example:tasks@0.1.0;
interface types { record task { id: u64 } }
",
    );
    contract(
        &root,
        "right/v0.1",
        r"
package example:tasks@0.1.0;
interface types { record task { id: u64 } }
",
    );
    let left = root.join("left/v0.1");
    let right = root.join("right/v0.1");
    lock(&root, "left/v0.1");
    lock(&root, "right/v0.1");

    let output = diff(&left, &right, &[]);
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("no contract changes"),
        "{output:?}"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_reports_an_alias_target_change_as_a_conversion_blocker() {
    let root = workspace("alias-target-blocker");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types { type id = u32; record task { id: id } }
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types { type id = u64; record task { id: id } }
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    let lock_text = fs::read_to_string(old.join("wire.lock")).unwrap();
    fs::write(
        new.join("wire.lock"),
        lock_text.replace(
            "example:tasks/types/task.id = 1 field u32",
            "example:tasks/types/task.id = 1 field u64",
        ),
    )
    .unwrap();

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let blockers = report["conversion_blockers"].as_array().unwrap();
    assert!(
        blockers
            .iter()
            .any(|blocker| { blocker["kind"] == "retyped" && blocker["path"] == "types/task.id" }),
        "{report}"
    );
    assert!(
        report["changes"].as_array().unwrap().iter().any(|change| {
            change["kind"] == "retyped"
                && change["path"] == "types/task.id"
                && change["wire_breaking"] == true
        }),
        "{report}"
    );

    let check = diff(&old, &new, &["--check"]);
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_reports_required_additions_that_block_conversion_generation() {
    let root = workspace("required-addition");
    contract(
        &root,
        "old/v0.1",
        r"
package example:tasks@0.1.0;
interface types { record task { id: u64 } }
",
    );
    contract(
        &root,
        "new/v0.2",
        r"
package example:tasks@0.2.0;
interface types { record task { id: u64, required: string, note: option<string> } }
",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();
    lock_against(&root, "new/v0.2", "old/v0.1");

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["conversion_impacts"],
        serde_json::json!([]),
        "{report}"
    );
    let blockers = report["conversion_blockers"].as_array().unwrap();
    assert_eq!(blockers.len(), 1, "{report}");
    assert_eq!(blockers[0]["kind"], "required-addition");
    assert_eq!(blockers[0]["path"], "types/task.required");
    assert!(report["changes"].as_array().unwrap().iter().any(|change| {
        change["kind"] == "added"
            && change["path"] == "types/task.required"
            && change["defaulted"] == false
    }));
    assert!(report["changes"].as_array().unwrap().iter().any(|change| {
        change["kind"] == "added"
            && change["path"] == "types/task.note"
            && change["defaulted"] == true
    }));
    assert_eq!(report["summary"]["changes"], 2);
    assert_eq!(report["summary"]["breaking"], 2);

    let human = diff(&old, &new, &[]);
    assert!(human.status.success(), "{human:?}");
    let text = String::from_utf8_lossy(&human.stdout);
    assert!(text.contains("conversion generation refuses"), "{text}");
    assert!(text.contains("types/task.required"), "{text}");

    let diagnostics = diff(&old, &new, &["--format", "json"]);
    assert!(diagnostics.status.success(), "{diagnostics:?}");
    let envelope: serde_json::Value = serde_json::from_slice(&diagnostics.stdout).unwrap();
    let blocker = envelope["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| {
            diagnostic["code"] == "WF3004" && diagnostic["path"] == "types/task.required"
        })
        .expect("WF3004 diagnostic");
    assert_eq!(blocker["severity"], "note");

    let sarif = diff(&old, &new, &["--format", "sarif"]);
    assert!(sarif.status.success(), "{sarif:?}");
    let sarif: serde_json::Value = serde_json::from_slice(&sarif.stdout).unwrap();
    assert!(
        sarif["runs"][0]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["ruleId"] == "WF3004" && result["level"] == "note")
    );

    let checked = diff(&old, &new, &["--check"]);
    assert_eq!(checked.status.code(), Some(1), "{checked:?}");
    assert!(
        String::from_utf8_lossy(&checked.stderr).contains("2 breaking change(s)"),
        "{checked:?}"
    );

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

fn extract_backticked_witforge_command(text: &str) -> String {
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('`') else { break };
        let inside = &rest[..end];
        rest = &rest[end + 1..];
        if inside.starts_with("witforge ") && inside != "witforge lock" {
            return inside.to_string();
        }
    }
    panic!("no runnable `witforge ...` command found in:\n{text}");
}

fn split_posix_args(command: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut started = false;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_single {
            if ch == '\'' {
                in_single = false;
            } else {
                cur.push(ch);
            }
        } else {
            match ch {
                '\'' => {
                    in_single = true;
                    started = true;
                }
                '\\' if chars.peek() == Some(&'\'') => {
                    cur.push(chars.next().unwrap());
                    started = true;
                }
                c if c.is_ascii_whitespace() => {
                    if started {
                        args.push(std::mem::take(&mut cur));
                        started = false;
                    }
                }
                c => {
                    cur.push(c);
                    started = true;
                }
            }
        }
    }
    assert!(!in_single, "unterminated quote in `{command}`");
    if started {
        args.push(cur);
    }
    args
}

#[test]
fn diff_remedy_messages_quote_spaces_and_run_as_printed() {
    let root = std::env::temp_dir().join(format!("witforge diff remedies {}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let old = root.join("old dir/v0.1");
    let new = root.join("new dir/v0.2");
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&new).unwrap();
    fs::write(
        old.join("contract.wit"),
        "package example:tasks@0.1.0;\ninterface types { record task { id: u64 } }\n",
    )
    .unwrap();
    fs::write(
        new.join("contract.wit"),
        "package example:tasks@0.2.0;\ninterface types { record task { id: u64, note: option<string> } }\n",
    )
    .unwrap();

    // 1. Missing lockfile remedy quotes paths with spaces and runs verbatim.
    let locked_new = run(&["lock", &new.display().to_string(), "--first"]);
    assert!(locked_new.status.success(), "{locked_new:?}");
    let missing = diff(&old, &new, &[]);
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(!stderr.contains("--against <"), "{stderr}");
    let cmd = extract_backticked_witforge_command(&stderr);
    let tokens = split_posix_args(&cmd);
    let ran = Command::new(env!("CARGO_BIN_EXE_witforge"))
        .args(&tokens[1..])
        .output()
        .unwrap();
    assert!(ran.status.success(), "{cmd}: {ran:?}");

    // 2. Reversed-endpoints message names versions in argument order, quotes spaced paths,
    // appears in JSON/SARIF too, and its printed swapped command runs verbatim.
    let reversed = diff(&new, &old, &[]);
    assert_eq!(reversed.status.code(), Some(2), "{reversed:?}");
    let stderr = String::from_utf8_lossy(&reversed.stderr);
    assert!(
        stderr.contains("cannot diff version 0.2.0 against older version 0.1.0"),
        "{stderr}"
    );
    let cmd = extract_backticked_witforge_command(&stderr);
    let tokens = split_posix_args(&cmd);
    let ran = Command::new(env!("CARGO_BIN_EXE_witforge"))
        .args(&tokens[1..])
        .output()
        .unwrap();
    assert!(ran.status.success(), "{cmd}: {ran:?}");

    let reversed_json = diff(&new, &old, &["--format", "json"]);
    assert_eq!(reversed_json.status.code(), Some(2), "{reversed_json:?}");
    let stdout = String::from_utf8_lossy(&reversed_json.stdout);
    assert!(
        stdout.contains("cannot diff version 0.2.0 against older version 0.1.0"),
        "{stdout}"
    );

    // 3. Stale lockfile in `diff` prints a runnable `witforge lock <dir>` command (never bare
    // `witforge lock`) that fixes the lockfile when run.
    fs::write(
        new.join("contract.wit"),
        "package example:tasks@0.2.0;\ninterface types { record task { id: u64, note: option<string>, extra: option<u32> } }\n",
    )
    .unwrap();
    let stale = diff(&old, &new, &[]);
    assert_eq!(stale.status.code(), Some(1), "{stale:?}");
    let stderr = String::from_utf8_lossy(&stale.stderr);
    assert!(!stderr.contains("run `witforge lock`"), "{stderr}");
    let cmd = extract_backticked_witforge_command(&stderr);
    let tokens = split_posix_args(&cmd);
    let ran = Command::new(env!("CARGO_BIN_EXE_witforge"))
        .args(&tokens[1..])
        .output()
        .unwrap();
    assert!(ran.status.success(), "{cmd}: {ran:?}");
    assert!(diff(&old, &new, &[]).status.success());

    fs::remove_dir_all(&root).unwrap();
}
