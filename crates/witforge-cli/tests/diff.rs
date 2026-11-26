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
            && change["wire_breaking"] == true
            && change["decode_breaking"] == true
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
    // The new lock was started fresh, so the two removals lost their old entries AND the two
    // additions reused those exact wire numbers (3 and 2), alongside the explicit field retype.
    assert_eq!(report["summary"]["wire_breaking"], 5);

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
fn diff_renders_stability_changes_as_nonbreaking_metadata() {
    let root = workspace("stability");
    contract(
        &root,
        "old/v0.1",
        "package example:tasks@0.1.0;\n\
         interface types {\n\
             @since(version = 0.1.0)\n\
             record old { id: u64 }\n\
         }\n",
    );
    contract(
        &root,
        "new/v0.2",
        "package example:tasks@0.2.0;\n\
         interface types {\n\
             @since(version = 0.1.0)\n\
             @deprecated(version = 0.2.0)\n\
             record old { id: u64 }\n\
         }\n",
    );
    let old = root.join("old/v0.1");
    let new = root.join("new/v0.2");
    lock(&root, "old/v0.1");
    lock(&root, "new/v0.2");

    let output = diff(&old, &new, &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let change = &report["changes"][0];
    assert_eq!(change["kind"], "stability-changed");
    assert_eq!(change["element"], "type");
    assert_eq!(change["path"], "types/old");
    assert_eq!(change["from"], "stable(since=0.1.0)");
    assert_eq!(change["to"], "deprecated(since=0.1.0,version=0.2.0)");
    assert_eq!(report["summary"]["breaking"], 0);

    let checked = diff(&old, &new, &["--check"]);
    assert!(checked.status.success(), "{checked:?}");
    let human = String::from_utf8(checked.stdout).unwrap();
    assert!(human.contains("stability-changed"), "{human}");
    assert!(human.contains("result: COMPATIBLE"), "{human}");
    fs::remove_dir_all(root).unwrap();
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

#[allow(clippy::too_many_lines)]
#[test]
fn diff_check_gates_selected_views_and_rejects_empty_or_unknown_views() {
    let root = workspace("per-view-check");
    // Case 1: Wire-compatible, source/API-breaking addition (`option<string>` record field).
    contract(
        &root,
        "add/v0.1",
        "package example:views@0.1.0;\ninterface types { record item { id: u64 } }\n",
    );
    contract(
        &root,
        "add/v0.2",
        "package example:views@0.2.0;\ninterface types { record item { id: u64, note: option<string> } }\n",
    );
    let add_old = root.join("add/v0.1");
    let add_new = root.join("add/v0.2");
    lock(&root, "add/v0.1");
    fs::copy(add_old.join("wire.lock"), add_new.join("wire.lock")).unwrap();
    lock_against(&root, "add/v0.2", "add/v0.1");

    // `--check=wire` passes (exit 0) and SARIF emits warning (never error) on exit 0.
    let wire_ok = diff(&add_old, &add_new, &["--check=wire"]);
    assert!(wire_ok.status.success(), "{wire_ok:?}");
    let wire_sarif = diff(&add_old, &add_new, &["--check=wire", "--format", "sarif"]);
    assert!(wire_sarif.status.success(), "{wire_sarif:?}");
    let sarif_val: serde_json::Value = serde_json::from_slice(&wire_sarif.stdout).unwrap();
    let results = sarif_val["runs"][0]["results"].as_array().unwrap();
    assert!(
        results
            .iter()
            .any(|r| r["ruleId"] == "WF3001" && r["level"] == "warning"),
        "{results:?}"
    );
    assert!(
        results.iter().all(|r| r["level"] != "error"),
        "passing --check=wire must not emit error-level SARIF results: {results:?}"
    );

    // `--check` before positional args still parses OLD/NEW cleanly without swallowing OLD.
    let before_pos = run(&[
        "diff",
        "--check=wire",
        &add_old.display().to_string(),
        &add_new.display().to_string(),
    ]);
    assert!(before_pos.status.success(), "{before_pos:?}");
    let bare_before_pos = run(&[
        "diff",
        "--check",
        &add_old.display().to_string(),
        &add_new.display().to_string(),
    ]);
    assert_eq!(
        bare_before_pos.status.code(),
        Some(1),
        "{bare_before_pos:?}"
    );

    // `--check=source`, `--check=api`, `--check=wire,api`, and bare `--check` all fail (exit 1).
    for flag in [
        "--check",
        "--check=source",
        "--check=api",
        "--check=wire,api",
    ] {
        let out = diff(&add_old, &add_new, &[flag]);
        assert_eq!(out.status.code(), Some(1), "{flag}: {out:?}");
    }
    let api_sarif = diff(&add_old, &add_new, &["--check=api", "--format", "sarif"]);
    assert_eq!(api_sarif.status.code(), Some(1), "{api_sarif:?}");
    let api_val: serde_json::Value = serde_json::from_slice(&api_sarif.stdout).unwrap();
    assert!(
        api_val["runs"][0]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["ruleId"] == "WF3001" && r["level"] == "error"),
        "{api_val}"
    );

    // Case 2: Wire-only breaking change (renumbering fields, source & API unchanged).
    contract(
        &root,
        "renum/v0.1",
        "package example:renum@0.1.0;\ninterface types { record r { a: u8, b: u8 } }\n",
    );
    contract(
        &root,
        "renum/v0.2",
        "package example:renum@0.2.0;\ninterface types { record r { a: u8, b: u8 } }\n",
    );
    let r_old = root.join("renum/v0.1");
    let r_new = root.join("renum/v0.2");
    lock(&root, "renum/v0.1");
    fs::copy(r_old.join("wire.lock"), r_new.join("wire.lock")).unwrap();
    let swapped = fs::read_to_string(r_new.join("wire.lock"))
        .unwrap()
        .replace(
            "example:renum/types/r.a = 1 field u8",
            "example:renum/types/r.a = 2 field u8",
        )
        .replace(
            "example:renum/types/r.b = 2 field u8",
            "example:renum/types/r.b = 1 field u8",
        );
    fs::write(r_new.join("wire.lock"), swapped).unwrap();

    let renum_wire = diff(&r_old, &r_new, &["--check=wire"]);
    assert_eq!(renum_wire.status.code(), Some(1), "{renum_wire:?}");
    assert!(
        String::from_utf8_lossy(&renum_wire.stderr).contains("gated view(s) (wire)"),
        "{renum_wire:?}"
    );
    let renum_source_api = diff(&r_old, &r_new, &["--check=source,api"]);
    assert!(renum_source_api.status.success(), "{renum_source_api:?}");

    // Case 3: Empty set and unknown views are usage errors (exit 2, WF9001).
    for bad in [
        "--check=",
        "--check=bogus",
        "--check=wire,",
        "--check=,wire",
    ] {
        let out = diff(&add_old, &add_new, &[bad, "--format", "json"]);
        assert_eq!(out.status.code(), Some(2), "{bad}: {out:?}");
        let val: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(val["diagnostics"][0]["code"], "WF9001", "{bad}: {val}");
    }

    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn diff_and_check_across_major_report_omissions_and_support_retirement() {
    let root = workspace("major-cli");
    let wit = root.join("wit");
    for (dir, ver, dep_body, dep_ver, body) in [
        (
            "v0.1",
            "0.1.0",
            "package example:dep@0.1.0;\ninterface common { record tag { k: u32 } }\n",
            "0.1.0",
            "interface types {\n\
               use example:dep/common@0.1.0.{tag};\n\
               record keep { id: u32 }\n\
               record order { id: u32, t: tag }\n\
             }\n",
        ),
        (
            "v0.2",
            "0.2.0",
            "package example:dep@0.1.0;\ninterface common { record tag { k: u32 } }\n",
            "0.1.0",
            "interface types {\n\
               use example:dep/common@0.1.0.{tag};\n\
               record keep { id: u32, note: option<string> }\n\
               record order { id: u32, t: tag }\n\
             }\n",
        ),
        (
            "v1.0",
            "1.0.0",
            "package example:dep@1.0.0;\ninterface common { record tag { k: string } }\n",
            "1.0.0",
            "interface types {\n\
               use example:dep/common@1.0.0.{tag};\n\
               record keep { id: u32, note: option<string>, total: u64 }\n\
               record order { id: u32, t: tag }\n\
             }\n",
        ),
    ] {
        let _ = dep_ver;
        let dep_dir = wit.join(dir).join("deps/dep");
        fs::create_dir_all(&dep_dir).unwrap();
        fs::write(dep_dir.join("common.wit"), dep_body).unwrap();
        contract(
            &root,
            &format!("wit/{dir}"),
            &format!("package example:shop@{ver};\n{body}"),
        );
    }
    lock(&root, "wit/v0.1");
    fs::copy(wit.join("v0.1/wire.lock"), wit.join("v0.2/wire.lock")).unwrap();
    let lock_v02 = witforge()
        .args(["lock"])
        .arg(wit.join("v0.2"))
        .args(["--against"])
        .arg(wit.join("v0.1/wire.lock"))
        .output()
        .unwrap();
    assert!(lock_v02.status.success(), "{lock_v02:?}");
    lock(&root, "wit/v1.0");

    // Mark v0.1 retired in `released`; whole-root `witforge check` accepts retired oldest prefix
    // AND upgraded dependencies in v1.0!
    fs::write(wit.join("released"), "v0.1 retired\nv0.2\n").unwrap();
    let check_ok = witforge().args(["check"]).arg(&wit).output().unwrap();
    assert!(check_ok.status.success(), "{check_ok:?}");

    // `witforge rust` generates convert.rs with top omission comment block.
    let out_rust = root.join("rust");
    let rust_ok = witforge()
        .args(["rust"])
        .arg(&wit)
        .args(["--out"])
        .arg(&out_rust)
        .output()
        .unwrap();
    assert!(rust_ok.status.success(), "{rust_ok:?}");
    assert!(!out_rust.join("v0_1.rs").exists(), "retired v0.1 omitted");
    let convert_rs = fs::read_to_string(out_rust.join("convert.rs")).unwrap();
    let convert_lines: Vec<String> = convert_rs
        .lines()
        .filter_map(|line| line.strip_prefix("// - ").map(str::to_string))
        .collect();

    // `witforge diff v0.2 v1.0 --json` reports the exact same omissions and empty blockers.
    let json_out = diff(&wit.join("v0.2"), &wit.join("v1.0"), &["--json"]);
    assert!(json_out.status.success(), "{json_out:?}");
    let json_val: serde_json::Value = serde_json::from_slice(&json_out.stdout).unwrap();
    assert_eq!(json_val["conversion_blockers"], serde_json::json!([]));
    let diff_lines: Vec<String> = json_val["conversion_omissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            format!(
                "{} -> {} {}: {}",
                o["from_version"].as_str().unwrap(),
                o["to_version"].as_str().unwrap(),
                o["type_path"].as_str().unwrap(),
                o["reason"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(diff_lines, convert_lines, "{json_val}");
    // keep omitted only 0.2.0 -> 1.0.0; order and the local `use` alias `tag` omitted both
    // directions (upgraded dep).
    assert_eq!(diff_lines.len(), 5, "{diff_lines:?}");

    // Human output and SARIF emit omission notes (WF3004); `--check` still exits 1 on breaking.
    let human = diff(&wit.join("v0.2"), &wit.join("v1.0"), &[]);
    assert!(human.status.success(), "{human:?}");
    let human_text = String::from_utf8_lossy(&human.stdout);
    assert!(
        human_text.contains("conversion 0.2.0 -> 1.0.0 for `types/keep` is omitted"),
        "{human_text}"
    );
    assert!(
        human_text.contains("conversion 0.2.0 -> 1.0.0 for `types/order` is omitted"),
        "{human_text}"
    );

    let sarif = diff(&wit.join("v0.2"), &wit.join("v1.0"), &["--format", "sarif"]);
    assert!(sarif.status.success(), "{sarif:?}");
    let sarif_val: serde_json::Value = serde_json::from_slice(&sarif.stdout).unwrap();
    let wf3004_count = sarif_val["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["ruleId"] == "WF3004" && r["level"] == "note")
        .count();
    assert_eq!(wf3004_count, 5, "{sarif_val}");

    let checked = diff(&wit.join("v0.2"), &wit.join("v1.0"), &["--check"]);
    assert_eq!(checked.status.code(), Some(1), "{checked:?}");

    // Non-oldest retirement fails whole-root `witforge check`.
    fs::write(wit.join("released"), "v0.1\nv0.2 retired\n").unwrap();
    let check_bad = witforge().args(["check"]).arg(&wit).output().unwrap();
    assert_eq!(check_bad.status.code(), Some(1), "{check_bad:?}");
    assert!(
        String::from_utf8_lossy(&check_bad.stderr).contains("cannot retire `v0.2`"),
        "{check_bad:?}"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn diff_decode_view_gates_only_when_selected_and_preserves_json_fields() {
    let root = workspace("decode-cli");

    // Case 1: Adding a new interface with a function is compatible under default `--check`
    // (`wire,source,api`), yet decode-breaking (`old_reads_new = false`) when `--check=decode`
    // is explicitly selected!
    contract(
        &root,
        "iface/v0.1",
        "package example:svc@0.1.0;\ninterface core { ping: func(); }\n",
    );
    contract(
        &root,
        "iface/v0.2",
        "package example:svc@0.2.0;\n\
         interface core { ping: func(); }\n\
         interface extra { greet: func(); }\n",
    );
    let i_old = root.join("iface/v0.1");
    let i_new = root.join("iface/v0.2");
    lock(&root, "iface/v0.1");
    fs::copy(i_old.join("wire.lock"), i_new.join("wire.lock")).unwrap();
    let lock_i = witforge()
        .args(["lock"])
        .arg(&i_new)
        .args(["--against"])
        .arg(i_old.join("wire.lock"))
        .output()
        .unwrap();
    assert!(lock_i.status.success(), "{lock_i:?}");

    // Default `--check` does NOT gate on `decode`: exits 0!
    let default_check = diff(&i_old, &i_new, &["--check"]);
    assert!(
        default_check.status.success(),
        "default --check must not gate on decode: {default_check:?}"
    );

    // Explicit `--check=decode` gates on decode: exits 1, SARIF level = error!
    let decode_check = diff(&i_old, &i_new, &["--check=decode"]);
    assert_eq!(decode_check.status.code(), Some(1), "{decode_check:?}");
    assert!(
        String::from_utf8_lossy(&decode_check.stderr).contains("gated view(s) (decode)"),
        "{decode_check:?}"
    );
    let decode_sarif = diff(&i_old, &i_new, &["--check=decode", "--format", "sarif"]);
    assert_eq!(decode_sarif.status.code(), Some(1), "{decode_sarif:?}");
    let sarif_val: serde_json::Value = serde_json::from_slice(&decode_sarif.stdout).unwrap();
    assert!(
        sarif_val["runs"][0]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["ruleId"] == "WF3001" && r["level"] == "error"),
        "{sarif_val}"
    );

    // Case 2: Wire-compatible minor additions:
    // - added `option<string>` field: wire:compatible, decode:compatible
    // - added required `string` field: wire:compatible, decode:BREAKING(new<-old)
    // - added enum case: wire:compatible, decode:BREAKING(old<-new)
    contract(
        &root,
        "minor/v0.1",
        "package example:min@0.1.0;\n\
         interface types {\n\
           record r { id: u32 }\n\
           enum st { a, b }\n\
         }\n",
    );
    contract(
        &root,
        "minor/v0.2",
        "package example:min@0.2.0;\n\
         interface types {\n\
           record r { id: u32, opt: option<string>, req: string }\n\
           enum st { a, b, c }\n\
         }\n",
    );
    let m_old = root.join("minor/v0.1");
    let m_new = root.join("minor/v0.2");
    lock(&root, "minor/v0.1");
    fs::copy(m_old.join("wire.lock"), m_new.join("wire.lock")).unwrap();
    let lock_m = witforge()
        .args(["lock"])
        .arg(&m_new)
        .args(["--against"])
        .arg(m_old.join("wire.lock"))
        .output()
        .unwrap();
    assert!(lock_m.status.success(), "{lock_m:?}");

    // `--check=wire` passes (all wire-compatible), while `--check=wire,decode` fails!
    assert!(diff(&m_old, &m_new, &["--check=wire"]).status.success());
    let wire_decode = diff(&m_old, &m_new, &["--check=wire,decode"]);
    assert_eq!(wire_decode.status.code(), Some(1), "{wire_decode:?}");
    assert!(
        String::from_utf8_lossy(&wire_decode.stderr)
            .contains("2 breaking change(s) in gated view(s) (wire, decode)"),
        "{wire_decode:?}"
    );

    // Human output shows directional decode verdicts.
    let human = diff(&m_old, &m_new, &[]);
    assert!(human.status.success(), "{human:?}");
    let text = String::from_utf8_lossy(&human.stdout);
    assert!(text.contains("decode:compatible"), "{text}");
    assert!(text.contains("decode:BREAKING(new<-old)"), "{text}");
    assert!(text.contains("decode:BREAKING(old<-new)"), "{text}");
    assert!(
        text.contains("decode: 2 (new<-old: 1, old<-new: 1)"),
        "{text}"
    );

    // JSON output retains every existing field and adds decode fields.
    let json_out = diff(&m_old, &m_new, &["--json"]);
    assert!(json_out.status.success(), "{json_out:?}");
    let val: serde_json::Value = serde_json::from_slice(&json_out.stdout).unwrap();
    assert_eq!(val["summary"]["wire_breaking"], 0);
    assert_eq!(val["summary"]["decode_breaking"], 2);
    assert_eq!(val["summary"]["new_reads_old_breaking"], 1);
    assert_eq!(val["summary"]["old_reads_new_breaking"], 1);
    let changes = val["changes"].as_array().unwrap();
    let by_path = |p: &str| changes.iter().find(|c| c["path"] == p).unwrap();
    assert_eq!(by_path("types/r.opt")["decode_breaking"], false);
    assert_eq!(by_path("types/r.opt")["new_reads_old"], true);
    assert_eq!(by_path("types/r.opt")["old_reads_new"], true);
    assert_eq!(by_path("types/r.req")["decode_breaking"], true);
    assert_eq!(by_path("types/r.req")["new_reads_old"], false);
    assert_eq!(by_path("types/r.req")["old_reads_new"], true);
    assert_eq!(by_path("types/st.c")["decode_breaking"], true);
    assert_eq!(by_path("types/st.c")["new_reads_old"], true);
    assert_eq!(by_path("types/st.c")["old_reads_new"], false);

    // Case 3: Purely decode-compatible minor addition (`option<T>` only) passes `--check=wire,decode`!
    contract(
        &root,
        "opt/v0.1",
        "package example:opt@0.1.0;\ninterface types { record r { id: u32 } }\n",
    );
    contract(
        &root,
        "opt/v0.2",
        "package example:opt@0.2.0;\ninterface types { record r { id: u32, opt: option<u32> } }\n",
    );
    let o_old = root.join("opt/v0.1");
    let o_new = root.join("opt/v0.2");
    lock(&root, "opt/v0.1");
    fs::copy(o_old.join("wire.lock"), o_new.join("wire.lock")).unwrap();
    let lock_o = witforge()
        .args(["lock"])
        .arg(&o_new)
        .args(["--against"])
        .arg(o_old.join("wire.lock"))
        .output()
        .unwrap();
    assert!(lock_o.status.success(), "{lock_o:?}");
    assert!(
        diff(&o_old, &o_new, &["--check=wire,decode"])
            .status
            .success()
    );

    // Case 4: Reusing a removed live option field's wire number fails `--check=decode` AND
    // `--check=wire`, and deleting a same-major `reserved` entry fails `--check=wire`.
    contract(
        &root,
        "reuse-old/v0.1",
        "package example:reuse@0.1.0;\ninterface types { record r { old: option<u32> } }\n",
    );
    contract(
        &root,
        "reuse-new/v0.2",
        "package example:reuse@0.2.0;\ninterface types { record r { fresh: option<string> } }\n",
    );
    let r_old = root.join("reuse-old/v0.1");
    let r_new = root.join("reuse-new/v0.2");
    lock(&root, "reuse-old/v0.1");
    lock(&root, "reuse-new/v0.2");
    let reuse_decode = diff(&r_old, &r_new, &["--check=decode"]);
    assert_eq!(reuse_decode.status.code(), Some(1), "{reuse_decode:?}");
    let reuse_text = String::from_utf8_lossy(&reuse_decode.stdout);
    assert!(reuse_text.contains("decode:BREAKING(both)"), "{reuse_text}");
    let reuse_wire = diff(&r_old, &r_new, &["--check=wire"]);
    assert_eq!(reuse_wire.status.code(), Some(1), "{reuse_wire:?}");

    contract(
        &root,
        "resdrop-old/v0.1",
        "package example:resdrop@0.1.0;\ninterface types { record r { keep: u32 } }\n",
    );
    contract(
        &root,
        "resdrop-new/v0.2",
        "package example:resdrop@0.2.0;\ninterface types { record r { keep: u32 } }\n",
    );
    let d_old = root.join("resdrop-old/v0.1");
    let d_new = root.join("resdrop-new/v0.2");
    lock(&root, "resdrop-old/v0.1");
    lock(&root, "resdrop-new/v0.2");
    let mut with_reserved = fs::read_to_string(d_old.join("wire.lock")).unwrap();
    with_reserved.push_str("example:resdrop/types/r.old = 2 field u32 reserved\n");
    fs::write(d_old.join("wire.lock"), with_reserved).unwrap();
    let res_check = diff(&d_old, &d_new, &["--check=wire", "--json"]);
    assert_eq!(res_check.status.code(), Some(1), "{res_check:?}");
    let res_json: serde_json::Value = serde_json::from_slice(&res_check.stdout).unwrap();
    assert_eq!(res_json["changes"][0]["element"], "reserved entry");
    assert_eq!(res_json["changes"][0]["kind"], "removed");
    assert_eq!(res_json["changes"][0]["wire_breaking"], true);

    // Case 5: Human `result:` line matches the selected `--check` axes (prints `result: BREAKING`
    // for decode-only breaking changes under `--check=decode`, and `result: COMPATIBLE` when
    // gated only on `--check=wire`).
    contract(
        &root,
        "gate-old/v0.1",
        "package example:gate@0.1.0;\ninterface a { record r { id: u32 } }\n",
    );
    contract(
        &root,
        "gate-new/v0.2",
        "package example:gate@0.2.0;\n\
         interface a { record r { id: u32 } }\n\
         interface b { ping: func(); }\n",
    );
    let g_old = root.join("gate-old/v0.1");
    let g_new = root.join("gate-new/v0.2");
    lock(&root, "gate-old/v0.1");
    fs::copy(g_old.join("wire.lock"), g_new.join("wire.lock")).unwrap();
    let lock_g = witforge()
        .args(["lock"])
        .arg(&g_new)
        .args(["--against"])
        .arg(g_old.join("wire.lock"))
        .output()
        .unwrap();
    assert!(lock_g.status.success(), "{lock_g:?}");

    let decode_gate = diff(&g_old, &g_new, &["--check=decode"]);
    assert_eq!(decode_gate.status.code(), Some(1), "{decode_gate:?}");
    let decode_out = String::from_utf8_lossy(&decode_gate.stdout);
    assert!(decode_out.contains("result: BREAKING"), "{decode_out}");

    let wire_gate = diff(&g_old, &g_new, &["--check=wire"]);
    assert!(wire_gate.status.success(), "{wire_gate:?}");
    let wire_out = String::from_utf8_lossy(&wire_gate.stdout);
    assert!(wire_out.contains("result: COMPATIBLE"), "{wire_out}");

    fs::remove_dir_all(&root).unwrap();
}

fn run_git_cmd(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

fn init_git_repo(repo: &Path) {
    run_git_cmd(repo, &["init", "-b", "main"]);
    run_git_cmd(repo, &["config", "user.name", "Witforge Test"]);
    run_git_cmd(repo, &["config", "user.email", "test@example.com"]);
    run_git_cmd(repo, &["config", "commit.gpgsign", "false"]);
}

#[allow(clippy::too_many_lines)]
#[test]
fn diff_git_base_compares_working_tree_respects_tmpdir_and_enforces_guards() {
    let root = workspace("git-base");
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_git_repo(&repo);

    // Commit 1: wit/v0.1 locked and committed.
    contract(
        &repo,
        "wit/v0.1",
        "package example:gitdiff@0.1.0;\n\
         interface types {\n\
           record item { id: u32 }\n\
         }\n",
    );
    lock(&repo, "wit/v0.1");
    run_git_cmd(&repo, &["add", "."]);
    run_git_cmd(&repo, &["commit", "-m", "initial v0.1"]);

    // Commit 2: add v0.2 with a defaultable optional field (wire+decode compatible, source/api breaking).
    contract(
        &repo,
        "wit/v0.2",
        "package example:gitdiff@0.2.0;\n\
         interface types {\n\
           record item { id: u32, note: option<string> }\n\
         }\n",
    );
    fs::copy(
        repo.join("wit/v0.1/wire.lock"),
        repo.join("wit/v0.2/wire.lock"),
    )
    .unwrap();
    let lock_v02 = witforge()
        .args(["lock"])
        .arg(repo.join("wit/v0.2"))
        .args(["--against"])
        .arg(repo.join("wit/v0.1/wire.lock"))
        .output()
        .unwrap();
    assert!(lock_v02.status.success(), "{lock_v02:?}");
    run_git_cmd(&repo, &["add", "."]);
    run_git_cmd(&repo, &["commit", "-m", "add v0.2"]);

    // 1. Compare WIT root against HEAD~1 with private TMPDIR: verifies TMPDIR respect, cleanup,
    // deterministic `<REV>:<path>` labels, `--json`, `--format sarif`, and `--check` views.
    let private_tmp = root.join("private-tmp");
    fs::create_dir_all(&private_tmp).unwrap();

    let json_out = witforge()
        .env("TMPDIR", &private_tmp)
        .env("TMP", &private_tmp)
        .env("TEMP", &private_tmp)
        .current_dir(&repo)
        .args(["diff", "--git-base", "HEAD~1", "wit", "--json"])
        .output()
        .unwrap();
    assert!(json_out.status.success(), "{json_out:?}");
    let val: serde_json::Value = serde_json::from_slice(&json_out.stdout).unwrap();
    assert_eq!(val["old"]["path"], "HEAD~1:wit/v0.1", "{val}");
    assert_eq!(val["old"]["version"], "0.1.0");
    assert_eq!(
        Path::new(val["new"]["path"].as_str().unwrap()),
        Path::new("wit").join("v0.2"),
        "{val}"
    );
    assert_eq!(val["new"]["version"], "0.2.0");
    assert_eq!(val["summary"]["changes"], 1);
    assert_eq!(val["summary"]["wire_breaking"], 0);
    assert_eq!(val["summary"]["decode_breaking"], 0);
    assert_eq!(val["changes"][0]["path"], "types/item.note");
    assert_eq!(
        fs::read_dir(&private_tmp).unwrap().count(),
        0,
        "temp dir must be cleaned up after success"
    );

    // `--check=wire,decode` passes, default `--check` fails (source/api breaking).
    let wd_ok = witforge()
        .env("TMPDIR", &private_tmp)
        .current_dir(&repo)
        .args(["diff", "--git-base", "HEAD~1", "wit", "--check=wire,decode"])
        .output()
        .unwrap();
    assert!(wd_ok.status.success(), "{wd_ok:?}");
    let human_stdout = String::from_utf8_lossy(&wd_ok.stdout);
    assert!(
        human_stdout.contains("old: HEAD~1:wit/v0.1 (example:gitdiff 0.1.0)"),
        "{human_stdout}"
    );
    assert!(
        !human_stdout.contains("witforge-git-base"),
        "temp dir path must never leak into output: {human_stdout}"
    );

    let default_fail = witforge()
        .env("TMPDIR", &private_tmp)
        .current_dir(&repo)
        .args(["diff", "--git-base", "HEAD~1", "wit", "--check"])
        .output()
        .unwrap();
    assert_eq!(default_fail.status.code(), Some(1), "{default_fail:?}");

    // 2. Single version directory diff with an uncommitted draft edit (removed field -> location
    // points to `HEAD:wit/v0.2/contract.wit` with zero temp-path leak in SARIF).
    contract(
        &repo,
        "wit/v0.2",
        "package example:gitdiff@0.2.0;\n\
         interface types {\n\
           record item { id: u32, extra: string }\n\
         }\n",
    );
    let lock_edit = witforge()
        .args(["lock"])
        .arg(repo.join("wit/v0.2"))
        .args(["--against"])
        .arg(repo.join("wit/v0.1/wire.lock"))
        .output()
        .unwrap();
    assert!(lock_edit.status.success(), "{lock_edit:?}");

    let sarif_out = witforge()
        .env("TMPDIR", &private_tmp)
        .current_dir(&repo)
        .args([
            "diff",
            "--git-base",
            "HEAD",
            "wit/v0.2",
            "--check=decode",
            "--format",
            "sarif",
        ])
        .output()
        .unwrap();
    assert_eq!(sarif_out.status.code(), Some(1), "{sarif_out:?}");
    let sarif_text = String::from_utf8_lossy(&sarif_out.stdout);
    assert!(
        !sarif_text.contains("witforge-git-base"),
        "temp path must not leak into SARIF: {sarif_text}"
    );
    assert!(
        sarif_text.contains("HEAD%3Awit/v0.2/contract.wit"),
        "removed field SARIF URI should percent-encode HEAD:wit/v0.2/contract.wit: {sarif_text}"
    );
    let diag_json_out = witforge()
        .env("TMPDIR", &private_tmp)
        .current_dir(&repo)
        .args([
            "diff",
            "--git-base",
            "HEAD",
            "wit/v0.2",
            "--check=decode",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(diag_json_out.status.code(), Some(1), "{diag_json_out:?}");
    let diag_text = String::from_utf8_lossy(&diag_json_out.stdout);
    assert!(
        diag_text.contains("HEAD:wit/v0.2/contract.wit"),
        "removed field JSON diagnostic file should point to HEAD:wit/v0.2/contract.wit: {diag_text}"
    );
    assert_eq!(
        fs::read_dir(&private_tmp).unwrap().count(),
        0,
        "temp dir must be cleaned up after failure"
    );

    // Restore clean working tree for shallow-clone & symlink tests.
    run_git_cmd(&repo, &["checkout", "--", "wit/v0.2"]);

    // 3. Shallow-clone missing revision detection (`WF9004`, exit 2, mentions shallow + fetch-depth).
    let shallow_dir = root.join("shallow-clone");
    let canon_repo = fs::canonicalize(&repo).unwrap();
    let repo_uri_path = canon_repo
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/");
    let repo_url = format!("file:///{}", repo_uri_path.trim_start_matches('/'));
    run_git_cmd(
        &root,
        &[
            "clone",
            "--depth=1",
            &repo_url,
            shallow_dir.to_str().unwrap(),
        ],
    );
    let shallow_err = witforge()
        .env("TMPDIR", &private_tmp)
        .current_dir(&shallow_dir)
        .args(["diff", "--git-base", "HEAD~1", "wit", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(shallow_err.status.code(), Some(2), "{shallow_err:?}");
    let shallow_json: serde_json::Value = serde_json::from_slice(&shallow_err.stdout).unwrap();
    assert_eq!(shallow_json["diagnostics"][0]["code"], "WF9004");
    let shallow_msg = shallow_json["diagnostics"][0]["message"].as_str().unwrap();
    assert!(
        shallow_msg.contains("shallow clone") && shallow_msg.contains("fetch-depth: 0"),
        "{shallow_msg}"
    );

    // 4. Usage & operational guards:
    // - two positional paths with `--git-base` -> WF9001 (exit 2)
    let two_paths = witforge()
        .current_dir(&repo)
        .args([
            "diff",
            "--git-base",
            "HEAD~1",
            "wit/v0.1",
            "wit/v0.2",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(two_paths.status.code(), Some(2), "{two_paths:?}");
    let two_json: serde_json::Value = serde_json::from_slice(&two_paths.stdout).unwrap();
    assert_eq!(two_json["diagnostics"][0]["code"], "WF9001");

    // - missing second positional path without `--git-base` -> WF9001 (exit 2)
    let one_path = witforge()
        .current_dir(&repo)
        .args(["diff", "wit/v0.1", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(one_path.status.code(), Some(2), "{one_path:?}");
    let one_json: serde_json::Value = serde_json::from_slice(&one_path.stdout).unwrap();
    assert_eq!(one_json["diagnostics"][0]["code"], "WF9001");

    // - leading-`-` flag injection in `--git-base` -> WF9001 (exit 2)
    let dash_rev = witforge()
        .current_dir(&repo)
        .args(["diff", "--git-base=--all", "wit", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(dash_rev.status.code(), Some(2), "{dash_rev:?}");
    let dash_json: serde_json::Value = serde_json::from_slice(&dash_rev.stdout).unwrap();
    assert_eq!(dash_json["diagnostics"][0]["code"], "WF9001");

    // - unknown revision on full clone -> WF9001 (exit 2)
    let bad_rev = witforge()
        .current_dir(&repo)
        .args([
            "diff",
            "--git-base",
            "no-such-ref-xyz",
            "wit",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(bad_rev.status.code(), Some(2), "{bad_rev:?}");
    let bad_json: serde_json::Value = serde_json::from_slice(&bad_rev.stdout).unwrap();
    assert_eq!(bad_json["diagnostics"][0]["code"], "WF9001");

    // - outside any Git repository -> WF9004 (exit 2)
    let outside = root.join("outside-git");
    contract(
        &outside,
        "wit/v0.1",
        "package example:out@0.1.0;\ninterface a { record r { id: u32 } }\n",
    );
    lock(&outside, "wit/v0.1");
    let no_git = witforge()
        .current_dir(&outside)
        .args(["diff", "--git-base", "HEAD", "wit", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(no_git.status.code(), Some(2), "{no_git:?}");
    let no_git_json: serde_json::Value = serde_json::from_slice(&no_git.stdout).unwrap();
    assert_eq!(no_git_json["diagnostics"][0]["code"], "WF9004");

    // - path absent at `<REV>` -> WF9002 (exit 2)
    let missing_at_base = witforge()
        .current_dir(&repo)
        .args([
            "diff",
            "--git-base",
            "HEAD~1",
            "wit/v0.2",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        missing_at_base.status.code(),
        Some(2),
        "{missing_at_base:?}"
    );
    let missing_json: serde_json::Value = serde_json::from_slice(&missing_at_base.stdout).unwrap();
    assert_eq!(missing_json["diagnostics"][0]["code"], "WF9002");

    // 5. Symlink guards (Unix): working-tree target symlink, working-tree child symlink, and
    // committed Git tree symlink (`120000`) all fail with WF9004 (exit 2).
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let link_target = repo.join("wit-link");
        symlink(repo.join("wit"), &link_target).unwrap();
        let sym_target = witforge()
            .current_dir(&repo)
            .args(["diff", "--git-base", "HEAD", "wit-link", "--format", "json"])
            .output()
            .unwrap();
        assert_eq!(sym_target.status.code(), Some(2), "{sym_target:?}");
        let sym_target_json: serde_json::Value =
            serde_json::from_slice(&sym_target.stdout).unwrap();
        assert_eq!(sym_target_json["diagnostics"][0]["code"], "WF9004");
        fs::remove_file(&link_target).unwrap();

        let child_link = repo.join("wit/v0.2/extra-link");
        symlink("contract.wit", &child_link).unwrap();
        let sym_child = witforge()
            .current_dir(&repo)
            .args(["diff", "--git-base", "HEAD", "wit", "--format", "json"])
            .output()
            .unwrap();
        assert_eq!(sym_child.status.code(), Some(2), "{sym_child:?}");
        let sym_child_json: serde_json::Value = serde_json::from_slice(&sym_child.stdout).unwrap();
        assert_eq!(sym_child_json["diagnostics"][0]["code"], "WF9004");

        // Commit the symlink to HEAD, remove it from working tree, and diff against HEAD:
        // Git tree symlink (`120000`) MUST be refused before materialization!
        run_git_cmd(&repo, &["add", "wit/v0.2/extra-link"]);
        run_git_cmd(&repo, &["commit", "-m", "commit symlink"]);
        fs::remove_file(&child_link).unwrap();
        let sym_git = witforge()
            .env("TMPDIR", &private_tmp)
            .current_dir(&repo)
            .args(["diff", "--git-base", "HEAD", "wit", "--format", "json"])
            .output()
            .unwrap();
        assert_eq!(sym_git.status.code(), Some(2), "{sym_git:?}");
        let sym_git_json: serde_json::Value = serde_json::from_slice(&sym_git.stdout).unwrap();
        assert_eq!(sym_git_json["diagnostics"][0]["code"], "WF9004");
        assert_eq!(
            fs::read_dir(&private_tmp).unwrap().count(),
            0,
            "temp dir must be cleaned up when Git symlink guard fires"
        );
    }

    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn eighth_wave_audit_git_base_sibling_released_and_error_path_redaction() {
    let root = workspace("git-base-audit");
    let repo = root.join("repo");
    let private_tmp = root.join("tmp");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&private_tmp).unwrap();

    run_git_cmd(&repo, &["init", "-q"]);
    run_git_cmd(&repo, &["config", "user.email", "test@example.com"]);
    run_git_cmd(&repo, &["config", "user.name", "Test"]);

    // Commit v0.1, v0.2, and `wit/released` listing `v0.1` (sibling of `v0.2`).
    contract(
        &repo,
        "wit/v0.1",
        "package example:relbase@0.1.0;\ninterface types { record item { id: u32 } }\n",
    );
    lock(&repo, "wit/v0.1");
    contract(
        &repo,
        "wit/v0.2",
        "package example:relbase@0.2.0;\ninterface types { record item { id: u32, tag: option<string> } }\n",
    );
    fs::copy(
        repo.join("wit/v0.1/wire.lock"),
        repo.join("wit/v0.2/wire.lock"),
    )
    .unwrap();
    let lock_v02 = witforge()
        .args(["lock"])
        .arg(repo.join("wit/v0.2"))
        .args(["--against"])
        .arg(repo.join("wit/v0.1/wire.lock"))
        .output()
        .unwrap();
    assert!(lock_v02.status.success(), "{lock_v02:?}");
    fs::write(repo.join("wit/released"), "v0.1\n").unwrap();
    run_git_cmd(&repo, &["add", "."]);
    run_git_cmd(&repo, &["commit", "-m", "v0.1 released + v0.2 draft"]);

    // 1. Single-version `diff --git-base HEAD wit/v0.2` MUST succeed when sibling `wit/released`
    // lists `v0.1` (not fail with `WF9002: released directory 'v0.1' does not exist`)!
    contract(
        &repo,
        "wit/v0.2",
        "package example:relbase@0.2.0;\ninterface types { record item { id: u32, tag: option<string>, note: option<string> } }\n",
    );
    let lock_v02_b = witforge()
        .args(["lock"])
        .arg(repo.join("wit/v0.2"))
        .args(["--against"])
        .arg(repo.join("wit/v0.1/wire.lock"))
        .output()
        .unwrap();
    assert!(lock_v02_b.status.success(), "{lock_v02_b:?}");

    let single_ok = witforge()
        .env("TMPDIR", &private_tmp)
        .current_dir(&repo)
        .args(["diff", "--git-base", "HEAD", "wit/v0.2", "--json"])
        .output()
        .unwrap();
    assert!(single_ok.status.success(), "{single_ok:?}");
    let single_json: serde_json::Value = serde_json::from_slice(&single_ok.stdout).unwrap();
    assert_eq!(single_json["old"]["path"], "HEAD:wit/v0.2");
    assert_eq!(single_json["summary"]["changes"], 1);
    assert_eq!(fs::read_dir(&private_tmp).unwrap().count(), 0);

    // 2. When `HEAD:wit/v0.2` itself has a contract/lockfile error (stale lockfile missing field),
    // human stderr, `--format json`, and `--format sarif` MUST NOT leak `witforge-git-base`!
    fs::write(
        repo.join("wit/v0.2/contract.wit"),
        "package example:relbase@0.2.0;\ninterface types { record item { id: u32, tag: option<string>, unlocked: u32 } }\n",
    )
    .unwrap();
    run_git_cmd(&repo, &["add", "wit/v0.2/contract.wit"]);
    run_git_cmd(&repo, &["commit", "-m", "commit unlocked field"]);

    for (mode, extra_args) in [
        ("human", &[][..]),
        ("json", &["--format", "json"][..]),
        ("sarif", &["--format", "sarif"][..]),
    ] {
        let mut cmd = witforge();
        cmd.env("TMPDIR", &private_tmp)
            .current_dir(&repo)
            .args(["diff", "--git-base", "HEAD", "wit/v0.2"])
            .args(extra_args);
        let out = cmd.output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{mode}: {out:?}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stdout.contains("witforge-git-base") && !stderr.contains("witforge-git-base"),
            "{mode} leaked temp path!\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        let combined = format!("{stdout}\n{stderr}");
        assert!(
            combined.contains("HEAD:wit/v0.2") || combined.contains("HEAD%3Awit/v0.2"),
            "{mode} missing rewritten git_base path:\n{combined}"
        );
    }
    assert_eq!(fs::read_dir(&private_tmp).unwrap().count(), 0);

    // 3. Invalid/missing directory listed in sibling `wit/released` at `HEAD` fails with WF9002
    // and clean `HEAD:wit/v9.9` path.
    fs::write(repo.join("wit/released"), "v0.1\nv9.9\n").unwrap();
    run_git_cmd(&repo, &["add", "wit/released"]);
    run_git_cmd(&repo, &["commit", "-m", "broken released entry"]);
    let bad_rel = witforge()
        .env("TMPDIR", &private_tmp)
        .current_dir(&repo)
        .args(["diff", "--git-base", "HEAD", "wit/v0.2", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(bad_rel.status.code(), Some(2), "{bad_rel:?}");
    let bad_rel_json: serde_json::Value = serde_json::from_slice(&bad_rel.stdout).unwrap();
    assert_eq!(bad_rel_json["diagnostics"][0]["code"], "WF9002");
    assert_eq!(bad_rel_json["diagnostics"][0]["path"], "HEAD:wit/v9.9");
    assert_eq!(fs::read_dir(&private_tmp).unwrap().count(), 0);

    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn ninth_wave_audit_git_base_released_dir_nested_git_and_single_quote_redaction() {
    let root = workspace("git-base-ninth");
    let repo = root.join("repo");
    let quoted_tmp = root.join("tmp'quote");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&quoted_tmp).unwrap();

    run_git_cmd(&repo, &["init", "-q"]);
    run_git_cmd(&repo, &["config", "user.email", "test@example.com"]);
    run_git_cmd(&repo, &["config", "user.name", "Test"]);

    contract(
        &repo,
        "wit/v0.1",
        "package example:ninth@0.1.0;\ninterface types { record item { id: u32 } }\n",
    );
    lock(&repo, "wit/v0.1");
    run_git_cmd(&repo, &["add", "."]);
    run_git_cmd(&repo, &["commit", "-m", "v0.1"]);

    // 1. Single quote in TMPDIR: unlocked field at HEAD produces a shell-quoted hint; redaction
    // MUST strip both raw and `'\''`-escaped temporary path variants in human, JSON, and SARIF.
    fs::write(
        repo.join("wit/v0.1/contract.wit"),
        "package example:ninth@0.1.0;\ninterface types { record item { id: u32, extra: u32 } }\n",
    )
    .unwrap();
    run_git_cmd(&repo, &["add", "wit/v0.1/contract.wit"]);
    run_git_cmd(&repo, &["commit", "-m", "unlocked field"]);

    for (mode, extra_args) in [
        ("human", &[][..]),
        ("json", &["--format", "json"][..]),
        ("sarif", &["--format", "sarif"][..]),
    ] {
        let out = witforge()
            .env("TMPDIR", &quoted_tmp)
            .current_dir(&repo)
            .args(["diff", "--git-base", "HEAD", "wit/v0.1"])
            .args(extra_args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1), "{mode}: {out:?}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stdout.contains("witforge-git-base")
                && !stderr.contains("witforge-git-base")
                && !stdout.contains("tmp'quote")
                && !stderr.contains("tmp'quote")
                && !stdout.contains("tmp'\\''quote")
                && !stderr.contains("tmp'\\''quote"),
            "{mode} leaked single-quoted temp path!\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert_eq!(fs::read_dir(&quoted_tmp).unwrap().count(), 0);

    // Fix lockfile at HEAD for subsequent guard checks.
    lock(&repo, "wit/v0.1");
    run_git_cmd(&repo, &["add", "wit/v0.1/wire.lock"]);
    run_git_cmd(&repo, &["commit", "-m", "lock v0.1"]);

    // 2. Sibling `wit/released` committed as a directory (tree `040000`) at HEAD must fail with
    // `WF9004` naming `HEAD:wit/released`.
    fs::create_dir_all(repo.join("wit/released")).unwrap();
    fs::write(repo.join("wit/released/dummy"), "x\n").unwrap();
    run_git_cmd(&repo, &["add", "wit/released/dummy"]);
    run_git_cmd(&repo, &["commit", "-m", "directory released"]);

    let dir_rel = witforge()
        .env("TMPDIR", &quoted_tmp)
        .current_dir(&repo)
        .args(["diff", "--git-base", "HEAD", "wit/v0.1", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(dir_rel.status.code(), Some(2), "{dir_rel:?}");
    let dir_rel_json: serde_json::Value = serde_json::from_slice(&dir_rel.stdout).unwrap();
    assert_eq!(dir_rel_json["diagnostics"][0]["code"], "WF9004");
    assert_eq!(dir_rel_json["diagnostics"][0]["path"], "HEAD:wit/released");
    assert_eq!(fs::read_dir(&quoted_tmp).unwrap().count(), 0);

    // Remove the bogus directory `wit/released`.
    run_git_cmd(&repo, &["rm", "-r", "-q", "wit/released"]);
    run_git_cmd(&repo, &["commit", "-m", "remove directory released"]);

    // 3. Nested `.git` directory inside working-tree target (untracked submodule/nested repo)
    // must be refused with `WF9004`.
    fs::create_dir_all(repo.join("wit/v0.1/nested/.git")).unwrap();
    let nested_git = witforge()
        .env("TMPDIR", &quoted_tmp)
        .current_dir(&repo)
        .args(["diff", "--git-base", "HEAD", "wit/v0.1", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(nested_git.status.code(), Some(2), "{nested_git:?}");
    let nested_json: serde_json::Value = serde_json::from_slice(&nested_git.stdout).unwrap();
    assert_eq!(nested_json["diagnostics"][0]["code"], "WF9004");
    assert_eq!(fs::read_dir(&quoted_tmp).unwrap().count(), 0);

    fs::remove_dir_all(&root).unwrap();
}
