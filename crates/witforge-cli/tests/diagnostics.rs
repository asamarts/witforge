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
    assert_eq!(diagnostics[0]["code"], "WF1015");
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
    let rule = &sarif["runs"][0]["tool"]["driver"]["rules"][0];
    assert_eq!(rule["id"], "WF1015");
    assert!(
        !rule["shortDescription"]["text"]
            .as_str()
            .unwrap()
            .is_empty(),
        "{rule}"
    );
    assert_eq!(
        rule["helpUri"],
        "https://asamarts.github.io/witforge/guide/diagnostics.html#wf1015"
    );
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "WF1015");
    assert_eq!(result["level"], "error");
    let uri = result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
        .as_str()
        .unwrap();
    assert!(
        uri.ends_with("contract.wit"),
        "expected physical URI, got {uri}"
    );
    assert!(!uri.contains(".."), "URI must not contain `..`: {uri}");
    assert!(
        result["locations"][0]["physicalLocation"]["region"]["startLine"]
            .as_u64()
            .is_some(),
        "expected SARIF region: {result}"
    );
    assert_eq!(
        result["locations"][0]["logicalLocations"][0]["fullyQualifiedName"],
        "example:bad/types/blob"
    );
    assert!(
        diagnostics[0]["file"]
            .as_str()
            .unwrap()
            .ends_with("contract.wit"),
        "{}",
        diagnostics[0]
    );
    assert_eq!(diagnostics[0]["logical"], "example:bad/types/blob");
    assert!(
        diagnostics[0]["line"].as_u64().is_some(),
        "{}",
        diagnostics[0]
    );
    assert!(
        diagnostics[0]["column"].as_u64().is_some(),
        "{}",
        diagnostics[0]
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
    let results = sarif["runs"][0]["results"].as_array().unwrap();
    assert_eq!(results[0]["ruleId"], "WF3001");
    assert!(
        results.iter().all(|item| item["level"] != "error"),
        "diff without --check exits 0 and must not emit error-level SARIF results: {results:?}"
    );
    assert_eq!(results[0]["level"], "warning");
    let uri = results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
        .as_str()
        .unwrap();
    assert!(
        Path::new(uri)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wit")),
        "expected physical .wit URI: {uri}"
    );
    assert!(
        results[0]["locations"][0]["physicalLocation"]["region"]["startLine"]
            .as_u64()
            .is_some(),
        "{}",
        results[0]
    );

    let sarif_checked = output(&[
        "diff",
        &old.display().to_string(),
        &new.display().to_string(),
        "--format",
        "sarif",
        "--check",
    ]);
    assert_eq!(sarif_checked.status.code(), Some(1), "{sarif_checked:?}");
    let checked_results = json(&sarif_checked)["runs"][0]["results"]
        .as_array()
        .unwrap()
        .clone();
    assert!(
        checked_results
            .iter()
            .any(|item| item["ruleId"] == "WF3001" && item["level"] == "error"),
        "diff --check must emit error-level WF3001 results: {checked_results:?}"
    );
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

#[test]
fn lock_emits_structured_subset_errors() {
    let refused = repo().join("crates/witforge/tests/fixtures/refused/resource");
    let path = refused.display().to_string();
    let output = output(&["lock", &path, "--first", "--format", "json"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let value = json(&output);
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "WF1015");
    assert_eq!(diagnostics[0]["severity"], "error");
    assert_eq!(diagnostics[0]["path"], "example:bad/types/blob");
}

#[test]
fn check_emits_every_released_discrepancy_as_its_own_diagnostic() {
    let root = std::env::temp_dir().join(format!(
        "witforge-diagnostics-discrepancies-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    for version in ["v0.1", "v0.2"] {
        let dir = root.join(version);
        fs::create_dir_all(&dir).unwrap();
        let ty = if version == "v0.1" { "string" } else { "u32" };
        let fields = ["a", "b", "c", "d"]
            .into_iter()
            .map(|name| format!("{name}: {ty}"))
            .collect::<Vec<_>>()
            .join(", ");
        fs::write(
            dir.join("contract.wit"),
            format!(
                "package example:root@{}.0;\ninterface api {{ record r {{ {fields} }} }}\n",
                &version[1..]
            ),
        )
        .unwrap();
    }
    let old = root.join("v0.1");
    let new = root.join("v0.2");
    let first = output(&["lock", old.to_str().unwrap(), "--first"]);
    assert!(first.status.success(), "{first:?}");
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();

    let json_output = output(&[
        "check",
        new.to_str().unwrap(),
        "--against",
        old.join("wire.lock").to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(json_output.status.code(), Some(1), "{json_output:?}");
    let diagnostics = json(&json_output)["diagnostics"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(diagnostics.len(), 4, "{diagnostics:?}");
    for (index, name) in ["a", "b", "c", "d"].into_iter().enumerate() {
        assert_eq!(diagnostics[index]["code"], "WF1100");
        assert_eq!(
            diagnostics[index]["path"],
            format!("example:root/api/r.{name}")
        );
    }

    let sarif_output = output(&[
        "check",
        new.to_str().unwrap(),
        "--against",
        old.join("wire.lock").to_str().unwrap(),
        "--format",
        "sarif",
    ]);
    assert_eq!(sarif_output.status.code(), Some(1), "{sarif_output:?}");
    assert_eq!(
        json(&sarif_output)["runs"][0]["results"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn check_reports_dependency_version_and_case_collision_codes() {
    let root = std::env::temp_dir().join(format!(
        "witforge-diagnostics-subset-gaps-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let single_diag = |args: &[&str]| {
        let out = output(args);
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        let diags = json(&out)["diagnostics"].as_array().unwrap().clone();
        assert_eq!(diags.len(), 1, "{diags:?}");
        diags.into_iter().next().unwrap()
    };

    // 1. Unreferenced dependency with patch version: exit 1, WF1012, names package, version, deps/.
    let dep_bad = root.join("dep-bad/v0.1");
    fs::create_dir_all(dep_bad.join("deps/shared")).unwrap();
    fs::write(
        dep_bad.join("deps/shared/types.wit"),
        "package ex:shared@0.1.3;\ninterface types { record item { id: u32 } }\n",
    )
    .unwrap();
    fs::write(
        dep_bad.join("contract.wit"),
        "package ex:root@0.1.0;\ninterface api { record local { id: u32 } }\n",
    )
    .unwrap();
    let dep_diag = single_diag(&["check", dep_bad.to_str().unwrap(), "--format", "json"]);
    assert_eq!(dep_diag["code"], "WF1012");
    assert_eq!(dep_diag["path"], "ex:shared");
    let dep_msg = dep_diag["message"].as_str().unwrap();
    assert!(dep_msg.contains("0.1.3"), "{dep_msg}");
    assert!(
        dep_msg.contains("deps/shared") || dep_msg.contains(r"deps\shared"),
        "{dep_msg}"
    );

    // 2. Two interfaces differing only in ASCII letter case: exit 1, WF1003.
    let iface_case = root.join("iface-case/v0.1");
    fs::create_dir_all(&iface_case).unwrap();
    fs::write(
        iface_case.join("contract.wit"),
        "package ex:root@0.1.0;\ninterface api { record a { x: u32 } }\ninterface API { record b { y: u32 } }\n",
    )
    .unwrap();
    let iface_diag = single_diag(&["check", iface_case.to_str().unwrap(), "--format", "json"]);
    assert_eq!(iface_diag["code"], "WF1003");
    assert_eq!(iface_diag["path"], "ex:root/API");

    // 3. Reintroducing a reserved member in another letter case: exit 1, WF1003 for both
    //    `check` and `lock`, and `lock` writes nothing.
    let v1 = root.join("reserved-case/v0.1");
    let v2 = root.join("reserved-case/v0.2");
    fs::create_dir_all(&v1).unwrap();
    fs::create_dir_all(&v2).unwrap();
    fs::write(
        v1.join("contract.wit"),
        "package ex:root@0.1.0;\ninterface api { record r { keep: u32, foo-bar: u32 } }\n",
    )
    .unwrap();
    assert!(
        output(&["lock", v1.to_str().unwrap(), "--first"])
            .status
            .success()
    );

    let v1_lock = v1.join("wire.lock");
    fs::copy(&v1_lock, v2.join("wire.lock")).unwrap();
    fs::write(
        v2.join("contract.wit"),
        "package ex:root@0.2.0;\ninterface api { record r { keep: u32 } }\n",
    )
    .unwrap();
    assert!(
        output(&[
            "lock",
            v2.to_str().unwrap(),
            "--against",
            v1_lock.to_str().unwrap()
        ])
        .status
        .success()
    );
    let lock_before = fs::read_to_string(v2.join("wire.lock")).unwrap();

    fs::write(
        v2.join("contract.wit"),
        "package ex:root@0.2.0;\ninterface api { record r { keep: u32, FOO-bar: u32 } }\n",
    )
    .unwrap();
    for subcmd in ["check", "lock"] {
        let diag = single_diag(&[
            subcmd,
            v2.to_str().unwrap(),
            "--against",
            v1_lock.to_str().unwrap(),
            "--format",
            "json",
        ]);
        assert_eq!(diag["code"], "WF1003", "{subcmd}: {diag}");
        assert_eq!(diag["path"], "ex:root/api/r.FOO-bar", "{subcmd}: {diag}");
    }
    assert_eq!(
        fs::read_to_string(v2.join("wire.lock")).unwrap(),
        lock_before,
        "refused lock must leave wire.lock untouched"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn all_commands_report_identical_per_rule_codes_and_multiple_diagnostics() {
    let root = std::env::temp_dir().join(format!(
        "witforge-diagnostics-cross-cmd-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let wit_root = root.join("wit");
    let v01 = wit_root.join("v0.1");
    fs::create_dir_all(&v01).unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package ex:h@0.1.0;\ninterface types { record r { a: tuple<u32, u32>, b: char } }\n",
    )
    .unwrap();
    fs::write(v01.join("wire.lock"), "# witforge wire lockfile v2\n").unwrap();

    let good_v01 = root.join("good/v0.1");
    fs::create_dir_all(&good_v01).unwrap();
    fs::write(
        good_v01.join("contract.wit"),
        "package ex:h@0.1.0;\ninterface types { record r { id: u32 } }\n",
    )
    .unwrap();
    assert!(
        output(&["lock", good_v01.to_str().unwrap(), "--first"])
            .status
            .success()
    );

    let out_rust = root.join("out-rust");
    let out_proto = root.join("out-proto");
    let out_schema = root.join("out-schema");
    let out_glue = root.join("out-glue");

    let commands: Vec<(&str, Vec<String>)> = vec![
        ("check", vec!["check".into(), v01.display().to_string()]),
        (
            "lock",
            vec!["lock".into(), v01.display().to_string(), "--first".into()],
        ),
        (
            "diff",
            vec![
                "diff".into(),
                good_v01.display().to_string(),
                v01.display().to_string(),
            ],
        ),
        (
            "rust",
            vec![
                "rust".into(),
                wit_root.display().to_string(),
                "--out".into(),
                out_rust.display().to_string(),
            ],
        ),
        (
            "proto",
            vec![
                "proto".into(),
                wit_root.display().to_string(),
                "--out".into(),
                out_proto.display().to_string(),
            ],
        ),
        (
            "schema",
            vec![
                "schema".into(),
                wit_root.display().to_string(),
                "--out".into(),
                out_schema.display().to_string(),
            ],
        ),
        (
            "glue",
            vec![
                "glue".into(),
                wit_root.display().to_string(),
                "--out".into(),
                out_glue.display().to_string(),
                "--plain-root".into(),
                "crate::plain".into(),
                "--proto-root".into(),
                "crate::proto".into(),
            ],
        ),
    ];

    let mut expected_json: Option<Vec<(String, String, String)>> = None;
    for (label, base_args) in &commands {
        let mut json_args: Vec<&str> = base_args.iter().map(String::as_str).collect();
        json_args.extend(["--format", "json"]);
        let out = output(&json_args);
        assert_eq!(out.status.code(), Some(1), "{label} json: {out:?}");
        let diags = json(&out)["diagnostics"].as_array().unwrap().clone();
        assert_eq!(
            diags.len(),
            2,
            "{label} json must not join diagnostics: {diags:?}"
        );
        assert_eq!(diags[0]["code"], "WF1019", "{label}: {diags:?}");
        assert_eq!(diags[0]["path"], "ex:h/types/r.a", "{label}: {diags:?}");
        assert_eq!(diags[1]["code"], "WF1017", "{label}: {diags:?}");
        assert_eq!(diags[1]["path"], "ex:h/types/r.b", "{label}: {diags:?}");
        let summary: Vec<(String, String, String)> = diags
            .iter()
            .map(|d| {
                (
                    d["code"].as_str().unwrap().to_string(),
                    d["path"].as_str().unwrap().to_string(),
                    d["message"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        if let Some(first) = &expected_json {
            assert_eq!(&summary, first, "{label} diverged from check");
        } else {
            expected_json = Some(summary);
        }

        let mut sarif_args: Vec<&str> = base_args.iter().map(String::as_str).collect();
        sarif_args.extend(["--format", "sarif"]);
        let sarif_out = output(&sarif_args);
        assert_eq!(
            sarif_out.status.code(),
            Some(1),
            "{label} sarif: {sarif_out:?}"
        );
        let results = json(&sarif_out)["runs"][0]["results"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(results.len(), 2, "{label} sarif: {results:?}");
        assert_eq!(results[0]["ruleId"], "WF1019", "{label}: {results:?}");
        assert_eq!(
            results[0]["locations"][0]["logicalLocations"][0]["fullyQualifiedName"],
            "ex:h/types/r.a"
        );
        let uri0 = results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
            .as_str()
            .unwrap();
        assert!(
            uri0.starts_with("file:///") && uri0.ends_with("contract.wit"),
            "{label}: expected file:/// URI for outside-cwd contract, got {uri0}"
        );
        assert_eq!(
            results[0]["locations"][0]["physicalLocation"]["region"]["startLine"],
            2
        );
        assert_eq!(results[1]["ruleId"], "WF1017", "{label}: {results:?}");
        assert_eq!(
            results[1]["locations"][0]["logicalLocations"][0]["fullyQualifiedName"],
            "ex:h/types/r.b"
        );
        let uri1 = results[1]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
            .as_str()
            .unwrap();
        assert!(
            uri1.starts_with("file:///") && uri1.ends_with("contract.wit"),
            "{label}: expected file:/// URI, got {uri1}"
        );
        assert_eq!(
            results[1]["locations"][0]["physicalLocation"]["region"]["startLine"],
            2
        );
    }

    // Conversion refusal in `rust` gets WF2002 with logical path, never WF9000.
    let conv_root = root.join("conv-wit");
    let c1 = conv_root.join("v0.1");
    let c2 = conv_root.join("v0.2");
    fs::create_dir_all(&c1).unwrap();
    fs::create_dir_all(&c2).unwrap();
    fs::write(
        c1.join("contract.wit"),
        "package ex:c@0.1.0;\ninterface types { record item { id: u32 } }\n",
    )
    .unwrap();
    fs::write(
        c2.join("contract.wit"),
        "package ex:c@0.2.0;\ninterface types { record item { id: u32, req: string } }\n",
    )
    .unwrap();
    assert!(
        output(&["lock", c1.to_str().unwrap(), "--first"])
            .status
            .success()
    );
    fs::copy(c1.join("wire.lock"), c2.join("wire.lock")).unwrap();
    assert!(
        output(&[
            "lock",
            c2.to_str().unwrap(),
            "--against",
            c1.join("wire.lock").to_str().unwrap(),
        ])
        .status
        .success()
    );
    let conv_out = output(&[
        "rust",
        conv_root.to_str().unwrap(),
        "--out",
        root.join("conv-out").to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(conv_out.status.code(), Some(1), "{conv_out:?}");
    let conv_diags = json(&conv_out)["diagnostics"].as_array().unwrap().clone();
    assert_eq!(conv_diags.len(), 1, "{conv_diags:?}");
    assert_eq!(conv_diags[0]["code"], "WF2002");
    assert_eq!(conv_diags[0]["path"], "types/item.req");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn usage_errors_emit_json_and_sarif_on_stdout() {
    // Missing required argument before subcommand completes parsing.
    let json_missing = output(&["--format", "json", "check"]);
    assert_eq!(json_missing.status.code(), Some(2), "{json_missing:?}");
    let value = json(&json_missing);
    let diags = value["diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0]["code"], "WF9001");
    assert_eq!(diags[0]["severity"], "error");

    let sarif_missing = output(&["check", "--format=sarif"]);
    assert_eq!(sarif_missing.status.code(), Some(2), "{sarif_missing:?}");
    let sarif = json(&sarif_missing);
    assert_eq!(sarif["version"], "2.1.0");
    assert_eq!(sarif["runs"][0]["results"][0]["ruleId"], "WF9001");

    // Missing --out on generator is also WF9001 (never WF9000).
    let missing_out = output(&["rust", "wit", "--format", "json"]);
    assert_eq!(missing_out.status.code(), Some(2), "{missing_out:?}");
    assert_eq!(json(&missing_out)["diagnostics"][0]["code"], "WF9001");

    // Reversed diff endpoints emit WF9001.
    let old = repo().join("examples/evolved/wit/v0.1");
    let new = repo().join("examples/evolved/wit/v0.2");
    let reversed = output(&[
        "diff",
        &new.display().to_string(),
        &old.display().to_string(),
        "--format",
        "json",
    ]);
    assert_eq!(reversed.status.code(), Some(2), "{reversed:?}");
    assert_eq!(json(&reversed)["diagnostics"][0]["code"], "WF9001");

    // Passing a WIT root to `check` emits WF9001.
    let wit_root = repo().join("examples/evolved/wit");
    let root_check = output(&["check", &wit_root.display().to_string(), "--format", "json"]);
    assert_eq!(root_check.status.code(), Some(2), "{root_check:?}");
    assert_eq!(json(&root_check)["diagnostics"][0]["code"], "WF9001");

    // `--help` still prints help text and exits 0 even with `--format json`.
    let help = output(&["--format", "json", "--help"]);
    assert!(help.status.success(), "{help:?}");
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));
}

#[allow(clippy::too_many_lines)]
#[test]
fn operational_errors_have_specific_codes_and_never_emit_wf9000() {
    let root = std::env::temp_dir().join(format!(
        "witforge-diagnostics-operational-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    // 1. Missing directory -> WF9002 (exit 2).
    let missing = root.join("does-not-exist");
    let out = output(&["check", missing.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(json(&out)["diagnostics"][0]["code"], "WF9002");

    // 2. Missing wire.lock -> WF9002 (exit 2).
    let unlocked = root.join("unlocked/v0.1");
    fs::create_dir_all(&unlocked).unwrap();
    fs::write(
        unlocked.join("contract.wit"),
        "package ex:op@0.1.0;\ninterface types { record r { id: u32 } }\n",
    )
    .unwrap();
    let out = output(&["check", unlocked.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(json(&out)["diagnostics"][0]["code"], "WF9002");

    // 3. Malformed wire.lock -> WF9003 (exit 2) with lockfile path.
    let bad_lock_dir = root.join("bad-lock").join("v0.1");
    fs::create_dir_all(&bad_lock_dir).unwrap();
    fs::write(
        bad_lock_dir.join("contract.wit"),
        "package ex:op@0.1.0;\ninterface types { record r { id: u32 } }\n",
    )
    .unwrap();
    let bad_lock = bad_lock_dir.join("wire.lock");
    fs::write(&bad_lock, "not a lockfile\n").unwrap();
    let out = output(&["check", bad_lock_dir.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let diag = &json(&out)["diagnostics"][0];
    assert_eq!(diag["code"], "WF9003");
    assert_eq!(
        fs::canonicalize(Path::new(diag["path"].as_str().unwrap())).unwrap(),
        fs::canonicalize(&bad_lock).unwrap()
    );

    // 4. Malformed released manifest -> WF9003 (exit 2) with manifest path.
    let bad_manifest_root = root.join("bad-manifest");
    let bm_v01 = bad_manifest_root.join("v0.1");
    fs::create_dir_all(&bm_v01).unwrap();
    fs::write(
        bm_v01.join("contract.wit"),
        "package ex:op@0.1.0;\ninterface types { record r { id: u32 } }\n",
    )
    .unwrap();
    let manifest_path = bad_manifest_root.join("released");
    fs::write(&manifest_path, "../escape\n").unwrap();
    let out = output(&["check", bm_v01.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let diag = &json(&out)["diagnostics"][0];
    assert_eq!(diag["code"], "WF9003");
    assert_eq!(
        fs::canonicalize(Path::new(diag["path"].as_str().unwrap())).unwrap(),
        fs::canonicalize(&manifest_path).unwrap()
    );

    // 5. Malformed WIT file -> WF9003 (exit 2).
    let bad_wit = root.join("bad-wit/v0.1");
    fs::create_dir_all(&bad_wit).unwrap();
    fs::write(bad_wit.join("contract.wit"), "package broken @@@;\n").unwrap();
    let out = output(&["check", bad_wit.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(json(&out)["diagnostics"][0]["code"], "WF9003");

    // 6. Malformed witforge.toml -> WF9003 (exit 2); unexpanded ~ -> WF9004 (exit 2).
    let bad_toml = root.join("bad.toml");
    fs::write(&bad_toml, "version = [not valid\n").unwrap();
    let out = output(&[
        "rust",
        "--config",
        bad_toml.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(json(&out)["diagnostics"][0]["code"], "WF9003");

    let tilde_toml = root.join("tilde.toml");
    fs::write(
        &tilde_toml,
        "version = 1\n[project.demo]\nroot = \"~/wit\"\n[project.demo.rust]\nout = \"out\"\n",
    )
    .unwrap();
    let out = output(&[
        "rust",
        "--config",
        tilde_toml.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(json(&out)["diagnostics"][0]["code"], "WF9004");

    // 7. Locking a released version directory -> WF9004 (exit 2).
    let rel_root = root.join("released-root");
    let rel_v01 = rel_root.join("v0.1");
    fs::create_dir_all(&rel_v01).unwrap();
    fs::write(
        rel_v01.join("contract.wit"),
        "package ex:op@0.1.0;\ninterface types { record r { id: u32 } }\n",
    )
    .unwrap();
    assert!(
        output(&["lock", rel_v01.to_str().unwrap(), "--first"])
            .status
            .success()
    );
    fs::write(rel_root.join("released"), "v0.1\n").unwrap();
    let out = output(&["lock", rel_v01.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(json(&out)["diagnostics"][0]["code"], "WF9004");

    // 7b. Output root with another generator's header on an expected path -> WF9004 (exit 2).
    let foreign_out = root.join("foreign-out");
    fs::create_dir_all(&foreign_out).unwrap();
    fs::write(
        foreign_out.join("mod.rs"),
        "// @generated by witforge (glue, ex:op); do not edit.\n",
    )
    .unwrap();
    let out = output(&[
        "rust",
        rel_root.to_str().unwrap(),
        "--out",
        foreign_out.to_str().unwrap(),
        "--check",
        "--format",
        "json",
    ]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(json(&out)["diagnostics"][0]["code"], "WF9004");

    // 8. Permission error (Unix, non-root) -> WF9002 (exit 2).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perm_dir = root.join("perm/v0.1");
        fs::create_dir_all(&perm_dir).unwrap();
        let wit_file = perm_dir.join("contract.wit");
        fs::write(
            &wit_file,
            "package ex:op@0.1.0;\ninterface t { record r { a: u32 } }\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&wit_file).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&wit_file, perms.clone()).unwrap();
        // Skip assertion if running as root where mode 0000 is still readable.
        if fs::read_to_string(&wit_file).is_err() {
            let out = output(&["check", perm_dir.to_str().unwrap(), "--format", "json"]);
            assert_eq!(out.status.code(), Some(2), "{out:?}");
            assert_eq!(json(&out)["diagnostics"][0]["code"], "WF9002");
        }
        perms.set_mode(0o644);
        fs::set_permissions(&wit_file, perms).unwrap();
    }

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn foreign_package_lockfile_emits_wf1002_with_offending_file_path() {
    let root = std::env::temp_dir().join(format!(
        "witforge-diagnostics-wf1002-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let pkg_a = root.join("pkg-a").join("v0.1");
    let pkg_b = root.join("pkg-b").join("v0.1");
    fs::create_dir_all(&pkg_a).unwrap();
    fs::create_dir_all(&pkg_b).unwrap();
    fs::write(
        pkg_a.join("contract.wit"),
        "package ex:alpha@0.1.0;\ninterface types { record r { id: u32 } }\n",
    )
    .unwrap();
    fs::write(
        pkg_b.join("contract.wit"),
        "package ex:beta@0.1.0;\ninterface types { record r { id: u32 } }\n",
    )
    .unwrap();
    assert!(
        output(&["lock", pkg_a.to_str().unwrap(), "--first"])
            .status
            .success()
    );
    assert!(
        output(&["lock", pkg_b.to_str().unwrap(), "--first"])
            .status
            .success()
    );

    let beta_lock = pkg_b.join("wire.lock");
    // `--against` points to beta's lockfile: both `check` and `lock` must report WF1002 with
    // `path` naming `beta_lock` (NOT alpha's lockfile and NOT WF9000).
    for subcmd in ["check", "lock"] {
        let out = output(&[
            subcmd,
            pkg_a.to_str().unwrap(),
            "--against",
            beta_lock.to_str().unwrap(),
            "--format",
            "json",
        ]);
        assert_eq!(out.status.code(), Some(1), "{subcmd}: {out:?}");
        let diags = json(&out)["diagnostics"].as_array().unwrap().clone();
        assert_eq!(diags.len(), 1, "{subcmd}: {diags:?}");
        assert_eq!(diags[0]["code"], "WF1002", "{subcmd}: {diags:?}");
        assert_eq!(
            fs::canonicalize(Path::new(diags[0]["path"].as_str().unwrap())).unwrap(),
            fs::canonicalize(&beta_lock).unwrap(),
            "{subcmd} must name the foreign baseline file, not the current wire.lock"
        );
    }

    // Current wire.lock replaced with beta's lockfile: `check`, `lock`, and `rust` all emit WF1002
    // naming alpha's wire.lock.
    let alpha_lock = pkg_a.join("wire.lock");
    fs::copy(&beta_lock, &alpha_lock).unwrap();
    for (label, args) in [
        (
            "check",
            vec!["check", pkg_a.to_str().unwrap(), "--format", "json"],
        ),
        (
            "lock",
            vec!["lock", pkg_a.to_str().unwrap(), "--format", "json"],
        ),
        (
            "rust",
            vec![
                "rust",
                root.join("pkg-a").to_str().unwrap(),
                "--out",
                root.join("out-a").to_str().unwrap(),
                "--format",
                "json",
            ],
        ),
    ] {
        let out = output(&args);
        assert_eq!(out.status.code(), Some(1), "{label}: {out:?}");
        let diags = json(&out)["diagnostics"].as_array().unwrap().clone();
        assert_eq!(diags.len(), 1, "{label}: {diags:?}");
        assert_eq!(diags[0]["code"], "WF1002", "{label}: {diags:?}");
        assert_eq!(
            fs::canonicalize(Path::new(diags[0]["path"].as_str().unwrap())).unwrap(),
            fs::canonicalize(&alpha_lock).unwrap(),
            "{label}: {diags:?}"
        );
    }

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_json_wins_over_non_human_format() {
    let old = repo().join("examples/evolved/wit/v0.1");
    let new = repo().join("examples/evolved/wit/v0.2");

    // Success (with breaking changes and without `--check`): JSON diff report wins over SARIF.
    let out = output(&[
        "diff",
        &old.display().to_string(),
        &new.display().to_string(),
        "--json",
        "--format",
        "sarif",
    ]);
    assert!(out.status.success(), "{out:?}");
    let value = json(&out);
    assert!(
        value.get("old").is_some(),
        "expected diff JSON, got {value}"
    );
    assert!(
        value.get("changes").is_some(),
        "expected diff JSON, got {value}"
    );
    assert!(
        value.get("$schema").is_none(),
        "must not emit SARIF: {value}"
    );

    // Breaking changes with `--check` and `--format sarif` before subcommand: exit 1, still JSON.
    let checked = output(&[
        "--format",
        "sarif",
        "diff",
        &old.display().to_string(),
        &new.display().to_string(),
        "--json",
        "--check",
    ]);
    assert_eq!(checked.status.code(), Some(1), "{checked:?}");
    let checked_json = json(&checked);
    assert!(checked_json.get("summary").is_some(), "{checked_json}");
    assert!(checked_json.get("$schema").is_none(), "{checked_json}");

    // Command error (reversed versions) with `--json --format sarif`: exit 2, never prints SARIF.
    let err_out = output(&[
        "diff",
        &new.display().to_string(),
        &old.display().to_string(),
        "--json",
        "--format",
        "sarif",
    ]);
    assert_eq!(err_out.status.code(), Some(2), "{err_out:?}");
    assert!(
        err_out.stdout.is_empty(),
        "diff --json error must not emit SARIF on stdout: {}",
        String::from_utf8_lossy(&err_out.stdout)
    );
    let stderr = String::from_utf8_lossy(&err_out.stderr);
    assert!(stderr.contains("cannot diff version"), "{stderr}");
}

#[test]
fn diagnostic_catalog_matches_docs_design_diagnostics_md() {
    use std::collections::BTreeMap;

    let doc_path = repo().join("docs/design/diagnostics.md");
    let text = fs::read_to_string(&doc_path).unwrap();
    let mut doc_entries: BTreeMap<String, (String, Option<String>)> = BTreeMap::new();
    let mut in_table = false;
    for line in text.lines() {
        if line.starts_with("## Stable codes") {
            in_table = true;
            continue;
        }
        if in_table && line.starts_with("## ") {
            break;
        }
        if !in_table || !line.starts_with("| `WF") {
            continue;
        }
        let cols: Vec<&str> = line.split('|').map(str::trim).collect();
        assert!(cols.len() >= 5, "unexpected table row: {line}");
        let code = cols[1].trim_matches('`').to_string();
        let severity = cols[2].to_string();
        let prior = if cols[3] == "—" {
            None
        } else {
            Some(cols[3].trim_matches('`').to_string())
        };
        assert!(
            doc_entries
                .insert(code.clone(), (severity, prior))
                .is_none(),
            "duplicate code `{code}` in docs/design/diagnostics.md"
        );
    }

    let catalog_entries: BTreeMap<String, (String, Option<String>)> = witforge::diagnostic::CATALOG
        .iter()
        .map(|info| {
            (
                info.code.to_string(),
                (
                    info.severity.as_str().to_string(),
                    info.supersedes.map(str::to_string),
                ),
            )
        })
        .collect();

    assert_eq!(
        catalog_entries, doc_entries,
        "witforge::diagnostic::CATALOG and docs/design/diagnostics.md must stay 1-to-1"
    );

    // Every code must also have a public anchor heading `### WFxxxx` in docs/guide/diagnostics.md
    // so SARIF `helpUri` links resolve on the published documentation site.
    let guide_path = repo().join("docs/guide/diagnostics.md");
    let guide_text = fs::read_to_string(&guide_path).unwrap();
    let guide_codes: std::collections::BTreeSet<String> = guide_text
        .lines()
        .filter_map(|line| line.strip_prefix("### "))
        .map(|code| code.trim().to_string())
        .collect();
    let catalog_codes: std::collections::BTreeSet<String> =
        catalog_entries.keys().cloned().collect();
    assert_eq!(
        catalog_codes, guide_codes,
        "witforge::diagnostic::CATALOG and docs/guide/diagnostics.md anchors must stay 1-to-1"
    );
}

#[test]
fn wit_syntax_and_resolution_errors_carry_file_line_column_and_snippet() {
    let root = std::env::temp_dir().join(format!(
        "witforge-diagnostics-parse-pos-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("wit/v0.1");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("a.wit"),
        "package ex:multi@0.1.0;\ninterface ok {\n    record item { id: u32 }\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("b.wit"),
        "interface broken {\n    record wrapper {\n        missing: nosuch,\n    }\n}\n",
    )
    .unwrap();

    // Resolution error on line 3 of b.wit: human output renders snippet with b.wit:3:...
    let human = output(&["check", dir.to_str().unwrap()]);
    assert_eq!(human.status.code(), Some(2), "{human:?}");
    let stderr = String::from_utf8_lossy(&human.stderr);
    assert!(
        stderr.contains("b.wit:3:"),
        "expected b.wit:3 in stderr:\n{stderr}"
    );
    assert!(stderr.contains("nosuch"), "{stderr}");

    // JSON and SARIF carry exact file, line 3, and column (in UTF-16 code units).
    let json_out = output(&["check", dir.to_str().unwrap(), "--format", "json"]);
    assert_eq!(json_out.status.code(), Some(2), "{json_out:?}");
    let diag = &json(&json_out)["diagnostics"][0];
    assert_eq!(diag["code"], "WF9003", "{diag}");
    assert!(
        diag["file"].as_str().unwrap().ends_with("b.wit"),
        "expected b.wit: {diag}"
    );
    assert_eq!(diag["line"], 3, "{diag}");
    assert_eq!(diag["column"], 18, "{diag}");

    let sarif_out = output(&["check", dir.to_str().unwrap(), "--format", "sarif"]);
    assert_eq!(sarif_out.status.code(), Some(2), "{sarif_out:?}");
    let result = &json(&sarif_out)["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "WF9003", "{result}");
    let phys = &result["locations"][0]["physicalLocation"];
    assert!(
        phys["artifactLocation"]["uri"]
            .as_str()
            .unwrap()
            .ends_with("b.wit"),
        "{phys}"
    );
    assert_eq!(phys["region"]["startLine"], 3, "{phys}");
    assert_eq!(phys["region"]["startColumn"], 18, "{phys}");

    // Syntax error with tab + non-ASCII + CRLF on line 3.
    let syn_dir = root.join("syntax/v0.1");
    fs::create_dir_all(&syn_dir).unwrap();
    fs::write(
        syn_dir.join("contract.wit"),
        "package ex:syn@0.1.0;\r\n// café 😀\r\n\tinterface broken { @@@ }\r\n",
    )
    .unwrap();
    let syn_json = output(&["check", syn_dir.to_str().unwrap(), "--format", "json"]);
    assert_eq!(syn_json.status.code(), Some(2), "{syn_json:?}");
    let syn_diag = &json(&syn_json)["diagnostics"][0];
    assert_eq!(syn_diag["code"], "WF9003", "{syn_diag}");
    assert!(syn_diag["file"].as_str().unwrap().ends_with("contract.wit"));
    assert_eq!(syn_diag["line"], 3, "{syn_diag}");
    assert!(syn_diag["column"].as_u64().unwrap() >= 2, "{syn_diag}");

    // UTF-8 BOM on a WIT file does not shift line 1 columns.
    let bom_dir = root.join("bom/v0.1");
    fs::create_dir_all(&bom_dir).unwrap();
    fs::write(
        bom_dir.join("contract.wit"),
        "\u{feff}package ex:bom@0.1.0; interface a { record r { bad: char } }\n",
    )
    .unwrap();
    let bom_json = output(&["check", bom_dir.to_str().unwrap(), "--format", "json"]);
    assert_eq!(bom_json.status.code(), Some(1), "{bom_json:?}");
    let bom_diag = &json(&bom_json)["diagnostics"][0];
    assert_eq!(bom_diag["code"], "WF1017", "{bom_diag}");
    assert_eq!(bom_diag["line"], 1, "{bom_diag}");
    // `package ex:bom@0.1.0; interface a { record r { ` is 47 chars -> 1-based col 48 (BOM not counted).
    assert_eq!(bom_diag["column"], 48, "{bom_diag}");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn subset_use_deps_utf16_columns_and_cwd_relative_sarif_uris() {
    let root = std::env::temp_dir().join(format!(
        "witforge-diagnostics-positions-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("wit/v0.1");
    fs::create_dir_all(dir.join("deps/shared")).unwrap();
    // Tab (1 UTF-16 unit) + comment with supplementary emoji on line above + CRLF + `use` into deps.
    fs::write(
        dir.join("deps/shared/types.wit"),
        "package ex:shared@0.1.0;\r\ninterface base {\r\n    // 😀 café\r\n\trecord bad {\r\n\t\tflag-char: char,\r\n\t}\r\n}\r\n",
    )
    .unwrap();
    fs::write(
        dir.join("contract.wit"),
        "package ex:app@0.1.0;\ninterface api {\n    use ex:shared/base@0.1.0.{bad};\n    record holder { item: bad }\n}\n",
    )
    .unwrap();

    // Run with cwd = `root` so SARIF URI MUST be cwd-relative with forward slashes, no `..`.
    let sarif_out = witforge()
        .current_dir(&root)
        .args(["check", "wit/v0.1", "--format", "sarif"])
        .output()
        .unwrap();
    assert_eq!(sarif_out.status.code(), Some(1), "{sarif_out:?}");
    let sarif = json(&sarif_out);
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "WF1017", "{result}");
    // Position MUST be the declaration in deps/shared/types.wit (line 5), NOT the `use` in contract.wit!
    assert_eq!(
        result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "wit/v0.1/deps/shared/types.wit"
    );
    assert_eq!(
        result["locations"][0]["physicalLocation"]["region"]["startLine"],
        5
    );
    // Two leading tabs on line 5 -> `flag-char` starts at 1-based UTF-16 column 3!
    assert_eq!(
        result["locations"][0]["physicalLocation"]["region"]["startColumn"],
        3
    );
    assert_eq!(
        result["locations"][0]["logicalLocations"][0]["fullyQualifiedName"],
        "ex:shared/base/bad.flag-char"
    );

    // Inline UTF-16 column check with tab + BMP + supplementary char on the same line.
    let col_dir = root.join("cols/v0.1");
    fs::create_dir_all(&col_dir).unwrap();
    fs::write(
        col_dir.join("contract.wit"),
        "package ex:col@0.1.0;\ninterface a {\n\trecord r { /* café 😀 */ bad: char }\n}\n",
    )
    .unwrap();
    let col_json = witforge()
        .current_dir(&root)
        .args(["check", "cols/v0.1", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(col_json.status.code(), Some(1), "{col_json:?}");
    let col_diag = &json(&col_json)["diagnostics"][0];
    assert_eq!(col_diag["file"], "cols/v0.1/contract.wit");
    assert_eq!(col_diag["line"], 3);
    // `\t` (1) + `record r { /* café ` (19) + `😀` (2 UTF-16 units) + ` */ ` (4) = 26 -> 1-based col 27!
    assert_eq!(col_diag["column"], 27, "{col_diag}");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn lockfile_parse_and_discrepancy_carry_wire_lock_lines_and_related_locations() {
    let root = std::env::temp_dir().join(format!(
        "witforge-diagnostics-lock-pos-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let v1 = root.join("wit/v0.1");
    let v2 = root.join("wit/v0.2");
    fs::create_dir_all(&v1).unwrap();
    fs::create_dir_all(&v2).unwrap();
    fs::write(
        v1.join("contract.wit"),
        "package ex:lp@0.1.0;\ninterface api {\n    record item {\n        id: u32,\n        label: string,\n    }\n}\n",
    )
    .unwrap();
    assert!(
        output(&["lock", v1.to_str().unwrap(), "--first"])
            .status
            .success()
    );

    // 1. Lockfile parse error on line 3 carries wire.lock file and line 3.
    let bad_parse = root.join("bad-parse/v0.1");
    fs::create_dir_all(&bad_parse).unwrap();
    fs::write(
        bad_parse.join("contract.wit"),
        "package ex:lp@0.1.0;\ninterface api { record item { id: u32 } }\n",
    )
    .unwrap();
    fs::write(
        bad_parse.join("wire.lock"),
        "# witforge wire lockfile v2\n\nnot a valid entry\n",
    )
    .unwrap();
    let parse_out = witforge()
        .current_dir(&root)
        .args(["check", "bad-parse/v0.1", "--format", "sarif"])
        .output()
        .unwrap();
    assert_eq!(parse_out.status.code(), Some(2), "{parse_out:?}");
    let parse_res = &json(&parse_out)["runs"][0]["results"][0];
    assert_eq!(parse_res["ruleId"], "WF9003");
    assert_eq!(
        parse_res["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "bad-parse/v0.1/wire.lock"
    );
    assert_eq!(
        parse_res["locations"][0]["physicalLocation"]["region"]["startLine"],
        3
    );

    // 2. Discrepancy between WIT and wire.lock carries primary WIT location + related wire.lock line!
    fs::write(
        v2.join("contract.wit"),
        "package ex:lp@0.2.0;\ninterface api {\n    record item {\n        id: u32,\n        label: u64,\n    }\n}\n",
    )
    .unwrap();
    fs::copy(v1.join("wire.lock"), v2.join("wire.lock")).unwrap();
    let disc_out = witforge()
        .current_dir(&root)
        .args([
            "check",
            "wit/v0.2",
            "--against",
            "wit/v0.1/wire.lock",
            "--format",
            "sarif",
        ])
        .output()
        .unwrap();
    assert_eq!(disc_out.status.code(), Some(1), "{disc_out:?}");
    let disc_res = &json(&disc_out)["runs"][0]["results"][0];
    assert_eq!(disc_res["ruleId"], "WF1100", "{disc_res}");
    assert_eq!(
        disc_res["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "wit/v0.2/contract.wit"
    );
    assert_eq!(
        disc_res["locations"][0]["physicalLocation"]["region"]["startLine"],
        5
    );
    assert_eq!(
        disc_res["locations"][0]["logicalLocations"][0]["fullyQualifiedName"],
        "ex:lp/api/item.label"
    );
    let related = disc_res["relatedLocations"].as_array().unwrap();
    assert_eq!(related.len(), 1, "{disc_res}");
    assert_eq!(
        related[0]["physicalLocation"]["artifactLocation"]["uri"],
        "wit/v0.2/wire.lock"
    );
    assert!(
        related[0]["physicalLocation"]["region"]["startLine"]
            .as_u64()
            .unwrap()
            >= 2,
        "{disc_res}"
    );

    fs::remove_dir_all(&root).unwrap();
}
