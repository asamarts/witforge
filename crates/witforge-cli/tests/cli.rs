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

    for args in [
        &["--help"][..],
        &["check", "--help"][..],
        &["proto", "--help"][..],
        &["glue", "--help"][..],
        &["help"][..],
    ] {
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
    assert_eq!(
        stderr.trim(),
        "error: example:evolve/types/user.removed was removed and its name and number 4 stay reserved; give the member a new name"
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
    assert_eq!(
        String::from_utf8_lossy(&lock.stdout).trim(),
        "updated  example:evolve/types/user.new-field"
    );
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
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            "dropped  example:evolve/types/user.old-field = 2",
            "dropped  example:evolve/types/user.removed = 4",
            "dropped  example:evolve/types/user.rename-me = 3",
        ]
    );
    let text = fs::read_to_string(v02.join("wire.lock")).unwrap();
    assert!(!text.contains("old-field"), "{text}");
    assert!(!text.contains("rename-me"), "{text}");
    assert!(!text.contains("removed"), "{text}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn rust_generates_mod_and_version_files_and_checks_drift() {
    let root = versions("rust");
    let out = root
        .parent()
        .unwrap()
        .join(format!("rust-out-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let generate = witforge()
        .arg("rust")
        .arg(root.join("wit"))
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(generate.status.success(), "{generate:?}");
    for name in ["mod.rs", "v0_1.rs", "v0_2.rs"] {
        assert!(out.join(name).is_file(), "{name}");
    }
    let check = witforge()
        .arg("rust")
        .arg(root.join("wit"))
        .arg("--out")
        .arg(&out)
        .arg("--check")
        .output()
        .unwrap();
    assert!(check.status.success(), "{check:?}");

    fs::write(out.join("v0_1.rs"), "changed\n").unwrap();
    let check = witforge()
        .arg("rust")
        .arg(root.join("wit"))
        .arg("--out")
        .arg(&out)
        .arg("--check")
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert!(String::from_utf8_lossy(&check.stderr).contains("changed generated"));

    let inside = root.join("wit/out");
    let output = witforge()
        .arg("rust")
        .arg(root.join("wit"))
        .arg("--out")
        .arg(&inside)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(!inside.exists());
    fs::remove_dir_all(&out).unwrap();
    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn glue_generates_highest_minor_and_checks_drift() {
    let root = versions("glue").join("wit");
    let out = root
        .parent()
        .unwrap()
        .join(format!("glue-out-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let generate = witforge()
        .arg("glue")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .arg("--plain-root")
        .arg("crate::plain")
        .arg("--proto-root")
        .arg("crate::proto")
        .output()
        .unwrap();
    assert!(
        generate.status.success(),
        "glue failed: {}",
        String::from_utf8_lossy(&generate.stderr)
    );
    for name in ["mod.rs", "v0.rs"] {
        assert!(out.join(name).is_file(), "{name}");
    }
    let module = fs::read_to_string(out.join("mod.rs")).unwrap();
    assert!(module.contains("pub mod v0;"), "{module}");
    assert!(!module.contains("v0_1"), "{module}");
    let version = fs::read_to_string(out.join("v0.rs")).unwrap();
    assert!(version.contains("example:evolve@0.2.0"), "{version}");
    assert!(version.contains("crate::plain::v0_2"), "{version}");
    assert!(version.contains("crate::proto::v0"), "{version}");

    let check = witforge()
        .arg("glue")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .arg("--plain-root")
        .arg("crate::plain")
        .arg("--proto-root")
        .arg("crate::proto")
        .arg("--check")
        .output()
        .unwrap();
    assert!(check.status.success(), "{check:?}");

    fs::write(out.join("v0.rs"), "changed\n").unwrap();
    let check = witforge()
        .arg("glue")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .arg("--plain-root")
        .arg("crate::plain")
        .arg("--proto-root")
        .arg("crate::proto")
        .arg("--check")
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert!(String::from_utf8_lossy(&check.stderr).contains("changed generated"));

    fs::remove_file(out.join("v0.rs")).unwrap();
    let check = witforge()
        .arg("glue")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .arg("--plain-root")
        .arg("crate::plain")
        .arg("--proto-root")
        .arg("crate::proto")
        .arg("--check")
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert!(String::from_utf8_lossy(&check.stderr).contains("missing generated"));

    fs::write(
        out.join("stale.rs"),
        "// @generated by witforge; do not edit.\n",
    )
    .unwrap();
    let check = witforge()
        .arg("glue")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .arg("--plain-root")
        .arg("crate::plain")
        .arg("--proto-root")
        .arg("crate::proto")
        .arg("--check")
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert!(String::from_utf8_lossy(&check.stderr).contains("extra generated"));

    let regenerate = witforge()
        .arg("glue")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .arg("--plain-root")
        .arg("crate::plain")
        .arg("--proto-root")
        .arg("crate::proto")
        .output()
        .unwrap();
    assert!(regenerate.status.success(), "{regenerate:?}");
    assert!(!out.join("stale.rs").exists());

    for args in [
        vec![
            "glue",
            root.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--plain-root",
            "crate::plain",
        ],
        vec![
            "glue",
            root.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--plain-root",
            "crate::plain",
            "--proto-root",
            "crate::1proto",
        ],
    ] {
        let output = witforge().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
    }
    let inside = root.join("glue-out");
    let output = witforge()
        .arg("glue")
        .arg(&root)
        .arg("--out")
        .arg(&inside)
        .arg("--plain-root")
        .arg("crate::plain")
        .arg("--proto-root")
        .arg("crate::proto")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(!inside.exists());
    fs::remove_dir_all(&out).unwrap();
    fs::remove_dir_all(
        root.parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(format!("witforge-versions-glue-{}", std::process::id())),
    )
    .unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn proto_generates_highest_minor_and_checks_drift() {
    let root = versions("proto").join("wit");
    let out = root
        .parent()
        .unwrap()
        .join(format!("proto-out-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let generate = witforge()
        .arg("proto")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        generate.status.success(),
        "proto failed: {}",
        String::from_utf8_lossy(&generate.stderr)
    );
    assert!(out.join("v0.proto").is_file());
    let schema = fs::read_to_string(out.join("v0.proto")).unwrap();
    assert!(
        schema.contains("// source: example:evolve@0.2.0"),
        "{schema}"
    );
    assert!(schema.contains("package example.evolve.v0;"), "{schema}");
    assert!(!schema.contains("@0.1.0"), "{schema}");

    let check = witforge()
        .arg("proto")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .arg("--check")
        .output()
        .unwrap();
    assert!(check.status.success(), "{check:?}");

    fs::write(out.join("v0.proto"), "changed\n").unwrap();
    let check = witforge()
        .arg("proto")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .arg("--check")
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert!(String::from_utf8_lossy(&check.stderr).contains("changed generated"));

    fs::remove_file(out.join("v0.proto")).unwrap();
    let check = witforge()
        .arg("proto")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .arg("--check")
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert!(String::from_utf8_lossy(&check.stderr).contains("missing generated"));

    fs::write(
        out.join("stale.proto"),
        "// @generated by witforge; do not edit.\n",
    )
    .unwrap();
    let check = witforge()
        .arg("proto")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .arg("--check")
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert!(String::from_utf8_lossy(&check.stderr).contains("extra generated"));

    let regenerate = witforge()
        .arg("proto")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(regenerate.status.success(), "{regenerate:?}");
    assert!(!out.join("stale.proto").exists());

    let missing_out = witforge().arg("proto").arg(&root).output().unwrap();
    assert_eq!(missing_out.status.code(), Some(2), "{missing_out:?}");

    let inside = root.join("proto-out");
    let output = witforge()
        .arg("proto")
        .arg(&root)
        .arg("--out")
        .arg(&inside)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(!inside.exists());
    fs::remove_dir_all(&out).unwrap();
    fs::remove_dir_all(
        root.parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(format!("witforge-versions-proto-{}", std::process::id())),
    )
    .unwrap();
}

#[test]
fn generators_ignore_a_binary_file_in_the_output_root() {
    let base = versions("binary-output");
    let root = base.join("wit");
    let bytes = [
        0x00, 0x00, 0x00, 0x01, 0x42, 0x75, 0x64, 0x31, 0xff, 0xfe, 0x80,
    ];
    for command in ["rust", "proto", "glue"] {
        let out = base.join(format!("{command}-out"));
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join(".DS_Store"), bytes).unwrap();
        let run = |check: bool| {
            let mut witforge = witforge();
            witforge.arg(command).arg(&root).arg("--out").arg(&out);
            if command == "glue" {
                witforge.args([
                    "--plain-root",
                    "crate::plain",
                    "--proto-root",
                    "crate::proto",
                ]);
            }
            if check {
                witforge.arg("--check");
            }
            witforge.output().unwrap()
        };
        let generate = run(false);
        assert!(generate.status.success(), "{command}: {generate:?}");
        let check = run(true);
        assert!(check.status.success(), "{command} --check: {check:?}");
        assert_eq!(fs::read(out.join(".DS_Store")).unwrap(), bytes);
    }
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn generators_refuse_an_output_root_that_reaches_the_wit_root_through_parent_components() {
    let base = versions("out-parent");
    fs::create_dir_all(base.join("existing")).unwrap();
    for out in [
        "missing/../wit/gen",
        "existing/../wit/gen",
        "existing/../wit/v0.2",
    ] {
        for command in ["rust", "proto", "glue"] {
            let mut witforge = witforge();
            witforge
                .current_dir(&base)
                .args([command, "wit", "--out", out]);
            if command == "glue" {
                witforge.args([
                    "--plain-root",
                    "crate::plain",
                    "--proto-root",
                    "crate::proto",
                ]);
            }
            let output = witforge.output().unwrap();
            assert_eq!(output.status.code(), Some(2), "{command} {out}: {output:?}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("must not be the WIT root"),
                "{command} {out}: {output:?}"
            );
        }
    }
    assert!(!base.join("wit/gen").exists());
    let version_files: Vec<_> = fs::read_dir(base.join("wit/v0.2"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(version_files.len(), 2, "{version_files:?}");
    fs::remove_dir_all(&base).unwrap();
}
