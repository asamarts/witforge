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

/// `path` as `check`'s hints spell it: unchanged when only characters no POSIX shell treats
/// specially occur in it, and single-quoted otherwise (a Windows temporary path has `\` and `~`);
/// a path that starts with `-` gets a leading `./`, so it is not read as an option.
fn shell_word(path: &Path) -> String {
    let mut text = path.display().to_string();
    if text.starts_with('-') {
        text.insert_str(0, "./");
    }
    if !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"%+,-./:=@_".contains(&byte))
    {
        text
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
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
    let lock = witforge()
        .arg("lock")
        .arg(&dir)
        .arg("--first")
        .output()
        .unwrap();
    assert!(
        lock.status.success(),
        "lock failed: {}",
        String::from_utf8_lossy(&lock.stderr)
    );
    assert!(dir.join("wire.lock").is_file());
    let relock = witforge().arg("lock").arg(&dir).output().unwrap();
    assert!(relock.status.success(), "{relock:?}");
    assert!(relock.stdout.is_empty(), "{relock:?}");

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
    let first = witforge()
        .arg("lock")
        .arg(&dir)
        .arg("--first")
        .output()
        .unwrap();
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
    assert_eq!(second.status.code(), Some(1), "{second:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), changed);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_reports_an_unlocked_member() {
    let dir = temp_contract("unlocked");
    fs::write(dir.join("wire.lock"), format!("{HEADER}\n")).unwrap();
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
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("copy"),
        "{output:?}"
    );
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
    assert_eq!(lock.status.code(), Some(1), "{lock:?}");
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

    fs::write(
        out.join("v0_1.rs"),
        "// @generated by witforge; do not edit.\nchanged\n",
    )
    .unwrap();
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

    fs::write(
        out.join("v0.rs"),
        "// @generated by witforge; do not edit.\nchanged\n",
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

    fs::write(
        out.join("v0.proto"),
        "// @generated by witforge; do not edit.\nchanged\n",
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

#[test]
fn released_refusal_resolves_the_version_directory() {
    let root = versions("released-resolve");
    let v01 = root.join("wit/v0.1");
    let v02 = root.join("wit/v0.2");
    let v01_lock = fs::read(v01.join("wire.lock")).unwrap();
    let v02_lock = fs::read(v02.join("wire.lock")).unwrap();

    let unreleased = witforge()
        .current_dir(&v02)
        .args(["lock", "."])
        .output()
        .unwrap();
    assert!(unreleased.status.success(), "{unreleased:?}");
    assert_eq!(fs::read(v02.join("wire.lock")).unwrap(), v02_lock);

    for (cwd, dir) in [(&v01, "."), (&v02, "../v0.1"), (&v02, "../v0.2/../v0.1")] {
        let released = witforge()
            .current_dir(cwd)
            .args(["lock", dir])
            .output()
            .unwrap();
        assert_eq!(released.status.code(), Some(2), "{dir}: {released:?}");
        assert!(
            String::from_utf8_lossy(&released.stderr).contains("is released"),
            "{dir}: {released:?}"
        );
    }
    assert_eq!(fs::read(v01.join("wire.lock")).unwrap(), v01_lock);
    fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn lock_and_check_refuse_a_symlink_to_a_version_directory() {
    let root = versions("released-symlink");
    let v01 = root.join("wit/v0.1");
    let before = fs::read(v01.join("wire.lock")).unwrap();
    std::os::unix::fs::symlink("v0.1", root.join("wit/alias")).unwrap();
    for command in ["lock", "check"] {
        for spelling in ["wit/alias", "wit/alias/", "wit/alias/."] {
            let output = witforge()
                .arg(command)
                .arg(root.join(spelling))
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(2),
                "{command} {spelling}: {output:?}"
            );
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("is a symlink"),
                "{command} {spelling}: {output:?}"
            );
        }
    }
    assert_eq!(fs::read(v01.join("wire.lock")).unwrap(), before);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn every_command_validates_the_released_manifest() {
    let root = versions("released-flag");
    let wit = root.join("wit");
    let out = root.join("out");
    let missing = root.join("missing-released");
    let malformed = root.join("malformed-released");
    fs::write(&malformed, "v0 1\n").unwrap();
    let command = |name: &str| {
        let mut witforge = witforge();
        witforge.arg(name);
        match name {
            "check" | "lock" => {
                witforge.arg(wit.join("v0.2"));
            }
            _ => {
                witforge.arg(&wit).arg("--out").arg(&out);
            }
        }
        if name == "glue" {
            witforge.args([
                "--plain-root",
                "crate::plain",
                "--proto-root",
                "crate::proto",
            ]);
        }
        witforge
    };
    for name in ["check", "lock", "rust", "proto", "glue"] {
        let output = command(name)
            .arg("--released")
            .arg(&missing)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{name} missing: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("failed to read released manifest"),
            "{name} missing: {output:?}"
        );

        let output = command(name)
            .arg("--released")
            .arg(&malformed)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "{name} malformed: {output:?}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("released entry `v0 1`"),
            "{name} malformed: {output:?}"
        );
    }

    // The default manifest is validated too: `<WIT_DIR>/../released` and `<WIT_ROOT>/released`.
    fs::write(wit.join("released"), "v0.1\nv0.1\n").unwrap();
    for name in ["check", "lock", "rust", "proto", "glue"] {
        let output = command(name).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{name} default: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("duplicate released entry"),
            "{name} default: {output:?}"
        );
    }
    assert!(!out.exists());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn check_reports_only_the_subset_violation_when_a_lockfile_exists() {
    let dir = temp_contract("violation-with-lock");
    let lock = witforge()
        .arg("lock")
        .arg(&dir)
        .arg("--first")
        .output()
        .unwrap();
    assert!(lock.status.success(), "{lock:?}");
    let source = fs::read_to_string(dir.join("contract.wit")).unwrap();
    let changed = source.replace("start: u64,", "start: char,");
    assert_ne!(changed, source);
    fs::write(dir.join("contract.wit"), changed).unwrap();

    let check = witforge().arg("check").arg(&dir).output().unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    let stderr = String::from_utf8_lossy(&check.stderr);
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 1, "{stderr}");
    assert!(lines[0].starts_with("error: "), "{stderr}");
    assert!(lines[0].contains("char"), "{stderr}");
    assert!(!stderr.contains("witforge lock"), "{stderr}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn lock_exits_1_for_a_subset_violation_without_writing() {
    let dir = temp_contract("exit-lock-violation");
    let first = witforge()
        .arg("lock")
        .arg(&dir)
        .arg("--first")
        .output()
        .unwrap();
    assert!(first.status.success(), "{first:?}");
    let before = fs::read(dir.join("wire.lock")).unwrap();
    let source = fs::read_to_string(dir.join("contract.wit")).unwrap();
    fs::write(
        dir.join("contract.wit"),
        source.replace("start: u64,", "start: char,"),
    )
    .unwrap();
    let lock = witforge().arg("lock").arg(&dir).output().unwrap();
    assert_eq!(lock.status.code(), Some(1), "{lock:?}");
    assert!(
        String::from_utf8_lossy(&lock.stderr).contains("subset violation"),
        "{lock:?}"
    );
    assert_eq!(fs::read(dir.join("wire.lock")).unwrap(), before);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn generators_exit_1_for_contract_violations_and_2_for_parse_errors() {
    let base = versions("exit-generators");
    let root = base.join("wit");
    let source = root.join("v0.2/contract.wit");
    let original = fs::read_to_string(&source).unwrap();
    let run = |command: &str, check: bool| {
        let out = base.join(format!("{command}-out"));
        fs::create_dir_all(&out).unwrap();
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
    for (contract, code, message) in [
        (
            original.replace("renamed: string,", "renamed: string,\n        extra: u8,"),
            1,
            "lockfile is stale",
        ),
        (
            original.replace("name: string,", "name: char,"),
            1,
            "`char` is not supported",
        ),
        (
            original.replace("record user {", "record user {{"),
            2,
            "failed to load the WIT package",
        ),
    ] {
        assert_ne!(contract, original);
        fs::write(&source, &contract).unwrap();
        for command in ["rust", "proto", "glue"] {
            for check in [false, true] {
                let output = run(command, check);
                assert_eq!(
                    output.status.code(),
                    Some(code),
                    "{command} check={check} {message}: {output:?}"
                );
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains(message),
                    "{command} check={check}: {output:?}"
                );
            }
        }
    }
    fs::remove_dir_all(&base).unwrap();
}

/// Add `wit/v0.3` to a versions-fixture copy: the v0.2 contract at version 0.3.0 with one more
/// field, and no lockfile.
fn add_v0_3(root: &Path) -> PathBuf {
    let v03 = root.join("wit/v0.3");
    fs::create_dir_all(&v03).unwrap();
    let source = fs::read_to_string(root.join("wit/v0.2/contract.wit")).unwrap();
    let source = source
        .replace("@0.2.0", "@0.3.0")
        .replace("renamed: string,", "renamed: string,\n        added: u64,");
    fs::write(v03.join("contract.wit"), source).unwrap();
    v03
}

#[test]
fn lock_and_check_refuse_a_missing_lockfile_without_first() {
    let root = versions("missing-lock");
    let v03 = add_v0_3(&root);
    for command in ["lock", "check"] {
        let output = witforge().arg(command).arg(&v03).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{command}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("does not exist"), "{command}: {stderr}");
        assert!(stderr.contains("--first"), "{command}: {stderr}");
        assert!(stderr.contains("--against"), "{command}: {stderr}");
        assert!(stderr.contains("copy"), "{command}: {stderr}");
        assert!(!v03.join("wire.lock").exists(), "{command}");
    }
    let previous = root.join("wit/v0.2/wire.lock");
    for command in ["lock", "check"] {
        let output = witforge()
            .arg(command)
            .arg(&v03)
            .arg("--against")
            .arg(&previous)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{command}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("does not exist"), "{command}: {stderr}");
        assert!(
            stderr.contains(&format!("copy {}", previous.display())),
            "{command}: {stderr}"
        );
        assert!(!v03.join("wire.lock").exists(), "{command}");
    }

    // The documented workflow for a new minor: copy the previous lockfile, then `--against`.
    fs::copy(&previous, v03.join("wire.lock")).unwrap();
    let lock = witforge()
        .arg("lock")
        .arg(&v03)
        .arg("--against")
        .arg(&previous)
        .output()
        .unwrap();
    assert!(lock.status.success(), "{lock:?}");
    assert_eq!(
        String::from_utf8_lossy(&lock.stdout).trim(),
        "added    example:evolve/types/user.added = 7"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn generators_refuse_a_new_minor_that_does_not_continue_the_previous_lockfile() {
    let base = versions("continuity");
    let root = base.join("wit");
    let v03 = add_v0_3(&base);
    // A fresh lockfile for the new minor renumbers every member.
    let fresh = witforge()
        .arg("lock")
        .arg(&v03)
        .arg("--first")
        .output()
        .unwrap();
    assert!(fresh.status.success(), "{fresh:?}");
    let check = witforge().arg("check").arg(&v03).output().unwrap();
    assert!(check.status.success(), "{check:?}");

    let run = |command: &str, check: bool| {
        let out = base.join(format!("{command}-out"));
        fs::create_dir_all(&out).unwrap();
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
    for command in ["rust", "proto", "glue"] {
        for check in [false, true] {
            let output = run(command, check);
            assert_eq!(
                output.status.code(),
                Some(1),
                "{command} {check}: {output:?}"
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("does not continue"), "{command}: {stderr}");
            assert!(stderr.contains("v0.2"), "{command}: {stderr}");
            assert!(stderr.contains("--against"), "{command}: {stderr}");
        }
        assert_eq!(
            fs::read_dir(base.join(format!("{command}-out")))
                .unwrap()
                .count(),
            0,
            "{command} wrote output"
        );
    }

    // Starting the minor from a copy of the previous lockfile restores continuity.
    fs::copy(root.join("v0.2/wire.lock"), v03.join("wire.lock")).unwrap();
    let lock = witforge()
        .arg("lock")
        .arg(&v03)
        .arg("--against")
        .arg(root.join("v0.2/wire.lock"))
        .output()
        .unwrap();
    assert!(lock.status.success(), "{lock:?}");
    for command in ["rust", "proto", "glue"] {
        for check in [false, true] {
            let output = run(command, check);
            assert!(output.status.success(), "{command} {check}: {output:?}");
        }
    }
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn lock_refuses_a_lockfile_path_inside_a_released_directory() {
    let root = versions("released-lock-path");
    let released = root.join("wit/v0.1/wire.lock");
    let before = fs::read(&released).unwrap();
    let output = witforge()
        .arg("lock")
        .arg(root.join("wit/v0.2"))
        .arg("--lock")
        .arg(&released)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("is in the released directory"),
        "{output:?}"
    );
    assert_eq!(fs::read(&released).unwrap(), before);
    fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn every_command_refuses_a_symlinked_version_directory() {
    // `wit/v0.1` is a symlink into a store that has no manifest; `wit/released` lists it.
    let root = versions("released-symlinked-version");
    let store = root.join("store");
    fs::create_dir_all(&store).unwrap();
    fs::rename(root.join("wit/v0.1"), store.join("v0.1")).unwrap();
    std::os::unix::fs::symlink(store.join("v0.1"), root.join("wit/v0.1")).unwrap();
    let released = store.join("v0.1");
    let listing = |dir: &Path| {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        names
    };
    let before = listing(&released);
    let lock_before = fs::read(released.join("wire.lock")).unwrap();
    let out = root.join("out");
    let runs: Vec<Vec<std::ffi::OsString>> = vec![
        vec!["lock".into(), root.join("wit/v0.1").into()],
        vec!["check".into(), root.join("wit/v0.1").into()],
        vec![
            "rust".into(),
            root.join("wit").into(),
            "--out".into(),
            out.clone().into(),
        ],
        vec![
            "proto".into(),
            root.join("wit").into(),
            "--out".into(),
            out.clone().into(),
        ],
        vec![
            "glue".into(),
            root.join("wit").into(),
            "--out".into(),
            out.clone().into(),
            "--plain-root".into(),
            "crate::plain".into(),
            "--proto-root".into(),
            "crate::proto".into(),
        ],
        // The link's target is outside the WIT root, so only the refusal of the symlinked
        // version keeps generation from writing into the released directory.
        vec![
            "rust".into(),
            root.join("wit").into(),
            "--out".into(),
            root.join("wit/v0.1").into(),
        ],
    ];
    for args in runs {
        let output = witforge().args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("is a symlink"),
            "{args:?}: {output:?}"
        );
    }
    assert_eq!(listing(&released), before);
    assert_eq!(fs::read(released.join("wire.lock")).unwrap(), lock_before);
    assert!(!out.join("v0_1.rs").exists());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn lock_refuses_a_released_lockfile_whatever_the_manifest_options_say() {
    let root = versions("released-lock-anywhere");
    let released = root.join("wit/v0.1/wire.lock");
    let before = fs::read(&released).unwrap();
    // Another WIT root without a manifest, whose v0.2 the released lockfile does not match.
    let other = root.join("other");
    copy_tree(&root.join("wit/v0.2"), &other.join("v0.2"));
    let empty = root.join("empty-released");
    fs::write(&empty, "# nothing\n").unwrap();
    let runs: Vec<Vec<std::ffi::OsString>> = vec![
        vec![
            "lock".into(),
            other.join("v0.2").into(),
            "--lock".into(),
            released.clone().into(),
        ],
        vec![
            "lock".into(),
            other.join("v0.2").into(),
            "--lock".into(),
            released.clone().into(),
            "--released".into(),
            empty.clone().into(),
        ],
        vec![
            "lock".into(),
            root.join("wit/v0.1").into(),
            "--released".into(),
            empty.clone().into(),
        ],
    ];
    for args in runs {
        let output = witforge().args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("released"), "{args:?}: {stderr}");
        assert_eq!(fs::read(&released).unwrap(), before, "{args:?}");
    }
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn lock_refuses_a_lockfile_in_a_subdirectory_of_a_released_directory() {
    let root = versions("released-lock-subdirectory");
    // Another WIT root whose manifest releases its own v0.1.
    let other = root.join("other");
    copy_tree(&root.join("wit/v0.1"), &other.join("v0.1"));
    fs::write(other.join("released"), "v0.1\n").unwrap();
    for (released, nested) in [
        (root.join("wit/v0.1"), "sub"),
        (other.join("v0.1"), "sub"),
        (other.join("v0.1"), "a/b"),
    ] {
        let directory = released.join(nested);
        fs::create_dir_all(&directory).unwrap();
        let lock = directory.join("wire.lock");
        let output = witforge()
            .arg("lock")
            .arg(root.join("wit/v0.2"))
            .arg("--first")
            .arg("--lock")
            .arg(&lock)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{lock:?}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("is in the released directory"),
            "{lock:?}: {output:?}"
        );
        assert!(!lock.exists(), "{lock:?}");
    }
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn check_and_lock_refuse_a_lockfile_of_another_package_even_without_members() {
    let dir = std::env::temp_dir().join(format!("witforge-cli-foreign-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    // A contract with no lockable members, and a lockfile whose only entry is another
    // package's, already reserved.
    fs::write(
        dir.join("contract.wit"),
        "package example:empty@0.1.0;\n\ninterface api {}\n",
    )
    .unwrap();
    let lock = format!("{HEADER}\nother:pkg/types/thing.field = 1 field u32 reserved\n");
    fs::write(dir.join("wire.lock"), &lock).unwrap();
    for command in ["check", "lock"] {
        let output = witforge().arg(command).arg(&dir).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{command}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("not the contract's package"),
            "{command}: {output:?}"
        );
    }
    assert_eq!(fs::read_to_string(dir.join("wire.lock")).unwrap(), lock);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_requires_the_canonical_lockfile_that_lock_writes() {
    let dir = temp_contract("canonical");
    let lock = witforge()
        .arg("lock")
        .arg(&dir)
        .arg("--first")
        .output()
        .unwrap();
    assert!(lock.status.success(), "{lock:?}");
    let canonical = fs::read_to_string(dir.join("wire.lock")).unwrap();
    let mut lines: Vec<&str> = canonical.lines().collect();
    let body = lines.split_off(1);
    let mut reversed = body.clone();
    reversed.reverse();
    let edits = [
        ("no final newline", canonical.trim_end().to_string()),
        ("a comment", format!("{canonical}# note\n")),
        ("a blank line", format!("{canonical}\n")),
        ("a tab", canonical.replacen(" = ", "\t= ", 1)),
        (
            "another order",
            format!("{}\n{}\n", lines[0], reversed.join("\n")),
        ),
    ];
    for (what, text) in edits {
        assert_ne!(text, canonical, "{what}");
        fs::write(dir.join("wire.lock"), &text).unwrap();
        let output = witforge().arg("check").arg(&dir).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{what}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("is not in the form `witforge lock` writes"),
            "{what}: {stderr}"
        );
        assert!(
            stderr.contains(&format!("run `witforge lock {}`", shell_word(&dir))),
            "{what}: {stderr}"
        );
        let fixed = witforge().arg("lock").arg(&dir).output().unwrap();
        assert!(fixed.status.success(), "{what}: {fixed:?}");
        assert_eq!(
            fs::read_to_string(dir.join("wire.lock")).unwrap(),
            canonical
        );
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_reads_crlf_line_endings_as_lf() {
    // A checkout with `core.autocrlf=true` (the Git for Windows default) writes CRLF.
    let dir = temp_contract("crlf");
    let lock = witforge()
        .arg("lock")
        .arg(&dir)
        .arg("--first")
        .output()
        .unwrap();
    assert!(lock.status.success(), "{lock:?}");
    let canonical = fs::read_to_string(dir.join("wire.lock")).unwrap();
    let crlf = canonical.replace('\n', "\r\n");
    let mixed = canonical.replacen('\n', "\r\n", 1);
    for (what, text) in [("CRLF", &crlf), ("CRLF on one line", &mixed)] {
        assert_ne!(text, &canonical, "{what}");
        fs::write(dir.join("wire.lock"), text).unwrap();
        let output = witforge().arg("check").arg(&dir).output().unwrap();
        assert!(output.status.success(), "{what}: {output:?}");
        assert_eq!(fs::read_to_string(dir.join("wire.lock")).unwrap(), *text);
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_accepts_a_released_lockfile_that_lock_would_rewrite() {
    // `lock` refuses a released directory, so `check` cannot require the form only `lock`
    // writes; the freeze check guards the released file as it was committed.
    let root = versions("released-form");
    let v01 = root.join("wit/v0.1");
    let canonical = fs::read_to_string(v01.join("wire.lock")).unwrap();
    let edits = [
        ("CRLF", canonical.replace('\n', "\r\n")),
        ("a comment", format!("{canonical}# released 2026-10-28\n")),
        ("a blank line", format!("{canonical}\n")),
    ];
    for (what, text) in edits {
        assert_ne!(text, canonical, "{what}");
        fs::write(v01.join("wire.lock"), &text).unwrap();
        let output = witforge().arg("check").arg(&v01).output().unwrap();
        assert!(output.status.success(), "{what}: {output:?}");
        // Generation reads the same file.
        let out = root.join("out");
        let rust = witforge()
            .arg("rust")
            .arg(root.join("wit"))
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap();
        assert!(rust.status.success(), "{what}: {rust:?}");
    }
    // An unreleased version must still be in canonical form, and `lock` fixes it.
    let v02 = root.join("wit/v0.2");
    let unreleased = fs::read_to_string(v02.join("wire.lock")).unwrap();
    fs::write(v02.join("wire.lock"), format!("{unreleased}# a note\n")).unwrap();
    let output = witforge().arg("check").arg(&v02).output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("is not in the form `witforge lock` writes"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("run `witforge lock {}`", shell_word(&v02))),
        "{stderr}"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn check_tells_a_released_version_to_restore_rather_than_lock() {
    let root = versions("released-hint");
    let v01 = root.join("wit/v0.1");
    let lock = fs::read_to_string(v01.join("wire.lock")).unwrap();
    // Drop one entry from the released lockfile, so the member it numbered is unlocked.
    let mut lines: Vec<&str> = lock.lines().collect();
    lines.remove(1);
    fs::write(v01.join("wire.lock"), format!("{}\n", lines.join("\n"))).unwrap();
    let output = witforge().arg("check").arg(&v01).output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is released"), "{stderr}");
    assert!(stderr.contains("restore its released WIT"), "{stderr}");
    assert!(!stderr.contains("run `witforge lock"), "{stderr}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn check_names_the_lock_command_for_its_mode() {
    let dir = temp_contract("check-hint");
    let output = witforge()
        .arg("check")
        .arg(&dir)
        .arg("--first")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("run `witforge lock {} --first`", shell_word(&dir))),
        "{stderr}"
    );
    fs::remove_dir_all(&dir).unwrap();

    // A path a shell would split or expand is quoted, so the hint can be pasted as it is.
    let spaced = std::env::temp_dir().join(format!("witforge cli it's {}", std::process::id()));
    let _ = fs::remove_dir_all(&spaced);
    fs::create_dir_all(&spaced).unwrap();
    fs::copy(fixture(), spaced.join("contract.wit")).unwrap();
    let output = witforge()
        .arg("check")
        .arg(&spaced)
        .arg("--first")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let quoted = shell_word(&spaced);
    assert!(quoted.starts_with('\''), "{quoted}");
    assert!(
        stderr.contains(&format!("run `witforge lock {quoted} --first`")),
        "{stderr}"
    );
    fs::remove_dir_all(&spaced).unwrap();
}

/// Run a hint's `witforge ...` command through `sh`, with the binary under test, in `cwd`.
#[cfg(unix)]
fn run_hint(command: &str, cwd: &Path) -> std::process::Output {
    let binary = Path::new(env!("CARGO_BIN_EXE_witforge"));
    let rest = command.strip_prefix("witforge ").unwrap();
    Command::new("sh")
        .arg("-c")
        .arg(format!("{} {rest}", shell_word(binary)))
        .current_dir(cwd)
        .output()
        .unwrap()
}

#[test]
fn missing_lockfile_hint_is_quoted_and_keeps_the_options() {
    let base = std::env::temp_dir().join(format!("witforge cli it's here {}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let dir = base.join("contract");
    fs::create_dir_all(&dir).unwrap();
    fs::copy(fixture(), dir.join("contract.wit")).unwrap();
    let lock = base.join("custom wire.lock");
    let released = base.join("released");
    fs::write(&released, "# nothing released\n").unwrap();
    for command in ["check", "lock"] {
        let output = witforge()
            .arg(command)
            .arg(&dir)
            .arg("--lock")
            .arg(&lock)
            .arg("--released")
            .arg(&released)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{command}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let options = |mode: &str| {
            format!(
                "witforge lock {} --lock {} {mode} --released {}",
                shell_word(&dir),
                shell_word(&lock),
                shell_word(&released)
            )
        };
        assert!(
            stderr.contains(&format!("create it with `{}`", options("--first"))),
            "{command}: {stderr}"
        );
        assert!(
            stderr.contains(&format!(
                "run `{}`",
                options("--against <previous minor's wire.lock>")
            )),
            "{command}: {stderr}"
        );
        assert!(!lock.exists(), "{command}");
    }
    // The hint runs as printed.
    #[cfg(unix)]
    {
        let command = format!(
            "witforge lock {} --lock {} --first --released {}",
            shell_word(&dir),
            shell_word(&lock),
            shell_word(&released)
        );
        let output = run_hint(&command, &base);
        assert!(output.status.success(), "{command}: {output:?}");
        assert!(lock.is_file());
    }
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn hints_spell_a_path_that_starts_with_a_dash_as_a_path() {
    let base = std::env::temp_dir().join(format!("witforge-cli-dash-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let dir = Path::new("-w").join("contract");
    let lock = Path::new("-w").join("custom.lock");
    fs::create_dir_all(base.join(&dir)).unwrap();
    fs::copy(fixture(), base.join(&dir).join("contract.wit")).unwrap();
    assert!(
        shell_word(&dir)
            .trim_start_matches('\'')
            .starts_with("./-w")
    );
    let lock_option = format!("--lock={}", lock.display());

    // The missing-lockfile hint, and `check`'s hint for an unlocked member.
    let output = witforge()
        .current_dir(&base)
        .args(["check", &lock_option, "--"])
        .arg(&dir)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let command = format!(
        "witforge lock {} --lock {} --first",
        shell_word(&dir),
        shell_word(&lock)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("create it with `{command}`")),
        "{stderr}"
    );
    let output = witforge()
        .current_dir(&base)
        .args(["check", "--first", &lock_option, "--"])
        .arg(&dir)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&format!("run `{command}`")), "{stderr}");

    // The hint runs as printed: clap would refuse `-w/contract` as an unknown option.
    #[cfg(unix)]
    {
        let output = run_hint(&command, &base);
        assert!(output.status.success(), "{command}: {output:?}");
        let output = witforge()
            .current_dir(&base)
            .args(["check", &lock_option, "--"])
            .arg(&dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn check_and_lock_refuse_a_version_directory_holding_another_version() {
    let root = versions("directory-version");
    // `v0.3` holds a copy of v0.2's package, `@0.2.0`, as the generators already refuse.
    let v03 = root.join("wit/v0.3");
    copy_tree(&root.join("wit/v0.2"), &v03);
    let lock = fs::read(v03.join("wire.lock")).unwrap();
    for args in [
        &["check"][..],
        &["lock"],
        &["check", "--first"],
        &["lock", "--first"],
    ] {
        let output = witforge().args(args).arg(&v03).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("declares package version 0.2.0, expected 0.3.0"),
            "{args:?}: {output:?}"
        );
        assert_eq!(fs::read(v03.join("wire.lock")).unwrap(), lock, "{args:?}");
    }
    // The name of the directory `.` stands for counts.
    let output = witforge()
        .current_dir(&v03)
        .args(["check", "."])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    // A version spelled with a leading zero is refused, as the generators refuse it.
    let misspelled = root.join("wit/v0.03");
    fs::rename(&v03, &misspelled).unwrap();
    let output = witforge().arg("check").arg(&misspelled).output().unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("must be spelled `v0.3`"),
        "{output:?}"
    );
    // A directory with any other name may hold any version.
    let adhoc = root.join("adhoc");
    fs::rename(&misspelled, &adhoc).unwrap();
    let output = witforge().arg("check").arg(&adhoc).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn generators_refuse_a_lockfile_of_another_package_even_without_members() {
    // A contract with no lockable members has no drift for another package's reserved entry to
    // show up in; only the package check refuses the lockfile.
    let root =
        std::env::temp_dir().join(format!("witforge-cli-foreign-gen-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let wit = root.join("wit");
    fs::create_dir_all(wit.join("v0.1")).unwrap();
    fs::write(
        wit.join("v0.1/contract.wit"),
        "package example:empty@0.1.0;\n\ninterface api {}\n",
    )
    .unwrap();
    fs::write(
        wit.join("v0.1/wire.lock"),
        format!("{HEADER}\nother:pkg/types/thing.field = 1 field u32 reserved\n"),
    )
    .unwrap();
    let out = root.join("out");
    for (command, extra) in [
        ("rust", &[][..]),
        ("proto", &[][..]),
        (
            "glue",
            &[
                "--plain-root",
                "crate::plain",
                "--proto-root",
                "crate::proto",
            ][..],
        ),
    ] {
        let output = witforge()
            .arg(command)
            .arg(&wit)
            .arg("--out")
            .arg(&out)
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{command}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("not the contract's package"),
            "{command}: {output:?}"
        );
    }
    assert!(!out.exists());
    fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn generators_refuse_a_symlinked_version_directory_no_manifest_lists() {
    // `wit/v0.2` is a symlink and `wit/released` lists only v0.1, so the released manifest does
    // not refuse the link first: only the generators' own refusal does.
    let root = versions("unlisted-symlinked-version");
    let store = root.join("store");
    fs::create_dir_all(&store).unwrap();
    fs::rename(root.join("wit/v0.2"), store.join("v0.2")).unwrap();
    std::os::unix::fs::symlink(store.join("v0.2"), root.join("wit/v0.2")).unwrap();
    let out = root.join("out");
    for (command, extra) in [
        ("rust", &[][..]),
        ("proto", &[][..]),
        (
            "glue",
            &[
                "--plain-root",
                "crate::plain",
                "--proto-root",
                "crate::proto",
            ][..],
        ),
    ] {
        for check in [false, true] {
            let mut run = witforge();
            run.arg(command)
                .arg(root.join("wit"))
                .arg("--out")
                .arg(&out);
            run.args(extra);
            if check {
                fs::create_dir_all(&out).unwrap();
                run.arg("--check");
            }
            let output = run.output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(2),
                "{command} {check}: {output:?}"
            );
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("is a symlink"),
                "{command} {check}: {output:?}"
            );
        }
    }
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    fs::remove_dir_all(&root).unwrap();
}
