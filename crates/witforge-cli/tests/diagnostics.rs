//! Structured JSON and SARIF diagnostics.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn witforge() -> Command {
    Command::new(env!("CARGO_BIN_EXE_witforge"))
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn output(args: &[&str]) -> std::process::Output {
    witforge().args(args).output().unwrap()
}

fn json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn check_emits_json_and_sarif_with_stable_fields() {
    let refused = repo().join("crates/witforge/tests/fixtures/refused/resource");
    let path = refused.display().to_string();

    let json_output = output(&["check", &path, "--format", "json"]);
    assert_eq!(json_output.status.code(), Some(1), "{json_output:?}");
    let value = json(&json_output);
    assert_eq!(value["version"], 1);
    assert_eq!(value["tool"]["name"], "witforge");
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "WF1001");
    assert_eq!(diagnostics[0]["severity"], "error");
    assert_eq!(diagnostics[0]["path"], "example:bad/types/blob");
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .contains("resources and handles")
    );

    let sarif_output = output(&["check", &path, "--format", "sarif"]);
    assert_eq!(sarif_output.status.code(), Some(1), "{sarif_output:?}");
    let sarif = json(&sarif_output);
    assert_eq!(sarif["version"], "2.1.0");
    assert_eq!(sarif["runs"][0]["tool"]["driver"]["name"], "witforge");
    assert_eq!(
        sarif["runs"][0]["tool"]["driver"]["rules"][0]["id"],
        "WF1001"
    );
    assert_eq!(sarif["runs"][0]["results"][0]["ruleId"], "WF1001");
    assert_eq!(sarif["runs"][0]["results"][0]["level"], "error");
    assert_eq!(
        sarif["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "example:bad/types/blob"
    );

    let human = output(&["check", &path]);
    assert_eq!(human.status.code(), Some(1));
    let text = String::from_utf8_lossy(&human.stderr);
    assert!(text.starts_with("error: example:bad/types/blob:"), "{text}");
    assert!(!text.contains("\"diagnostics\""), "{text}");
}

#[test]
fn successful_check_emits_empty_json_and_sarif() {
    let valid = repo().join("crates/witforge/tests/fixtures/contracts/alint-plugin");
    let json_output = output(&["check", &valid.display().to_string(), "--format", "json"]);
    assert!(json_output.status.success(), "{json_output:?}");
    assert_eq!(json(&json_output)["diagnostics"], serde_json::json!([]));

    let sarif_output = output(&["check", &valid.display().to_string(), "--format", "sarif"]);
    assert!(sarif_output.status.success(), "{sarif_output:?}");
    assert_eq!(
        json(&sarif_output)["runs"][0]["results"],
        serde_json::json!([])
    );
}

#[test]
fn diff_emits_breaking_changes_as_diagnostics() {
    let old = repo().join("examples/evolved/wit/v0.1");
    let new = repo().join("examples/evolved/wit/v0.2");
    let json_output = output(&[
        "diff",
        &old.display().to_string(),
        &new.display().to_string(),
        "--format",
        "json",
        "--check",
    ]);
    assert_eq!(json_output.status.code(), Some(1), "{json_output:?}");
    let value = json(&json_output);
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 2);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic["code"] == "WF3001")
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic["severity"] == "error")
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["path"] == "types/task.priority")
    );

    let sarif_output = output(&[
        "diff",
        &old.display().to_string(),
        &new.display().to_string(),
        "--format",
        "sarif",
    ]);
    assert!(sarif_output.status.success(), "{sarif_output:?}");
    let sarif = json(&sarif_output);
    assert_eq!(sarif["version"], "2.1.0");
    assert_eq!(sarif["runs"][0]["results"][0]["ruleId"], "WF3001");
}

#[test]
fn generator_drift_is_a_structured_diagnostic() {
    let root = std::env::temp_dir().join(format!(
        "witforge-diagnostics-generator-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let source = repo().join("crates/witforge/tests/fixtures/versions/wit");
    let out = root.join("generated");
    let generated = output(&[
        "rust",
        &source.display().to_string(),
        "--out",
        &out.display().to_string(),
    ]);
    assert!(generated.status.success(), "{generated:?}");
    let mod_rs = out.join("mod.rs");
    let mut contents = fs::read_to_string(&mod_rs).unwrap();
    contents.push_str("// edited by hand\n");
    fs::write(&mod_rs, contents).unwrap();

    let checked = output(&[
        "rust",
        &source.display().to_string(),
        "--out",
        &out.display().to_string(),
        "--check",
        "--format",
        "json",
    ]);
    assert_eq!(checked.status.code(), Some(1), "{checked:?}");
    let value = json(&checked);
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "WF2001");
    assert_eq!(diagnostics[0]["severity"], "error");
    assert_eq!(diagnostics[0]["path"], mod_rs.display().to_string());

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn lock_emits_info_diagnostics_for_changes() {
    let root =
        std::env::temp_dir().join(format!("witforge-diagnostics-lock-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("v0.1");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("contract.wit"),
        r"
package example:diag@0.1.0;
interface types {
    record item { id: u64 }
}
",
    )
    .unwrap();

    let locked = output(&[
        "lock",
        &dir.display().to_string(),
        "--first",
        "--format",
        "json",
    ]);
    assert!(locked.status.success(), "{locked:?}");
    let value = json(&locked);
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "WF1201");
    assert_eq!(diagnostics[0]["severity"], "note");
    assert_eq!(diagnostics[0]["path"], "example:diag/types/item.id");

    let unchanged = output(&["lock", &dir.display().to_string(), "--format", "json"]);
    assert!(unchanged.status.success(), "{unchanged:?}");
    assert_eq!(json(&unchanged)["diagnostics"], serde_json::json!([]));

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn check_drift_emits_member_diagnostics() {
    let root = std::env::temp_dir().join(format!(
        "witforge-diagnostics-check-drift-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("v0.1");
    fs::create_dir_all(&dir).unwrap();
    let contract = dir.join("contract.wit");
    fs::write(
        &contract,
        r"
package example:diag@0.1.0;
interface types {
    record item { id: u64 }
}
",
    )
    .unwrap();
    let locked = output(&["lock", &dir.display().to_string(), "--first"]);
    assert!(locked.status.success(), "{locked:?}");
    fs::write(
        &contract,
        r"
package example:diag@0.1.0;
interface types {
    record item { id: u64, name: string }
}
",
    )
    .unwrap();

    let checked = output(&["check", &dir.display().to_string(), "--format", "json"]);
    assert_eq!(checked.status.code(), Some(1), "{checked:?}");
    let value = json(&checked);
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "WF1101");
    assert_eq!(diagnostics[0]["path"], "example:diag/types/item.name");
    assert_eq!(diagnostics[0]["severity"], "error");

    fs::remove_dir_all(&root).unwrap();
}
