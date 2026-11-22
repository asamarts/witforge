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

fn version_contract(root: &Path, version: &str, package_version: &str) {
    let dir = root.join(version);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("contract.wit"),
        format!(
            "package ex:t@{package_version};\n\
             interface api {{ alpha: func() -> u32; beta: func(); }}\n"
        ),
    )
    .unwrap();
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

/// The lockfile kind token a WIT type kind produces.
fn member_kind(kind: &str) -> &'static str {
    match kind {
        "record" => "field",
        "variant" => "variant-case",
        "enum" => "enum-case",
        "flags" => "flag",
        other => panic!("not a type kind: {other}"),
    }
}

/// A one-type contract whose members are `<prefix>one` and `<prefix>two`.
fn kind_contract(version: &str, kind: &str, prefix: &str) -> String {
    let body = match kind {
        "record" => format!("record thing {{ {prefix}one: u32, {prefix}two: string }}"),
        "variant" => format!("variant thing {{ {prefix}one, {prefix}two }}"),
        "enum" => format!("enum thing {{ {prefix}one, {prefix}two }}"),
        "flags" => format!("flags thing {{ {prefix}one, {prefix}two }}"),
        other => panic!("not a type kind: {other}"),
    };
    format!("package ex:t@{version};\ninterface types {{\n  {body}\n}}\n")
}

/// One lockfile line for `kind`'s member at `index` (which fixes a record field's type).
fn kind_entry(kind: &str, name: &str, index: usize, number: u32, reserved: bool) -> String {
    let token = member_kind(kind);
    let ty = if token == "field" {
        if index == 0 { " u32" } else { " string" }
    } else {
        ""
    };
    format!(
        "ex:t/types/thing.{name} = {number} {token}{ty}{}\n",
        if reserved { " reserved" } else { "" }
    )
}

#[test]
fn respelling_a_byte_list_does_not_change_the_wire() {
    let root = std::env::temp_dir().join(format!("witforge-byte-respell-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let old = root.join("v0.1");
    let new = root.join("v0.2");
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&new).unwrap();
    fs::write(
        old.join("contract.wit"),
        "package example:respelling@0.1.0;\n\
interface api {\n\
  type byte = u8;\n\
  record box { value: list<byte> }\n\
  get: func(input: list<byte>) -> list<u8>;\n\
}\n",
    )
    .unwrap();
    fs::write(
        new.join("contract.wit"),
        "package example:respelling@0.2.0;\n\
interface api {\n\
  record box { value: list<u8> }\n\
  get: func(input: list<u8>) -> list<u8>;\n\
}\n",
    )
    .unwrap();

    let lock = witforge()
        .arg("lock")
        .arg(&old)
        .arg("--first")
        .output()
        .unwrap();
    assert!(
        lock.status.success(),
        "first lock failed: {}",
        String::from_utf8_lossy(&lock.stderr)
    );
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();
    let before = fs::read(new.join("wire.lock")).unwrap();
    let against = witforge()
        .arg("lock")
        .arg(&new)
        .arg("--against")
        .arg(old.join("wire.lock"))
        .output()
        .unwrap();
    assert!(
        against.status.success(),
        "respelling changed the lockfile: {}",
        String::from_utf8_lossy(&against.stderr)
    );
    assert_eq!(fs::read(new.join("wire.lock")).unwrap(), before);

    let check = witforge()
        .arg("check")
        .arg(&new)
        .arg("--against")
        .arg(old.join("wire.lock"))
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "check rejected a respelling: {}",
        String::from_utf8_lossy(&check.stderr)
    );

    let out =
        std::env::temp_dir().join(format!("witforge-byte-respell-out-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let proto = witforge()
        .arg("proto")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        proto.status.success(),
        "proto failed: {}",
        String::from_utf8_lossy(&proto.stderr)
    );
    let schema = fs::read_to_string(out.join("v0.proto")).unwrap();
    assert!(schema.contains("optional bytes value = 1;"), "{schema}");
    assert!(schema.contains("optional bytes input = 1;"), "{schema}");
    assert!(!schema.contains("repeated uint32"), "{schema}");
    fs::remove_dir_all(&out).unwrap();
    fs::remove_dir_all(&root).unwrap();
}

#[test]
#[allow(clippy::too_many_lines)]
fn every_ordered_kind_change_is_refused_by_lock_check_and_every_generator() {
    let base = std::env::temp_dir().join(format!("witforge-kind-pairs-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();

    let kinds = ["record", "variant", "enum", "flags"];
    for from in kinds {
        for to in kinds {
            if from == to {
                continue;
            }
            let pair = format!("{from}-to-{to}");
            let root = base.join(&pair);
            let old = root.join("v0.1");
            let new = root.join("v0.2");
            fs::create_dir_all(&old).unwrap();
            fs::create_dir_all(&new).unwrap();
            fs::write(old.join("contract.wit"), kind_contract("0.1.0", from, "")).unwrap();
            fs::write(
                new.join("contract.wit"),
                kind_contract("0.2.0", to, "fresh-"),
            )
            .unwrap();

            let lock = witforge()
                .arg("lock")
                .arg(&old)
                .arg("--first")
                .output()
                .unwrap();
            assert!(
                lock.status.success(),
                "{pair}: first lock failed: {}",
                String::from_utf8_lossy(&lock.stderr)
            );
            fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();
            let before = fs::read(new.join("wire.lock")).unwrap();

            let from_token = member_kind(from);
            let to_token = member_kind(to);
            let failed_lock = witforge()
                .arg("lock")
                .arg(&new)
                .arg("--against")
                .arg(old.join("wire.lock"))
                .output()
                .unwrap();
            assert_eq!(
                failed_lock.status.code(),
                Some(1),
                "{pair}: lock --against: {failed_lock:?}"
            );
            let stderr = String::from_utf8_lossy(&failed_lock.stderr);
            assert!(stderr.contains("mixes member kinds"), "{pair}: {stderr}");
            assert!(stderr.contains("ex:t/types/thing"), "{pair}: {stderr}");
            assert!(
                stderr.contains(from_token) && stderr.contains(to_token),
                "{pair}: {stderr}"
            );
            assert_eq!(
                fs::read(new.join("wire.lock")).unwrap(),
                before,
                "{pair}: a refused lock wrote its lockfile"
            );

            // The lockfile a pre-0.6.0 `lock --against` left behind: both kinds under one
            // parent, with the old members reserved and the new members numbered after them.
            let mut mixed = format!("{HEADER}\n");
            mixed.push_str(&kind_entry(from, "one", 0, 1, true));
            mixed.push_str(&kind_entry(from, "two", 1, 2, true));
            mixed.push_str(&kind_entry(to, "fresh-one", 0, 3, false));
            mixed.push_str(&kind_entry(to, "fresh-two", 1, 4, false));
            fs::write(new.join("wire.lock"), mixed).unwrap();

            let out = base.join(format!("out-{pair}"));
            let commands: Vec<(&str, Command)> = vec![
                ("check", {
                    let mut command = witforge();
                    command.arg("check").arg(&new);
                    command
                }),
                ("check --against", {
                    let mut command = witforge();
                    command
                        .arg("check")
                        .arg(&new)
                        .arg("--against")
                        .arg(old.join("wire.lock"));
                    command
                }),
                ("rust", {
                    let mut command = witforge();
                    command
                        .arg("rust")
                        .arg(&root)
                        .arg("--out")
                        .arg(out.join("rust"));
                    command
                }),
                ("proto", {
                    let mut command = witforge();
                    command
                        .arg("proto")
                        .arg(&root)
                        .arg("--out")
                        .arg(out.join("proto"));
                    command
                }),
                ("schema", {
                    let mut command = witforge();
                    command
                        .arg("schema")
                        .arg(&root)
                        .arg("--out")
                        .arg(out.join("schema"));
                    command
                }),
                ("glue", {
                    let mut command = witforge();
                    command
                        .arg("glue")
                        .arg(&root)
                        .arg("--out")
                        .arg(out.join("glue"))
                        .args(["--plain-root", "crate::plain"])
                        .args(["--proto-root", "crate::proto"]);
                    command
                }),
            ];
            for (name, mut command) in commands {
                let output = command.output().unwrap();
                assert_eq!(output.status.code(), Some(1), "{pair}: {name}: {output:?}");
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(
                    stderr.contains("mixes member kinds"),
                    "{pair}: {name}: {stderr}"
                );
                assert!(
                    stderr.contains("ex:t/types/thing"),
                    "{pair}: {name}: {stderr}"
                );
            }
        }
    }
    fs::remove_dir_all(&base).unwrap();
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
    assert_eq!(relock.stdout.len(), 0, "{relock:?}");

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
fn a_missing_order_section_fails_check_in_every_mode_and_generators() {
    let root = versions("order-section");
    let v01 = root.join("wit/v0.1");
    let v02 = root.join("wit/v0.2");
    let contract = fs::read_to_string(v02.join("contract.wit")).unwrap();
    let reordered = contract.replace(
        "        name: string,\n        new-field: option<string>,",
        "        new-field: option<string>,\n        name: string,",
    );
    assert_ne!(contract, reordered);
    fs::write(v02.join("contract.wit"), reordered).unwrap();
    let lock = fs::read_to_string(v02.join("wire.lock")).unwrap();
    let cut = lock.find("# witforge order v1").unwrap();
    fs::write(v02.join("wire.lock"), &lock[..cut]).unwrap();

    for args in [
        &["check", v02.to_str().unwrap()][..],
        &["check", v02.to_str().unwrap(), "--first"],
        &[
            "check",
            v02.to_str().unwrap(),
            "--against",
            v01.join("wire.lock").to_str().unwrap(),
        ],
    ] {
        let output = witforge().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("order"),
            "{args:?}: {output:?}"
        );
    }

    let out = root
        .parent()
        .unwrap()
        .join(format!("witforge-order-section-out-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    for command in ["rust", "proto", "schema", "glue"] {
        let mut run = witforge();
        run.arg(command)
            .arg(root.join("wit"))
            .arg("--out")
            .arg(&out);
        if command == "glue" {
            run.args([
                "--plain-root",
                "crate::plain",
                "--proto-root",
                "crate::proto",
            ]);
        }
        let output = run.output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{command}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("order"),
            "{command}: {output:?}"
        );
    }
    let _ = fs::remove_dir_all(&out);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn first_is_refused_behind_a_same_major_predecessor() {
    let root =
        std::env::temp_dir().join(format!("witforge-first-predecessor-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    version_contract(&root, "v0.1", "0.1.0");
    version_contract(&root, "v0.2", "0.2.0");
    let first = witforge()
        .args(["lock", root.join("v0.1").to_str().unwrap(), "--first"])
        .output()
        .unwrap();
    assert!(first.status.success(), "{first:?}");
    // Re-running `--first` on the lowest minor while it is still a draft stays allowed.
    let again = witforge()
        .args(["lock", root.join("v0.1").to_str().unwrap(), "--first"])
        .output()
        .unwrap();
    assert!(again.status.success(), "{again:?}");

    for verb in ["lock", "check"] {
        let output = witforge()
            .arg(verb)
            .arg(root.join("v0.2"))
            .arg("--first")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{verb}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("v0.1"), "{verb}: {stderr}");
        assert!(stderr.contains("--against"), "{verb}: {stderr}");
    }

    // A new major and a root that starts at a later minor are both firsts.
    version_contract(&root, "v1.0", "1.0.0");
    let new_major = witforge()
        .args(["lock", root.join("v1.0").to_str().unwrap(), "--first"])
        .output()
        .unwrap();
    assert!(new_major.status.success(), "{new_major:?}");
    let _ = fs::remove_dir_all(&root);

    let starts_later = std::env::temp_dir().join(format!(
        "witforge-first-starts-later-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&starts_later);
    version_contract(&starts_later, "v0.3", "0.3.0");
    let output = witforge()
        .args([
            "lock",
            starts_later.join("v0.3").to_str().unwrap(),
            "--first",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    fs::remove_dir_all(starts_later).unwrap();
}

#[test]
fn against_baseline_must_be_the_sibling_major_and_immediate_predecessor() {
    let root = std::env::temp_dir().join(format!("witforge-against-paths-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for (version, package_version) in [
        ("v0.1", "0.1.0"),
        ("v0.2", "0.2.0"),
        ("v0.3", "0.3.0"),
        ("v1.0", "1.0.0"),
    ] {
        version_contract(&root, version, package_version);
    }
    let first = witforge()
        .args(["lock", root.join("v0.1").to_str().unwrap(), "--first"])
        .output()
        .unwrap();
    assert!(first.status.success(), "{first:?}");
    fs::copy(root.join("v0.1/wire.lock"), root.join("v0.2/wire.lock")).unwrap();
    let second = witforge()
        .args([
            "lock",
            root.join("v0.2").to_str().unwrap(),
            "--against",
            root.join("v0.1/wire.lock").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(second.status.success(), "{second:?}");
    fs::copy(root.join("v0.2/wire.lock"), root.join("v0.3/wire.lock")).unwrap();
    let new_major = witforge()
        .args(["lock", root.join("v1.0").to_str().unwrap(), "--first"])
        .output()
        .unwrap();
    assert!(new_major.status.success(), "{new_major:?}");

    let skipped = witforge()
        .args([
            "check",
            root.join("v0.3").to_str().unwrap(),
            "--against",
            root.join("v0.1/wire.lock").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(skipped.status.code(), Some(1), "{skipped:?}");
    let stderr = String::from_utf8_lossy(&skipped.stderr);
    assert!(stderr.contains("v0.2"), "{stderr}");
    assert!(stderr.contains("--against"), "{stderr}");

    for verb in ["check", "lock"] {
        let cross_major = witforge()
            .arg(verb)
            .arg(root.join("v1.0"))
            .arg("--against")
            .arg(root.join("v0.2/wire.lock"))
            .output()
            .unwrap();
        assert_eq!(
            cross_major.status.code(),
            Some(1),
            "{verb}: {cross_major:?}"
        );
        let stderr = String::from_utf8_lossy(&cross_major.stderr);
        assert!(stderr.contains("--first"), "{verb}: {stderr}");
    }

    // A baseline outside a version directory is accepted as given.
    let custom = root.join("baseline.lock");
    fs::copy(root.join("v0.1/wire.lock"), &custom).unwrap();
    let accepted = witforge()
        .args([
            "check",
            root.join("v0.2").to_str().unwrap(),
            "--against",
            custom.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(accepted.status.success(), "{accepted:?}");
    fs::remove_dir_all(&root).unwrap();
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
fn check_and_lock_refuse_a_reorder_of_common_members() {
    let root = versions("order-reorder");
    let v02 = root.join("wit/v0.2");
    let v03 = root.join("wit/v0.3");
    let source = fs::read_to_string(v02.join("contract.wit")).unwrap();
    let reordered = source.replace(
        "        name: string,\n        new-field: option<string>,",
        "        new-field: option<string>,\n        name: string,",
    );
    assert_ne!(source, reordered);
    fs::create_dir_all(&v03).unwrap();
    fs::write(
        v03.join("contract.wit"),
        reordered.replace("@0.2.0", "@0.3.0"),
    )
    .unwrap();
    fs::copy(v02.join("wire.lock"), v03.join("wire.lock")).unwrap();
    let before = fs::read(v03.join("wire.lock")).unwrap();

    for command in ["check", "lock"] {
        let output = witforge()
            .arg(command)
            .arg(&v03)
            .arg("--against")
            .arg(v02.join("wire.lock"))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{command}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("declaration order"), "{command}: {stderr}");
        assert!(stderr.contains("types/user"), "{command}: {stderr}");
        assert_eq!(
            fs::read(v03.join("wire.lock")).unwrap(),
            before,
            "{command}"
        );
    }
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
    let default_check = witforge().arg("check").arg(&v02).output().unwrap();
    assert_eq!(default_check.status.code(), Some(1), "{default_check:?}");
    assert!(
        String::from_utf8_lossy(&default_check.stderr).contains("use `--against`"),
        "{default_check:?}"
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
        text.contains("example:evolve/types/user.new-field = 5 field option<u16>"),
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
    // `--first` is refused in v0.2 beside v0.1; develop the same draft in an ad-hoc directory
    // to exercise the mode itself.
    let draft = root.join("draft");
    copy_tree(&v02, &draft);
    let lock = witforge()
        .arg("lock")
        .arg(&draft)
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
    let text = fs::read_to_string(draft.join("wire.lock")).unwrap();
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

#[test]
fn schema_generates_one_file_per_version_and_checks_drift() {
    let root = versions("schema");
    let out = root.join("schema");
    let generate = witforge()
        .arg("schema")
        .arg(root.join("wit"))
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(generate.status.success(), "{generate:?}");
    for version in ["v0_1.schema.json", "v0_2.schema.json"] {
        assert!(out.join(version).is_file(), "{version}");
    }
    let check = witforge()
        .arg("schema")
        .arg(root.join("wit"))
        .arg("--out")
        .arg(&out)
        .arg("--check")
        .output()
        .unwrap();
    assert!(check.status.success(), "{check:?}");

    let path = out.join("v0_1.schema.json");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push(' ');
    fs::write(&path, text).unwrap();
    let check = witforge()
        .arg("schema")
        .arg(root.join("wit"))
        .arg("--out")
        .arg(&out)
        .arg("--check")
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert!(
        String::from_utf8_lossy(&check.stderr).contains("v0_1.schema.json"),
        "{check:?}"
    );
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
fn check_refuses_a_symlink_inside_a_released_directory() {
    let root = versions("released-lock-symlink");
    let released = root.join("wit/v0.1");
    let store = root.join("store");
    fs::create_dir_all(&store).unwrap();
    fs::copy(released.join("wire.lock"), store.join("wire.lock")).unwrap();
    fs::remove_file(released.join("wire.lock")).unwrap();
    std::os::unix::fs::symlink(store.join("wire.lock"), released.join("wire.lock")).unwrap();

    let output = witforge()
        .args(["check", &released.display().to_string()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("contains symlink"), "{stderr}");

    // The walk is recursive: a symlinked dependency package is caught too.
    fs::remove_file(released.join("wire.lock")).unwrap();
    fs::copy(store.join("wire.lock"), released.join("wire.lock")).unwrap();
    fs::create_dir_all(released.join("deps")).unwrap();
    std::os::unix::fs::symlink(&store, released.join("deps/shared")).unwrap();
    let output = witforge()
        .args(["check", &released.display().to_string()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("deps/shared"), "{stderr}");

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
            original.replace(
                "new-field: option<string>,",
                "new-field: option<string>,\n        extra: u8,",
            ),
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
        for command in ["rust", "proto", "schema", "glue"] {
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
    let source = source.replace("@0.2.0", "@0.3.0").replace(
        "new-field: option<string>,",
        "new-field: option<string>,\n        added: option<u64>,",
    );
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
    // A fresh lockfile for the new minor renumbers every member. `--first` is refused beside a
    // predecessor, so generate the same draft in an ad-hoc directory and copy its lockfile.
    let draft = base.join("draft");
    copy_tree(&v03, &draft);
    let fresh = witforge()
        .arg("lock")
        .arg(&draft)
        .arg("--first")
        .output()
        .unwrap();
    assert!(fresh.status.success(), "{fresh:?}");
    fs::copy(draft.join("wire.lock"), v03.join("wire.lock")).unwrap();
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
            stderr.contains("with --against pointing at the previous minor's wire.lock"),
            "{command}: {stderr}"
        );
        assert!(
            !stderr.contains("--against <"),
            "must not emit an unrunnable placeholder inside a command: {command}: {stderr}"
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

#[test]
fn a_draft_removal_of_a_released_member_can_be_restored() {
    let root = std::env::temp_dir().join(format!("witforge-draft-restore-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let v01 = root.join("v0.1");
    let v02 = root.join("v0.2");
    fs::create_dir_all(&v01).unwrap();
    fs::create_dir_all(&v02).unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package ex:restore@0.1.0;\ninterface api { record r { a: u32, b: u32 } }\n",
    )
    .unwrap();
    fs::write(
        v02.join("contract.wit"),
        "package ex:restore@0.2.0;\ninterface api { record r { a: u32, b: u32 } }\n",
    )
    .unwrap();
    let first = witforge()
        .arg("lock")
        .arg(&v01)
        .arg("--first")
        .output()
        .unwrap();
    assert!(first.status.success(), "{first:?}");
    fs::copy(v01.join("wire.lock"), v02.join("wire.lock")).unwrap();
    let baseline = v01.join("wire.lock");
    let never = witforge()
        .arg("lock")
        .arg(&v02)
        .arg("--against")
        .arg(&baseline)
        .output()
        .unwrap();
    assert!(never.status.success(), "{never:?}");
    let never_lock = fs::read_to_string(v02.join("wire.lock")).unwrap();
    assert!(never_lock.contains("ex:restore/api/r.b = 2 field u32"));

    fs::write(
        v02.join("contract.wit"),
        "package ex:restore@0.2.0;\ninterface api { record r { a: u32 } }\n",
    )
    .unwrap();
    let removed = witforge()
        .arg("lock")
        .arg(&v02)
        .arg("--against")
        .arg(&baseline)
        .output()
        .unwrap();
    assert!(removed.status.success(), "{removed:?}");
    assert!(
        fs::read_to_string(v02.join("wire.lock"))
            .unwrap()
            .contains("ex:restore/api/r.b = 2 field u32 reserved")
    );

    fs::write(
        v02.join("contract.wit"),
        "package ex:restore@0.2.0;\ninterface api { record r { a: u32, b: u32 } }\n",
    )
    .unwrap();
    let stale = witforge()
        .arg("check")
        .arg(&v02)
        .arg("--against")
        .arg(&baseline)
        .output()
        .unwrap();
    assert_eq!(stale.status.code(), Some(1), "{stale:?}");
    assert!(
        String::from_utf8_lossy(&stale.stderr).contains("can be restored"),
        "{stale:?}"
    );
    let restored = witforge()
        .arg("lock")
        .arg(&v02)
        .arg("--against")
        .arg(&baseline)
        .output()
        .unwrap();
    assert!(restored.status.success(), "{restored:?}");
    assert_eq!(
        fs::read_to_string(v02.join("wire.lock")).unwrap(),
        never_lock,
        "restoring a draft removal must recreate the unchanged lockfile"
    );
    fs::remove_dir_all(&root).unwrap();
}

fn closed_pipe_writer() -> std::process::Stdio {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    std::process::Stdio::from(writer)
}

fn write_large_contract(dir: &Path, version: &str, count: usize) {
    use std::fmt::Write as _;
    fs::create_dir_all(dir).unwrap();
    let mut source = format!("package ex:pipe@{version};\ninterface api {{\n  record big {{\n");
    for index in 0..count {
        let _ = writeln!(source, "    field{index}: option<u32>,");
    }
    source.push_str("  }\n}\n");
    fs::write(dir.join("contract.wit"), source).unwrap();
}

#[test]
fn lock_writes_before_reporting_and_survives_closed_stdout_and_stderr() {
    use std::io::Read as _;
    let root = std::env::temp_dir().join(format!("witforge-cli-pipe-lock-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let v01 = root.join("v0.1");
    write_large_contract(&v01, "0.1.0", 400);

    // Reader leaves immediately (`head -0`).
    let output = witforge()
        .args(["lock", v01.to_str().unwrap(), "--first"])
        .stdout(closed_pipe_writer())
        .stderr(std::process::Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(v01.join("wire.lock").is_file());
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("panicked"),
        "{output:?}"
    );

    // Reader takes a few bytes and leaves early (`head -1` on 400-member output).
    fs::remove_file(v01.join("wire.lock")).unwrap();
    let (mut reader, writer) = std::io::pipe().unwrap();
    let child = witforge()
        .args(["lock", v01.to_str().unwrap(), "--first"])
        .stdout(writer)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut sample = [0u8; 16];
    let _ = reader.read(&mut sample);
    drop(reader);
    let early = child.wait_with_output().unwrap();
    assert_eq!(early.status.code(), Some(0), "{early:?}");
    let v01_lock = fs::read_to_string(v01.join("wire.lock")).unwrap();
    assert!(v01_lock.contains("ex:pipe/api/big.field399 = 400 field option<u32>"));

    // `--against`, default `lock`, and JSON/SARIF modes all write before printing.
    let v02 = root.join("v0.2");
    write_large_contract(&v02, "0.2.0", 405);
    fs::copy(v01.join("wire.lock"), v02.join("wire.lock")).unwrap();
    let against = witforge()
        .args([
            "lock",
            v02.to_str().unwrap(),
            "--against",
            v01.join("wire.lock").to_str().unwrap(),
        ])
        .stdout(closed_pipe_writer())
        .output()
        .unwrap();
    assert_eq!(against.status.code(), Some(0), "{against:?}");
    assert!(
        fs::read_to_string(v02.join("wire.lock"))
            .unwrap()
            .contains("ex:pipe/api/big.field404 = 405 field option<u32>")
    );

    write_large_contract(&v02, "0.2.0", 406);
    for extra in [
        &[][..],
        &["--format", "json"][..],
        &["--format", "sarif"][..],
    ] {
        let mut cmd = witforge();
        cmd.args(["lock", v02.to_str().unwrap()]).args(extra);
        let res = cmd
            .stdout(closed_pipe_writer())
            .stderr(closed_pipe_writer())
            .status()
            .unwrap();
        assert_eq!(res.code(), Some(0), "{extra:?}: {res:?}");
    }
    assert!(
        fs::read_to_string(v02.join("wire.lock"))
            .unwrap()
            .contains("ex:pipe/api/big.field405 = 406 field option<u32>")
    );

    // `check` preserves exit 0 and exit 1 even when both stdout and stderr are closed.
    let ok = witforge()
        .args(["check", v01.to_str().unwrap()])
        .stdout(closed_pipe_writer())
        .stderr(closed_pipe_writer())
        .status()
        .unwrap();
    assert_eq!(ok.code(), Some(0));
    write_large_contract(&v02, "0.2.0", 407);
    let drift = witforge()
        .args(["check", v02.to_str().unwrap()])
        .stdout(closed_pipe_writer())
        .stderr(closed_pipe_writer())
        .status()
        .unwrap();
    assert_eq!(drift.code(), Some(1));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn diff_and_generators_handle_closed_stdout_without_panicking() {
    let base = versions("pipe-diff-gen");
    let wit = base.join("wit");
    let v01 = wit.join("v0.1");
    let v02 = wit.join("v0.2");

    // Human, `--json`, `--format json`, and `--format sarif` diff with closed stdout.
    for extra in [
        &[][..],
        &["--json"][..],
        &["--format", "json"][..],
        &["--format", "sarif"][..],
    ] {
        let out = witforge()
            .args(["diff", v01.to_str().unwrap(), v02.to_str().unwrap()])
            .args(extra)
            .stdout(closed_pipe_writer())
            .stderr(std::process::Stdio::piped())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{extra:?}: {out:?}");
        assert!(
            !String::from_utf8_lossy(&out.stderr).contains("panicked"),
            "{extra:?}: {out:?}"
        );
    }

    // `diff --check` preserves exit 1 on breaking changes with closed stdout and stderr.
    let breaking = witforge()
        .args([
            "diff",
            v01.to_str().unwrap(),
            v02.to_str().unwrap(),
            "--check",
        ])
        .stdout(closed_pipe_writer())
        .stderr(closed_pipe_writer())
        .status()
        .unwrap();
    assert_eq!(breaking.code(), Some(1));

    // Generator with JSON/SARIF into closed stdout writes its files and exits 0.
    let gen_out = base.join("rust-out");
    for fmt in ["json", "sarif"] {
        let res = witforge()
            .args([
                "rust",
                wit.to_str().unwrap(),
                "--out",
                gen_out.to_str().unwrap(),
                "--format",
                fmt,
            ])
            .stdout(closed_pipe_writer())
            .stderr(std::process::Stdio::piped())
            .output()
            .unwrap();
        assert_eq!(res.status.code(), Some(0), "{fmt}: {res:?}");
        assert!(gen_out.join("mod.rs").is_file());
    }

    // `--help` into a closed pipe does not panic.
    let help = witforge()
        .arg("--help")
        .stdout(closed_pipe_writer())
        .stderr(std::process::Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(help.status.code(), Some(0), "{help:?}");
    fs::remove_dir_all(&base).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn full_stdout_device_writes_outputs_and_exits_two() {
    let dev_full = || {
        fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .unwrap()
    };
    let root = std::env::temp_dir().join(format!("witforge-cli-dev-full-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let wit = root.join("wit");
    let v01 = wit.join("v0.1");
    let v02 = wit.join("v0.2");
    write_large_contract(&v01, "0.1.0", 6);
    write_large_contract(&v02, "0.2.0", 7);

    // `lock --first > /dev/full`: writes `wire.lock`, reports to stderr, exits 2 without panic.
    let lock_first = witforge()
        .args(["lock", v01.to_str().unwrap(), "--first"])
        .stdout(dev_full())
        .stderr(std::process::Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(lock_first.status.code(), Some(2), "{lock_first:?}");
    assert!(v01.join("wire.lock").is_file());
    let stderr = String::from_utf8_lossy(&lock_first.stderr);
    assert!(stderr.contains("stdout"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");

    // `lock --against > /dev/full` with closed stderr also writes `wire.lock` and exits 2.
    fs::copy(v01.join("wire.lock"), v02.join("wire.lock")).unwrap();
    let lock_against = witforge()
        .args([
            "lock",
            v02.to_str().unwrap(),
            "--against",
            v01.join("wire.lock").to_str().unwrap(),
        ])
        .stdout(dev_full())
        .stderr(closed_pipe_writer())
        .status()
        .unwrap();
    assert_eq!(lock_against.code(), Some(2));
    assert!(
        fs::read_to_string(v02.join("wire.lock"))
            .unwrap()
            .contains("ex:pipe/api/big.field6 = 7 field option<u32>")
    );

    // `diff` (human, `--json`, `--format json`, `--format sarif`) to `/dev/full`: exits 2.
    for extra in [
        &[][..],
        &["--json"][..],
        &["--format", "json"][..],
        &["--format", "sarif"][..],
    ] {
        let diff_out = witforge()
            .args(["diff", v01.to_str().unwrap(), v02.to_str().unwrap()])
            .args(extra)
            .stdout(dev_full())
            .stderr(std::process::Stdio::piped())
            .output()
            .unwrap();
        assert_eq!(diff_out.status.code(), Some(2), "{extra:?}: {diff_out:?}");
        assert!(
            !String::from_utf8_lossy(&diff_out.stderr).is_empty(),
            "{extra:?}"
        );
    }

    // Generator (`rust --format json > /dev/full`): writes files first, then exits 2.
    let gen_out = root.join("rust-out");
    let gen_res = witforge()
        .args([
            "rust",
            wit.to_str().unwrap(),
            "--out",
            gen_out.to_str().unwrap(),
            "--format",
            "json",
        ])
        .stdout(dev_full())
        .stderr(std::process::Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(gen_res.status.code(), Some(2), "{gen_res:?}");
    assert!(gen_out.join("mod.rs").is_file());
    fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn output_root_guard_closes_symlink_and_parent_component_bypasses() {
    let base = versions("out-guard-symlinks");
    let wit = base.join("wit");
    fs::create_dir_all(wit.join("v0.1/deps/shared")).unwrap();
    let v01_before: Vec<_> = fs::read_dir(wit.join("v0.1"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();

    std::os::unix::fs::symlink(&wit, base.join("witlink")).unwrap();
    std::os::unix::fs::symlink(&base, base.join("parentlink")).unwrap();
    std::os::unix::fs::symlink(wit.join("v0.1"), base.join("outlink")).unwrap();
    std::os::unix::fs::symlink(wit.join("gen"), base.join("danglinglink")).unwrap();

    // Exact audit reproduction (`missing/../witlink/v0.1`), symlinked ancestor, symlinked
    // output directory (existing and dangling), inside `deps/`, equal to WIT root, and an
    // output root containing the WIT root.
    let cases = [
        ("wit", "missing/../witlink/v0.1"),
        ("wit", "parentlink/wit/gen"),
        ("wit", "outlink"),
        ("wit", "danglinglink"),
        ("wit", "danglinglink/sub"),
        ("witlink", "wit/gen"),
        ("wit", "wit/v0.1/deps/shared"),
        ("wit", "wit"),
        ("wit", "."),
    ];
    for (root_arg, out_arg) in cases {
        for command in ["rust", "proto", "schema", "glue"] {
            for check_flag in [false, true] {
                let mut cmd = witforge();
                cmd.current_dir(&base)
                    .args([command, root_arg, "--out", out_arg]);
                if command == "glue" {
                    cmd.args([
                        "--plain-root",
                        "crate::plain",
                        "--proto-root",
                        "crate::proto",
                    ]);
                }
                if check_flag {
                    cmd.arg("--check");
                }
                let output = cmd.output().unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(2),
                    "{command} {root_arg} --out {out_arg} check={check_flag}: {output:?}"
                );
            }
        }
    }
    assert!(!base.join("missing").exists());
    assert!(!wit.join("gen").exists());
    let v01_after: Vec<_> = fs::read_dir(wit.join("v0.1"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(v01_after, v01_before);
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn output_root_guard_protects_released_directories() {
    let base = versions("out-guard-released");
    let wit = base.join("wit");
    // Put an explicit released manifest and its listed directory outside `wit/`.
    let archive = base.join("archive");
    copy_tree(&wit.join("v0.1"), &archive.join("v0.1"));
    let manifest = archive.join("released");
    fs::write(&manifest, "v0.1\n").unwrap();

    #[allow(unused_mut)]
    let mut bad_outs = vec![
        archive.join("v0.1"),
        archive.join("v0.1/nested"),
        archive.clone(),
    ];
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(archive.join("v0.1"), base.join("rel-link")).unwrap();
        bad_outs.push(base.join("rel-link"));
        bad_outs.push(base.join("missing/../rel-link"));
    }

    for out in &bad_outs {
        for command in ["rust", "proto", "schema", "glue"] {
            for check_flag in [false, true] {
                let mut cmd = witforge();
                cmd.args([
                    command,
                    wit.to_str().unwrap(),
                    "--out",
                    out.to_str().unwrap(),
                    "--released",
                    manifest.to_str().unwrap(),
                ]);
                if command == "glue" {
                    cmd.args([
                        "--plain-root",
                        "crate::plain",
                        "--proto-root",
                        "crate::proto",
                    ]);
                }
                if check_flag {
                    cmd.arg("--check");
                }
                let output = cmd.output().unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(2),
                    "{command} out={} check={check_flag}: {output:?}",
                    out.display()
                );
            }
        }
    }
    assert!(!archive.join("v0.1/nested").exists());
    assert!(!archive.join("mod.rs").exists());
    assert!(!base.join("missing").exists());

    // A bare `--released released` filename resolves its listed directories relative to cwd, not
    // the WIT root.
    let bare_released = witforge()
        .current_dir(&archive)
        .args([
            "rust",
            wit.to_str().unwrap(),
            "--out",
            "v0.1",
            "--released",
            "released",
        ])
        .output()
        .unwrap();
    assert_eq!(bare_released.status.code(), Some(2), "{bare_released:?}");
    assert!(
        String::from_utf8_lossy(&bare_released.stderr).contains("released directory"),
        "{bare_released:?}"
    );
    assert!(!archive.join("v0.1/mod.rs").exists());

    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn generators_refuse_shared_roots_across_both_orders_of_generators_and_contracts() {
    let base = versions("distinct-headers-shared");
    let wit_a = base.join("wit");
    // Build a second contract `ex:other` in `wit-b`.
    let wit_b = base.join("wit-b");
    let v01_b = wit_b.join("v0.1");
    fs::create_dir_all(&v01_b).unwrap();
    fs::write(
        v01_b.join("contract.wit"),
        "package ex:other@0.1.0;\ninterface api { record item { id: u32 } }\n",
    )
    .unwrap();
    assert!(
        witforge()
            .args(["lock", v01_b.to_str().unwrap(), "--first"])
            .status()
            .unwrap()
            .success()
    );

    let run_gen = |command: &str, wit: &Path, out: &Path, check: bool| {
        let mut cmd = witforge();
        cmd.args([
            command,
            wit.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ]);
        if command == "glue" {
            cmd.args([
                "--plain-root",
                "crate::plain",
                "--proto-root",
                "crate::proto",
            ]);
        }
        if check {
            cmd.arg("--check");
        }
        cmd.output().unwrap()
    };

    // Both orders of two generators into one output root: second is refused with exit 2 and
    // deletes nothing; `--check` also exits 2 naming the foreign generator rather than "extra".
    for (first, second, probe_file, expected_owner) in [
        ("rust", "glue", "v0_1.rs", "generator `rust`"),
        ("glue", "rust", "v0.rs", "generator `glue`"),
        ("rust", "proto", "mod.rs", "generator `rust`"),
        ("proto", "schema", "v0.proto", "generator `proto`"),
        ("schema", "rust", "v0_1.schema.json", "generator `schema`"),
    ] {
        let out = base.join(format!("shared-{first}-{second}"));
        assert!(run_gen(first, &wit_a, &out, false).status.success());
        let before = fs::read_to_string(out.join(probe_file)).unwrap();
        assert!(
            before.contains(&format!("@generated by witforge ({first}, ")),
            "{first}: {before}"
        );

        let write_second = run_gen(second, &wit_a, &out, false);
        assert_eq!(
            write_second.status.code(),
            Some(2),
            "{first}->{second}: {write_second:?}"
        );
        let stderr = String::from_utf8_lossy(&write_second.stderr);
        assert!(stderr.contains(expected_owner), "{stderr}");
        assert_eq!(fs::read_to_string(out.join(probe_file)).unwrap(), before);

        let check_second = run_gen(second, &wit_a, &out, true);
        assert_eq!(
            check_second.status.code(),
            Some(2),
            "{first}->{second} --check: {check_second:?}"
        );
        let check_err = String::from_utf8_lossy(&check_second.stderr);
        assert!(check_err.contains(expected_owner), "{check_err}");
        assert!(!check_err.contains("extra generated file"), "{check_err}");
    }

    // Both orders of two different contracts with the same generator into one output root.
    for (label, first_wit, second_wit, expected_pkg) in [
        ("a-then-b", &wit_a, &wit_b, "example:evolve"),
        ("b-then-a", &wit_b, &wit_a, "ex:other"),
    ] {
        let out = base.join(format!("two-roots-{label}"));
        assert!(run_gen("rust", first_wit, &out, false).status.success());
        let before = fs::read_to_string(out.join("mod.rs")).unwrap();
        let conflict = run_gen("rust", second_wit, &out, false);
        assert_eq!(conflict.status.code(), Some(2), "{label}: {conflict:?}");
        assert!(
            String::from_utf8_lossy(&conflict.stderr).contains(expected_pkg),
            "{conflict:?}"
        );
        assert_eq!(fs::read_to_string(out.join("mod.rs")).unwrap(), before);
    }

    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn generators_adopt_legacy_shared_header_and_preserve_unowned_files() {
    let base = versions("legacy-header-adopt");
    let wit = base.join("wit");

    // Rust: legacy header at expected path causes `--check` to report drift (exit 1), and
    // regenerating adopts and rewrites it with the scoped header while removing legacy stale files
    // and leaving unowned files untouched.
    let rust_out = base.join("rust-out");
    fs::create_dir_all(&rust_out).unwrap();
    fs::write(
        rust_out.join("mod.rs"),
        "// @generated by witforge; do not edit.\n// old\n",
    )
    .unwrap();
    fs::write(
        rust_out.join("stale_old.rs"),
        "// @generated by witforge; do not edit.\n",
    )
    .unwrap();
    fs::write(rust_out.join("notes.txt"), "hand-written\n").unwrap();

    let check_before = witforge()
        .args([
            "rust",
            wit.to_str().unwrap(),
            "--out",
            rust_out.to_str().unwrap(),
            "--check",
        ])
        .output()
        .unwrap();
    assert_eq!(check_before.status.code(), Some(1), "{check_before:?}");

    let adopted = witforge()
        .args([
            "rust",
            wit.to_str().unwrap(),
            "--out",
            rust_out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(adopted.status.success(), "{adopted:?}");
    let mod_rs = fs::read_to_string(rust_out.join("mod.rs")).unwrap();
    assert!(
        mod_rs.starts_with("// @generated by witforge (rust, example:evolve); do not edit.\n"),
        "{mod_rs}"
    );
    assert!(!rust_out.join("stale_old.rs").exists());
    assert_eq!(
        fs::read_to_string(rust_out.join("notes.txt")).unwrap(),
        "hand-written\n"
    );

    // JSON Schema: legacy `$comment` header is adopted and rewritten too.
    let schema_out = base.join("schema-out");
    fs::create_dir_all(&schema_out).unwrap();
    fs::write(
        schema_out.join("v0_1.schema.json"),
        "{\"$comment\":\"@generated by witforge; do not edit.\",\n  \"title\": \"old\"\n}\n",
    )
    .unwrap();
    assert!(
        witforge()
            .args([
                "schema",
                wit.to_str().unwrap(),
                "--out",
                schema_out.to_str().unwrap(),
            ])
            .status()
            .unwrap()
            .success()
    );
    let schema_text = fs::read_to_string(schema_out.join("v0_1.schema.json")).unwrap();
    assert!(
        schema_text.starts_with(
            "{\"$comment\":\"@generated by witforge (schema, example:evolve); do not edit.\",\n"
        ),
        "{schema_text}"
    );

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn glue_refuses_relative_and_colliding_module_roots_on_cli_and_in_config() {
    let base = versions("absolute-module-roots");
    let wit = base.join("wit");
    let glue_out = base.join("glue-out");

    for (plain_root, proto_root, expected) in [
        ("self::plain", "crate::proto", "relative"),
        ("crate::plain", "super::super::proto", "relative"),
        ("v0::plain", "crate::proto", "starts with generated module"),
        (
            "crate::plain",
            "types::proto",
            "starts with generated module",
        ),
        (
            "witforge_deps::plain",
            "crate::proto",
            "starts with generated item",
        ),
    ] {
        let output = witforge()
            .args([
                "glue",
                wit.to_str().unwrap(),
                "--out",
                glue_out.to_str().unwrap(),
                "--plain-root",
                plain_root,
                "--proto-root",
                proto_root,
            ])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "{plain_root} / {proto_root}: {output:?}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "{stderr}");
        assert!(!glue_out.exists());
    }

    // Invalid roots in `witforge.toml` are refused even when overridden on the CLI or when
    // another emitter runs against the config file.
    let bad_config = base.join("witforge.toml");
    fs::write(
        &bad_config,
        "version = 1\n\n\
         [project.app]\n\
         root = \"wit\"\n\n\
         [project.app.rust]\n\
         out = \"rust-out\"\n\n\
         [project.app.glue]\n\
         out = \"glue-out\"\n\
         plain-root = \"super::plain\"\n\
         proto-root = \"crate::proto\"\n",
    )
    .unwrap();
    let overridden = witforge()
        .args([
            "glue",
            "--config",
            bad_config.to_str().unwrap(),
            "--plain-root",
            "crate::plain",
        ])
        .output()
        .unwrap();
    assert_eq!(overridden.status.code(), Some(2), "{overridden:?}");
    assert!(
        String::from_utf8_lossy(&overridden.stderr).contains("plain-root"),
        "{overridden:?}"
    );
    let other_emitter = witforge()
        .args(["rust", "--config", bad_config.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(other_emitter.status.code(), Some(2), "{other_emitter:?}");

    // Valid absolute forms (`crate::...`, `::other::...`, `other::...`) succeed, while a CLI
    // override to a relative root over a valid config is still refused.
    fs::write(
        &bad_config,
        "version = 1\n\n\
         [project.app]\n\
         root = \"wit\"\n\n\
         [project.app.rust]\n\
         out = \"rust-out\"\n\n\
         [project.app.glue]\n\
         out = \"glue-out\"\n\
         plain-root = \"::other_plain::r#gen\"\n\
         proto-root = \"other_proto::wire\"\n",
    )
    .unwrap();
    let bad_override = witforge()
        .args([
            "glue",
            "--config",
            bad_config.to_str().unwrap(),
            "--proto-root",
            "super::proto",
        ])
        .output()
        .unwrap();
    assert_eq!(bad_override.status.code(), Some(2), "{bad_override:?}");

    let ok = witforge()
        .args(["glue", "--config", bad_config.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(ok.status.success(), "{ok:?}");
    assert!(glue_out.join("v0.rs").is_file());
    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn check_reports_generator_refusals_and_accepts_proto_keyword_packages() {
    let base = std::env::temp_dir().join(format!(
        "witforge-cli-check-implies-gen-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&base);

    // 1. `package example:service@0.1.0;` passes `lock`, `check`, and all four generators.
    let svc_wit = base.join("svc/wit");
    let svc_v01 = svc_wit.join("v0.1");
    fs::create_dir_all(&svc_v01).unwrap();
    fs::write(
        svc_v01.join("contract.wit"),
        "package example:service@0.1.0;\ninterface types { record item { id: u32 } ping: func(i: item) -> item; }\n",
    )
    .unwrap();
    let out = witforge()
        .args(["lock", "--first"])
        .arg(&svc_v01)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let out = witforge().args(["check"]).arg(&svc_v01).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    for (subcmd, extra) in [
        ("rust", &[][..]),
        ("proto", &[][..]),
        ("schema", &[][..]),
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
        let out_dir = base.join(format!("svc/{subcmd}"));
        let output = witforge()
            .arg(subcmd)
            .arg(&svc_wit)
            .arg("--out")
            .arg(&out_dir)
            .args(extra)
            .output()
            .unwrap();
        assert!(output.status.success(), "{subcmd} failed: {output:?}");
    }

    // 2. `record get-request` beside `get: func()` is refused by `witforge check` with exit 1 and
    // structured code `WF1003`.
    let clash_v01 = base.join("clash/wit/v0.1");
    fs::create_dir_all(&clash_v01).unwrap();
    fs::write(
        clash_v01.join("contract.wit"),
        "package example:clash@0.1.0;\ninterface types { record get-request { id: u32 } get: func() -> get-request; }\n",
    )
    .unwrap();
    fs::write(clash_v01.join("wire.lock"), "").unwrap();
    let output = witforge()
        .args(["--format", "json", "check"])
        .arg(&clash_v01)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"WF1003\""), "{stdout}");
    assert!(stdout.contains("TypesGetRequest"), "{stdout}");

    // 3. A live type colliding with a removed interface's preserved `Method` enum in `wire.lock`
    // is caught by `witforge check` with `WF1003` and exit 1.
    let reserved_root = base.join("reserved-method/wit");
    let reserved_v01 = reserved_root.join("v0.1");
    let reserved_v02 = reserved_root.join("v0.2");
    fs::create_dir_all(&reserved_v01).unwrap();
    fs::create_dir_all(&reserved_v02).unwrap();
    fs::write(
        reserved_v01.join("contract.wit"),
        "package example:clash@0.1.0;\ninterface api-x { ping: func(); }\n",
    )
    .unwrap();
    fs::write(
        reserved_v02.join("contract.wit"),
        "package example:clash@0.2.0;\ninterface api { record x-method { id: u32 } }\n",
    )
    .unwrap();
    let out = witforge()
        .args(["lock", "--first"])
        .arg(&reserved_v01)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    fs::copy(
        reserved_v01.join("wire.lock"),
        reserved_v02.join("wire.lock"),
    )
    .unwrap();
    let out = witforge()
        .args(["lock", "--against"])
        .arg(reserved_v01.join("wire.lock"))
        .arg(&reserved_v02)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let output = witforge()
        .args(["--format", "json", "check"])
        .arg(&reserved_v02)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"WF1003\""), "{stdout}");
    assert!(stdout.contains("ApiXMethod"), "{stdout}");

    fs::remove_dir_all(&base).unwrap();
}

/// Extract every backticked `` `witforge ...` `` command printed in `text`.
fn extract_witforge_commands(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('`') else { break };
        let inside = &rest[..end];
        rest = &rest[end + 1..];
        if inside.starts_with("witforge ") && inside != "witforge lock" {
            out.push(inside.to_string());
        }
    }
    out
}

/// Split a POSIX-quoted `witforge ...` command line into argv tokens (works on every OS).
fn split_posix_command(command: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut started = false;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_single {
            if ch == '\'' {
                in_single = false;
            } else {
                current.push(ch);
            }
        } else {
            match ch {
                '\'' => {
                    in_single = true;
                    started = true;
                }
                '\\' if chars.peek() == Some(&'\'') => {
                    current.push(chars.next().unwrap());
                    started = true;
                }
                c if c.is_ascii_whitespace() => {
                    if started {
                        args.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                c => {
                    current.push(c);
                    started = true;
                }
            }
        }
    }
    assert!(!in_single, "unterminated single quote in `{command}`");
    if started {
        args.push(current);
    }
    args
}

/// Execute a printed `witforge ...` remedy command directly via the compiled binary.
fn run_printed_command(command: &str) -> std::process::Output {
    let tokens = split_posix_command(command);
    assert_eq!(
        tokens.first().map(String::as_str),
        Some("witforge"),
        "not a witforge command: {command}"
    );
    witforge().args(&tokens[1..]).output().unwrap()
}

#[allow(clippy::too_many_lines)]
#[test]
fn cli_remedy_messages_are_accurate_and_run_as_printed() {
    let base = std::env::temp_dir().join(format!("witforge cli remedies {}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let wit_root = base.join("my wit");
    let v01 = wit_root.join("v0.1");
    let v02 = wit_root.join("v0.2");
    let out_dir = base.join("out");
    fs::create_dir_all(&v01).unwrap();
    fs::create_dir_all(&v02).unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package example:rem@0.1.0;\ninterface types { record item { id: u32 } }\n",
    )
    .unwrap();
    fs::write(
        v02.join("contract.wit"),
        "package example:rem@0.2.0;\ninterface types { record item { id: u32, name: option<string> } }\n",
    )
    .unwrap();

    // 1. Generator run with missing `wire.lock` on v0.1 names the file and both ways forward,
    // quotes paths with spaces, prints no unrunnable placeholder command, and its printed
    // `--first` command runs verbatim.
    let gen_missing = witforge()
        .args(["rust"])
        .arg(&wit_root)
        .arg("--out")
        .arg(&out_dir)
        .output()
        .unwrap();
    assert_eq!(gen_missing.status.code(), Some(2), "{gen_missing:?}");
    let stderr = String::from_utf8_lossy(&gen_missing.stderr);
    assert!(stderr.contains("wire.lock"), "{stderr}");
    assert!(stderr.contains("--first"), "{stderr}");
    assert!(stderr.contains("--against"), "{stderr}");
    assert!(!stderr.contains("--against <"), "{stderr}");
    assert!(!stderr.contains("run `witforge lock`"), "{stderr}");
    let cmds = extract_witforge_commands(&stderr);
    assert_eq!(cmds.len(), 1, "expected 1 runnable remedy in: {stderr}");
    let ran = run_printed_command(&cmds[0]);
    assert!(ran.status.success(), "{}: {ran:?}", cmds[0]);
    assert!(v01.join("wire.lock").is_file());

    // 2. Generator run with missing `wire.lock` on v0.2 (sibling v0.1 exists) gives a copy +
    // runnable `--against` command that works after copying v0.1/wire.lock.
    let gen_missing_v02 = witforge()
        .args(["rust"])
        .arg(&wit_root)
        .arg("--out")
        .arg(&out_dir)
        .output()
        .unwrap();
    assert_eq!(
        gen_missing_v02.status.code(),
        Some(2),
        "{gen_missing_v02:?}"
    );
    let stderr = String::from_utf8_lossy(&gen_missing_v02.stderr);
    assert!(stderr.contains("wire.lock"), "{stderr}");
    assert!(stderr.contains("--against"), "{stderr}");
    assert!(stderr.contains("--first"), "{stderr}");
    let cmds = extract_witforge_commands(&stderr);
    assert_eq!(cmds.len(), 1, "expected 1 runnable remedy in: {stderr}");
    fs::copy(v01.join("wire.lock"), v02.join("wire.lock")).unwrap();
    let ran = run_printed_command(&cmds[0]);
    assert!(ran.status.success(), "{}: {ran:?}", cmds[0]);

    // 3. `witforge lock` on a WIT root says a version directory is expected, lists all found
    // version directories, carries the text in JSON/SARIF, and its printed command runs verbatim.
    let root_lock = witforge().arg("lock").arg(&wit_root).output().unwrap();
    assert_eq!(root_lock.status.code(), Some(2), "{root_lock:?}");
    let stderr = String::from_utf8_lossy(&root_lock.stderr);
    assert!(stderr.contains("WIT root"), "{stderr}");
    assert!(stderr.contains("v0.1, v0.2"), "{stderr}");
    assert!(stderr.contains("version directory"), "{stderr}");
    let cmds = extract_witforge_commands(&stderr);
    assert_eq!(cmds.len(), 1, "expected 1 runnable remedy in: {stderr}");
    let ran = run_printed_command(&cmds[0]);
    assert!(ran.status.success(), "{}: {ran:?}", cmds[0]);

    // JSON & SARIF also carry the corrected WIT-root message on `lock`.
    let json_out = witforge()
        .args(["--format", "json", "lock"])
        .arg(&wit_root)
        .output()
        .unwrap();
    assert_eq!(json_out.status.code(), Some(2), "{json_out:?}");
    let json_stdout = String::from_utf8_lossy(&json_out.stdout);
    assert!(json_stdout.contains("v0.1, v0.2"), "{json_stdout}");
    let sarif_out = witforge()
        .args(["--format", "sarif", "lock"])
        .arg(&wit_root)
        .output()
        .unwrap();
    assert_eq!(sarif_out.status.code(), Some(2), "{sarif_out:?}");
    let sarif_stdout = String::from_utf8_lossy(&sarif_out.stdout);
    assert!(sarif_stdout.contains("v0.1, v0.2"), "{sarif_stdout}");

    // 4. Generator run with stale `wire.lock` prints a quoted runnable `witforge lock ...` command
    // (never bare `witforge lock`), and running it fixes generation.
    fs::write(
        v02.join("contract.wit"),
        "package example:rem@0.2.0;\ninterface types { record item { id: u32, name: option<string>, extra: option<u32> } }\n",
    )
    .unwrap();
    let gen_stale = witforge()
        .args(["rust"])
        .arg(&wit_root)
        .arg("--out")
        .arg(&out_dir)
        .output()
        .unwrap();
    assert_eq!(gen_stale.status.code(), Some(1), "{gen_stale:?}");
    let stderr = String::from_utf8_lossy(&gen_stale.stderr);
    assert!(!stderr.contains("run `witforge lock`"), "{stderr}");
    let cmds = extract_witforge_commands(&stderr);
    assert_eq!(cmds.len(), 1, "expected 1 runnable remedy in: {stderr}");
    let ran = run_printed_command(&cmds[0]);
    assert!(ran.status.success(), "{}: {ran:?}", cmds[0]);

    // 5. Generator run with missing order section prints a quoted runnable command too.
    let lock_text = fs::read_to_string(v02.join("wire.lock")).unwrap();
    let without_order = lock_text
        .split("# witforge order v1")
        .next()
        .unwrap()
        .to_string();
    fs::write(v02.join("wire.lock"), without_order).unwrap();
    let gen_order = witforge()
        .args(["rust"])
        .arg(&wit_root)
        .arg("--out")
        .arg(&out_dir)
        .output()
        .unwrap();
    assert_eq!(gen_order.status.code(), Some(1), "{gen_order:?}");
    let stderr = String::from_utf8_lossy(&gen_order.stderr);
    assert!(!stderr.contains("run `witforge lock`"), "{stderr}");
    let cmds = extract_witforge_commands(&stderr);
    assert_eq!(cmds.len(), 1, "expected 1 runnable remedy in: {stderr}");
    let ran = run_printed_command(&cmds[0]);
    assert!(ran.status.success(), "{}: {ran:?}", cmds[0]);

    // Generation now succeeds.
    let gen_ok = witforge()
        .args(["rust"])
        .arg(&wit_root)
        .arg("--out")
        .arg(&out_dir)
        .output()
        .unwrap();
    assert!(gen_ok.status.success(), "{gen_ok:?}");

    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn check_explains_drift_with_config_command_crlf_hint_and_unified_diff() {
    let base = std::env::temp_dir().join(format!("witforge-check-diff-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let wit_root = base.join("wit");
    let v01 = wit_root.join("v0.1");
    fs::create_dir_all(&v01).unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package example:demo@0.1.0;\ninterface types { record item { id: u32 } }\n",
    )
    .unwrap();
    assert!(
        witforge()
            .args(["lock", "--first"])
            .arg(&v01)
            .output()
            .unwrap()
            .status
            .success()
    );

    let cfg = base.join("witforge.toml");
    fs::write(
        &cfg,
        "version = 1\n[project.alpha]\nroot = \"wit\"\n[project.alpha.rust]\nout = \"gen/alpha\"\n[project.beta]\nroot = \"wit\"\n[project.beta.rust]\nout = \"gen/beta\"\n",
    )
    .unwrap();

    // Generate with --diff on a fresh directory prints the added files and exits 0.
    let first_gen = witforge()
        .args(["rust", "--config"])
        .arg(&cfg)
        .args(["--project", "alpha", "--diff"])
        .output()
        .unwrap();
    assert!(first_gen.status.success(), "{first_gen:?}");
    let first_stdout = String::from_utf8_lossy(&first_gen.stdout);
    assert!(first_stdout.contains("--- /dev/null"), "{first_stdout}");
    assert!(first_stdout.contains("+++ b/"), "{first_stdout}");

    // Clean --check --diff prints nothing and exits 0.
    let clean = witforge()
        .args(["rust", "--config"])
        .arg(&cfg)
        .args(["--project", "alpha", "--check", "--diff"])
        .output()
        .unwrap();
    assert!(clean.status.success(), "{clean:?}");
    assert!(clean.stdout.is_empty(), "{clean:?}");

    // Mutate one generated line: --check --diff reports first differing line, prints a unified
    // diff on stdout, names the exact --config/--project command, and keeps exit status 1.
    let mod_rs = base.join("gen/alpha/mod.rs");
    let original = fs::read_to_string(&mod_rs).unwrap();
    fs::write(&mod_rs, original.replace("pub mod v0_1;", "pub mod stale;")).unwrap();
    let stale_check = witforge()
        .args(["rust", "--config"])
        .arg(&cfg)
        .args(["--project", "alpha", "--check", "--diff"])
        .output()
        .unwrap();
    assert_eq!(stale_check.status.code(), Some(1), "{stale_check:?}");
    let stderr = String::from_utf8_lossy(&stale_check.stderr);
    let stdout = String::from_utf8_lossy(&stale_check.stdout);
    assert!(stderr.contains("first difference at line"), "{stderr}");
    assert!(stderr.contains("pub mod v0_1;"), "{stderr}");
    assert!(stderr.contains("pub mod stale;"), "{stderr}");
    assert!(stderr.contains("--config"), "{stderr}");
    assert!(stderr.contains("--project alpha"), "{stderr}");
    assert!(stdout.contains("--- a/"), "{stdout}");
    assert!(stdout.contains("-pub mod stale;"), "{stdout}");
    assert!(stdout.contains("+pub mod v0_1;"), "{stdout}");

    // Executing the printed regenerating command fixes drift cleanly.
    let cmds = extract_witforge_commands(&stderr);
    assert_eq!(cmds.len(), 1, "expected 1 regen command in: {stderr}");
    let ran = run_printed_command(&cmds[0]);
    assert!(ran.status.success(), "{}: {ran:?}", cmds[0]);
    assert_eq!(fs::read_to_string(&mod_rs).unwrap(), original);

    // CRLF-only drift gives a line-ending hint.
    fs::write(&mod_rs, original.replace('\n', "\r\n")).unwrap();
    let crlf_check = witforge()
        .args(["rust", "--config"])
        .arg(&cfg)
        .args(["--project", "alpha", "--check"])
        .output()
        .unwrap();
    assert_eq!(crlf_check.status.code(), Some(1), "{crlf_check:?}");
    let crlf_stderr = String::from_utf8_lossy(&crlf_check.stderr);
    assert!(
        crlf_stderr.contains("line endings differ") && crlf_stderr.contains("core.autocrlf"),
        "{crlf_stderr}"
    );

    fs::remove_dir_all(&base).unwrap();
}

fn git_in(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed in {}: {}",
        args,
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[allow(clippy::too_many_lines)]
#[test]
fn witforge_freeze_checks_history_shallow_clones_multiple_violations_and_config_projects() {
    let base = std::env::temp_dir().join(format!("witforge-freeze-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let repo = base.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git_in(&repo, &["init", "-q"]);
    git_in(&repo, &["config", "user.name", "test"]);
    git_in(&repo, &["config", "user.email", "test@example.invalid"]);

    // Two WIT roots in one repo configured via witforge.toml.
    for (root_name, pkg) in [("alpha", "ex:alpha"), ("beta", "ex:beta")] {
        let v1 = repo.join(root_name).join("v0.1");
        let v2 = repo.join(root_name).join("v0.2");
        fs::create_dir_all(&v1).unwrap();
        fs::create_dir_all(&v2).unwrap();
        fs::write(
            v1.join("contract.wit"),
            format!("package {pkg}@0.1.0;\ninterface a {{ record r {{ id: u32 }} }}\n"),
        )
        .unwrap();
        fs::write(
            v1.join("wire.lock"),
            format!("# witforge wire lockfile v2\n{pkg}/a/r.id = 1 field u32\n"),
        )
        .unwrap();
        fs::write(
            v2.join("contract.wit"),
            format!("package {pkg}@0.2.0;\ninterface a {{ record r {{ id: u32, n: u32 }} }}\n"),
        )
        .unwrap();
        fs::write(
            v2.join("wire.lock"),
            format!(
                "# witforge wire lockfile v2\n{pkg}/a/r.id = 1 field u32\n{pkg}/a/r.n = 2 field u32\n"
            ),
        )
        .unwrap();
        fs::write(repo.join(root_name).join("released"), "v0.1\n").unwrap();
    }
    let cfg = repo.join("witforge.toml");
    fs::write(
        &cfg,
        "version = 1\n\
         [project.alpha]\nroot = \"alpha\"\n\
         [project.beta]\nroot = \"beta\"\n",
    )
    .unwrap();
    git_in(&repo, &["add", "-A"]);
    git_in(&repo, &["commit", "-qm", "release alpha and beta v0.1"]);

    // 1. Clean multi-project config check passes (exit 0).
    let ok = witforge()
        .current_dir(&repo)
        .args(["freeze", "--config", "witforge.toml"])
        .output()
        .unwrap();
    assert!(ok.status.success(), "{ok:?}");
    let stdout = String::from_utf8_lossy(&ok.stdout);
    assert!(
        stdout.contains("alpha") && stdout.contains("beta"),
        "{stdout}"
    );

    // 2. Shallow clone MUST exit 2 and explicitly name `fetch-depth: 0`.
    let shallow = base.join("shallow");
    let clone_url = format!("file://{}", repo.display());
    let cloned = Command::new("git")
        .args(["clone", "-q", "--depth", "1", &clone_url])
        .arg(&shallow)
        .output()
        .unwrap();
    assert!(cloned.status.success(), "{cloned:?}");
    let shallow_out = witforge()
        .current_dir(&shallow)
        .args(["freeze", "alpha", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(shallow_out.status.code(), Some(2), "{shallow_out:?}");
    let shallow_json: serde_json::Value = serde_json::from_slice(&shallow_out.stdout).unwrap();
    assert_eq!(shallow_json["diagnostics"][0]["code"], "WF9004");
    assert!(
        shallow_json["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("fetch-depth: 0"),
        "{shallow_json}"
    );

    // 3. CRLF checkout under core.autocrlf=true still passes when Git blob is unchanged.
    let autocrlf_clone = base.join("autocrlf");
    let cloned_crlf = Command::new("git")
        .args(["clone", "-q", "-c", "core.autocrlf=true", &clone_url])
        .arg(&autocrlf_clone)
        .output()
        .unwrap();
    assert!(cloned_crlf.status.success(), "{cloned_crlf:?}");
    let checked_out_bytes = fs::read(autocrlf_clone.join("alpha/v0.1/contract.wit")).unwrap();
    assert!(
        checked_out_bytes.windows(2).any(|w| w == b"\r\n"),
        "expected core.autocrlf=true clone to write CRLF bytes on disk"
    );
    let crlf_out = witforge()
        .current_dir(&autocrlf_clone)
        .args(["freeze", "alpha"])
        .output()
        .unwrap();
    assert!(
        crlf_out.status.success(),
        "core.autocrlf=true checkout failed: {crlf_out:?}"
    );

    // 4. Multiple violations across BOTH projects and BOTH files inside v0.1 are ALL reported (exit 1).
    let base_sha_out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&repo)
        .output()
        .unwrap();
    let release_sha = String::from_utf8_lossy(&base_sha_out.stdout)
        .trim()
        .to_string();

    fs::write(
        repo.join("alpha/v0.1/contract.wit"),
        "package ex:alpha@0.1.0;\ninterface a { record r { id: u64 } }\n",
    )
    .unwrap();
    fs::write(
        repo.join("alpha/v0.1/wire.lock"),
        "# witforge wire lockfile v2\nex:alpha/a/r.id = 1 field u64\n",
    )
    .unwrap();
    fs::write(repo.join("beta/released"), "# removed v0.1\n").unwrap();
    git_in(&repo, &["add", "-A"]);
    git_in(&repo, &["commit", "-qm", "tamper alpha and unrelease beta"]);

    let multi = witforge()
        .current_dir(&repo)
        .args(["freeze", "--config", "witforge.toml", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(multi.status.code(), Some(1), "{multi:?}");
    let multi_json: serde_json::Value = serde_json::from_slice(&multi.stdout).unwrap();
    let diags = multi_json["diagnostics"].as_array().unwrap();
    assert!(
        diags.len() >= 3,
        "expected all violations across both projects: {diags:?}"
    );
    assert!(diags.iter().all(|d| d["code"] == "WF1109"), "{diags:?}");
    let paths: Vec<&str> = diags.iter().filter_map(|d| d["path"].as_str()).collect();
    assert!(paths.contains(&"alpha/v0.1/contract.wit"), "{paths:?}");
    assert!(paths.contains(&"alpha/v0.1/wire.lock"), "{paths:?}");
    assert!(paths.contains(&"beta/released"), "{paths:?}");

    // 5. `--base` bounds history while still catching edits after `--base`.
    let bounded = witforge()
        .current_dir(&repo)
        .args(["freeze", "alpha", "--base", &release_sha])
        .output()
        .unwrap();
    assert_eq!(bounded.status.code(), Some(1), "{bounded:?}");

    // 6. Outside any git repository -> exit 2.
    let outside = base.join("outside/wit");
    fs::create_dir_all(&outside).unwrap();
    let no_repo = witforge()
        .env("GIT_CEILING_DIRECTORIES", &base)
        .args(["freeze"])
        .arg(&outside)
        .output()
        .unwrap();
    assert_eq!(no_repo.status.code(), Some(2), "{no_repo:?}");

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn whole_root_check_new_version_and_release_workflow_cover_gaps_drafts_and_majors() {
    let base = std::env::temp_dir().join(format!("witforge-workflow-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let wit_root = base.join("wit");
    let v01 = wit_root.join("v0.1");
    fs::create_dir_all(v01.join("deps/shared")).unwrap();
    fs::write(
        v01.join("deps/shared/types.wit"),
        "package ex:shared@0.1.0;\ninterface common { record tag { key: string } }\n",
    )
    .unwrap();
    fs::write(
        v01.join("a.wit"),
        "package ex:flow@0.1.0;\ninterface types {\n    use ex:shared/common@0.1.0.{tag};\n    record item { id: u32, tag: tag }\n}\n",
    )
    .unwrap();
    fs::write(
        v01.join("b.wit"),
        "// secondary file without package header\ninterface extra { record note { text: string } }\n",
    )
    .unwrap();

    // 1. Before locking v0.1, `release` must refuse without creating `released`.
    let bad_rel = witforge().arg("release").arg(&v01).output().unwrap();
    assert_eq!(bad_rel.status.code(), Some(2), "{bad_rel:?}");
    assert!(!wit_root.join("released").exists());

    // Lock v0.1, check whole root (lowest minor as draft -> passes), then release v0.1.
    assert!(
        witforge()
            .args(["lock"])
            .arg(&v01)
            .arg("--first")
            .output()
            .unwrap()
            .status
            .success()
    );
    let root_ok_v1 = witforge().args(["check"]).arg(&wit_root).output().unwrap();
    assert!(root_ok_v1.status.success(), "{root_ok_v1:?}");

    let rel1 = witforge().args(["release"]).arg(&v01).output().unwrap();
    assert!(rel1.status.success(), "{rel1:?}");
    assert_eq!(
        fs::read_to_string(wit_root.join("released")).unwrap(),
        "v0.1\n"
    );
    // Releasing v0.1 a second time is an idempotent no-op.
    let rel1_again = witforge().args(["release"]).arg(&v01).output().unwrap();
    assert!(rel1_again.status.success(), "{rel1_again:?}");
    assert_eq!(
        fs::read_to_string(wit_root.join("released")).unwrap(),
        "v0.1\n"
    );

    // 2. `new-version` creates v0.2 with copied deps, wire.lock, and rewritten package header
    // (without touching `use ex:shared/common@0.1.0`).
    let nv2 = witforge()
        .args(["new-version"])
        .arg(&wit_root)
        .output()
        .unwrap();
    assert!(nv2.status.success(), "{nv2:?}");
    let v02 = wit_root.join("v0.2");
    assert!(v02.join("wire.lock").is_file());
    assert!(v02.join("deps/shared/types.wit").is_file());
    assert!(v02.join("b.wit").is_file());
    let v02_a = fs::read_to_string(v02.join("a.wit")).unwrap();
    assert!(v02_a.contains("package ex:flow@0.2.0;"), "{v02_a}");
    assert!(
        v02_a.contains("use ex:shared/common@0.1.0.{tag};"),
        "dependency version must not be rewritten: {v02_a}"
    );
    // Target already exists -> refused.
    let nv3 = witforge()
        .args(["new-version"])
        .arg(&wit_root)
        .output()
        .unwrap();
    assert!(nv3.status.success(), "{nv3:?}");
    // Now we have TWO unreleased drafts at once (`v0.2` and `v0.3`)!
    // Releasing v0.3 while v0.2 is unreleased MUST be refused (exit 1).
    let v03 = wit_root.join("v0.3");
    let rel3_unreleased_pred = witforge().args(["release"]).arg(&v03).output().unwrap();
    assert_eq!(
        rel3_unreleased_pred.status.code(),
        Some(1),
        "{rel3_unreleased_pred:?}"
    );
    assert_eq!(
        fs::read_to_string(wit_root.join("released")).unwrap(),
        "v0.1\n"
    );

    // 3. `new-version --major` creates v1.0 WITHOUT a copied wire.lock.
    let nv_major = witforge()
        .args(["new-version", "--major"])
        .arg(&wit_root)
        .output()
        .unwrap();
    assert!(nv_major.status.success(), "{nv_major:?}");
    let v10 = wit_root.join("v1.0");
    assert!(v10.join("a.wit").is_file());
    assert!(
        fs::read_to_string(v10.join("a.wit"))
            .unwrap()
            .contains("package ex:flow@1.0.0;")
    );
    assert!(
        !v10.join("wire.lock").exists(),
        "new major must not copy wire.lock"
    );

    // Whole-root check catches unlocked v1.0 (exit 1), then passes once v1.0 is locked --first.
    let check_unlocked_v10 = witforge().args(["check"]).arg(&wit_root).output().unwrap();
    assert_eq!(
        check_unlocked_v10.status.code(),
        Some(1),
        "{check_unlocked_v10:?}"
    );
    assert!(
        witforge()
            .args(["lock"])
            .arg(&v10)
            .arg("--first")
            .output()
            .unwrap()
            .status
            .success()
    );
    let check_all_drafts_and_major = witforge().args(["check"]).arg(&wit_root).output().unwrap();
    assert!(
        check_all_drafts_and_major.status.success(),
        "{check_all_drafts_and_major:?}"
    );

    // 4. Gap in minor numbers (`v0.1` released, remove `v0.2`, keep `v0.3` draft): whole-root check
    // checks `v0.3` against `v0.1`.
    fs::remove_dir_all(&v02).unwrap();
    let check_gap = witforge().args(["check"]).arg(&wit_root).output().unwrap();
    assert!(check_gap.status.success(), "{check_gap:?}");

    // 5. Ambiguous dir (both `.wit` and `v0.1/`) and empty dir are refused with clear messages.
    let both_dir = base.join("both");
    fs::create_dir_all(both_dir.join("v0.1")).unwrap();
    fs::write(both_dir.join("contract.wit"), "package ex:b@0.1.0;\n").unwrap();
    let both_out = witforge().args(["check"]).arg(&both_dir).output().unwrap();
    assert_eq!(both_out.status.code(), Some(2), "{both_out:?}");
    assert!(
        String::from_utf8_lossy(&both_out.stderr).contains("expected either"),
        "{both_out:?}"
    );

    let empty_dir = base.join("empty");
    fs::create_dir_all(&empty_dir).unwrap();
    let empty_out = witforge().args(["check"]).arg(&empty_dir).output().unwrap();
    assert_eq!(empty_out.status.code(), Some(2), "{empty_out:?}");

    // 6. `release` refuses a symlinked `released` manifest (WF9004, exit 2).
    #[cfg(unix)]
    {
        let sym_root = base.join("sym-release");
        let sym_v01 = sym_root.join("v0.1");
        fs::create_dir_all(&sym_v01).unwrap();
        fs::write(
            sym_v01.join("contract.wit"),
            "package ex:sym@0.1.0;\ninterface a { record r { id: u32 } }\n",
        )
        .unwrap();
        assert!(
            witforge()
                .args(["lock", sym_v01.to_str().unwrap(), "--first"])
                .output()
                .unwrap()
                .status
                .success()
        );
        let real_manifest = base.join("external-released");
        fs::write(&real_manifest, "").unwrap();
        std::os::unix::fs::symlink(&real_manifest, sym_root.join("released")).unwrap();
        let sym_out = witforge()
            .args(["release", sym_v01.to_str().unwrap(), "--format", "json"])
            .output()
            .unwrap();
        assert_eq!(sym_out.status.code(), Some(2), "{sym_out:?}");
        let sym_json: serde_json::Value = serde_json::from_slice(&sym_out.stdout).unwrap();
        assert_eq!(sym_json["diagnostics"][0]["code"], "WF9004", "{sym_json}");
    }

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn build_identity_quiet_mode_completions_and_explain() {
    // 1. `--version` carries commit identity on non-release-tag workspace builds and no timestamp.
    let version = witforge().arg("--version").output().unwrap();
    assert!(version.status.success(), "{version:?}");
    let version_text = String::from_utf8_lossy(&version.stdout);
    assert!(
        version_text.contains(env!("CARGO_PKG_VERSION")),
        "{version_text}"
    );
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let head_sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(manifest_dir)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string());
    let tags = Command::new("git")
        .args(["tag", "--points-at", "HEAD"])
        .current_dir(manifest_dir)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default();
    let on_release_tag = tags
        .lines()
        .any(|t| t.trim() == format!("v{}", env!("CARGO_PKG_VERSION")));
    if let Some(sha) = head_sha
        && !sha.is_empty()
        && !on_release_tag
    {
        assert!(
            version_text.contains(&format!("({sha}")),
            "expected commit `{sha}` in `--version`, got: {version_text}"
        );
    }
    assert!(
        !version_text.contains("UTC")
            && !version_text.contains("T0")
            && !version_text.contains("T1")
            && !version_text.contains("T2"),
        "version must not embed a timestamp: {version_text}"
    );

    // 2. `--quiet` / `-q` suppresses human notes and summaries while preserving errors, exit codes,
    // and JSON/SARIF output verbatim.
    let base = std::env::temp_dir().join(format!("witforge-w10-085-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let wit = base.join("wit");
    let v01 = wit.join("v0.1");
    fs::create_dir_all(&v01).unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package ex:quiet@0.1.0;\ninterface types { record item { id: u32, old: string } }\n",
    )
    .unwrap();

    // `lock --first --quiet` suppresses human notes on stdout.
    let lock_quiet = witforge()
        .args(["lock", v01.to_str().unwrap(), "--first", "--quiet"])
        .output()
        .unwrap();
    assert!(lock_quiet.status.success(), "{lock_quiet:?}");
    assert!(
        lock_quiet.stdout.is_empty(),
        "quiet lock must suppress notes: {}",
        String::from_utf8_lossy(&lock_quiet.stdout)
    );

    // `lock --first -q --format json` and `--format sarif` keep every diagnostic note intact.
    fs::remove_file(v01.join("wire.lock")).unwrap();
    let lock_json_loud = witforge()
        .args(["lock", v01.to_str().unwrap(), "--first", "--format", "json"])
        .output()
        .unwrap();
    fs::remove_file(v01.join("wire.lock")).unwrap();
    let lock_json_quiet = witforge()
        .args([
            "lock",
            v01.to_str().unwrap(),
            "--first",
            "-q",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(lock_json_quiet.stdout, lock_json_loud.stdout);
    let lock_json: serde_json::Value = serde_json::from_slice(&lock_json_quiet.stdout).unwrap();
    assert!(
        lock_json["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "WF1201"),
        "{lock_json}"
    );

    fs::remove_file(v01.join("wire.lock")).unwrap();
    let lock_sarif_loud = witforge()
        .args([
            "lock",
            v01.to_str().unwrap(),
            "--first",
            "--format",
            "sarif",
        ])
        .output()
        .unwrap();
    fs::remove_file(v01.join("wire.lock")).unwrap();
    let lock_sarif_quiet = witforge()
        .args([
            "lock",
            v01.to_str().unwrap(),
            "--first",
            "--quiet",
            "--format",
            "sarif",
        ])
        .output()
        .unwrap();
    assert_eq!(lock_sarif_quiet.stdout, lock_sarif_loud.stdout);

    // `release --quiet` and `new-version --quiet` suppress stdout summaries.
    let rel_q = witforge()
        .args(["release", v01.to_str().unwrap(), "--quiet"])
        .output()
        .unwrap();
    assert!(rel_q.status.success(), "{rel_q:?}");
    assert!(rel_q.stdout.is_empty(), "{rel_q:?}");
    let nv_q = witforge()
        .args(["new-version", wit.to_str().unwrap(), "-q"])
        .output()
        .unwrap();
    assert!(nv_q.status.success(), "{nv_q:?}");
    assert!(nv_q.stdout.is_empty(), "{nv_q:?}");

    // Modify v0.2 to drop `old` (triggers WF3003 conversion-impact note) and leave unstamped to
    // verify `check --quiet` STILL prints errors on stderr and exits 1.
    let v02 = wit.join("v0.2");
    fs::write(
        v02.join("contract.wit"),
        "package ex:quiet@0.2.0;\ninterface types { record item { id: u32 } }\n",
    )
    .unwrap();
    let check_err_q = witforge()
        .args(["check", v02.to_str().unwrap(), "--quiet"])
        .output()
        .unwrap();
    assert_eq!(check_err_q.status.code(), Some(1), "{check_err_q:?}");
    assert!(
        !check_err_q.stderr.is_empty(),
        "quiet mode must never suppress errors"
    );

    assert!(
        witforge()
            .args([
                "lock",
                v02.to_str().unwrap(),
                "--against",
                v01.join("wire.lock").to_str().unwrap(),
                "-q",
            ])
            .output()
            .unwrap()
            .status
            .success()
    );

    // `diff --quiet` omits `summary:`, `result:`, and `note:` while keeping change lines and
    // preserving `--check` errors on stderr and exit 1.
    let diff_q = witforge()
        .args([
            "diff",
            v01.to_str().unwrap(),
            v02.to_str().unwrap(),
            "--quiet",
        ])
        .output()
        .unwrap();
    assert!(diff_q.status.success(), "{diff_q:?}");
    let diff_q_out = String::from_utf8_lossy(&diff_q.stdout);
    assert!(diff_q_out.contains("types/item.old"), "{diff_q_out}");
    assert!(!diff_q_out.contains("summary:"), "{diff_q_out}");
    assert!(!diff_q_out.contains("result:"), "{diff_q_out}");
    assert!(!diff_q_out.contains("note:"), "{diff_q_out}");

    let diff_q_check = witforge()
        .args([
            "diff",
            v01.to_str().unwrap(),
            v02.to_str().unwrap(),
            "--check",
            "-q",
        ])
        .output()
        .unwrap();
    assert_eq!(diff_q_check.status.code(), Some(1), "{diff_q_check:?}");
    assert!(
        String::from_utf8_lossy(&diff_q_check.stderr).contains("--check failed"),
        "{diff_q_check:?}"
    );

    // `diff --json --quiet` and `diff --format sarif --quiet` are byte-identical to non-quiet runs.
    let diff_json_loud = witforge()
        .args([
            "diff",
            v01.to_str().unwrap(),
            v02.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    let diff_json_quiet = witforge()
        .args([
            "diff",
            v01.to_str().unwrap(),
            v02.to_str().unwrap(),
            "--json",
            "--quiet",
        ])
        .output()
        .unwrap();
    assert_eq!(diff_json_quiet.stdout, diff_json_loud.stdout);
    let diff_sarif_loud = witforge()
        .args([
            "diff",
            v01.to_str().unwrap(),
            v02.to_str().unwrap(),
            "--format",
            "sarif",
        ])
        .output()
        .unwrap();
    let diff_sarif_quiet = witforge()
        .args([
            "diff",
            v01.to_str().unwrap(),
            v02.to_str().unwrap(),
            "--format",
            "sarif",
            "--quiet",
        ])
        .output()
        .unwrap();
    assert_eq!(diff_sarif_quiet.stdout, diff_sarif_loud.stdout);

    // 3. `witforge completions bash|zsh|fish` covers all commands, global flags, and enum values.
    let expected_subcommands = [
        "check",
        "lock",
        "rust",
        "proto",
        "schema",
        "glue",
        "generate",
        "new-version",
        "release",
        "diff",
        "freeze",
        "completions",
        "explain",
    ];
    for shell in ["bash", "zsh", "fish"] {
        let out = witforge().args(["completions", shell]).output().unwrap();
        assert!(out.status.success(), "{shell}: {out:?}");
        let script = String::from_utf8(out.stdout).unwrap();
        assert!(!script.is_empty(), "{shell} empty");
        assert!(!script.contains('\r'), "{shell} must be LF-only");
        for sub in expected_subcommands {
            assert!(
                script.contains(sub),
                "{shell} completion missing subcommand `{sub}`"
            );
        }
        for token in [
            "format", "quiet", "human", "json", "sarif", "bash", "zsh", "fish",
        ] {
            assert!(
                script.contains(token),
                "{shell} completion missing `{token}`"
            );
        }
        #[cfg(unix)]
        if shell == "bash" {
            let syntax = Command::new("bash").args(["-n", "-c", &script]).output();
            if let Ok(syntax) = syntax {
                assert!(
                    syntax.status.success(),
                    "generated bash completion failed `bash -n`: {syntax:?}"
                );
            }
        }
    }
    let bad_shell = witforge()
        .args(["completions", "powershell", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(bad_shell.status.code(), Some(2), "{bad_shell:?}");
    let bad_shell_json: serde_json::Value = serde_json::from_slice(&bad_shell.stdout).unwrap();
    assert_eq!(bad_shell_json["diagnostics"][0]["code"], "WF9001");

    // 4. `witforge explain <CODE>` works for every code in CATALOG (case-insensitively) and
    // refuses unknown codes with exit 2 / WF9001.
    for info in witforge::diagnostic::CATALOG {
        let out = witforge().args(["explain", info.code]).output().unwrap();
        assert!(out.status.success(), "{}: {out:?}", info.code);
        let text = String::from_utf8_lossy(&out.stdout);
        let expected_meaning = witforge::diagnostic::explanation(info.code).unwrap();
        assert!(text.contains(info.code), "{}: {text}", info.code);
        assert!(text.contains(expected_meaning), "{}: {text}", info.code);
        assert!(
            text.contains(&witforge::diagnostic::help_uri(info.code)),
            "{}: {text}",
            info.code
        );
        if let Some(prior) = info.supersedes {
            assert!(
                text.contains(&format!("Prior code: {prior}")),
                "{}: {text}",
                info.code
            );
        }
    }
    let lower = witforge().args(["explain", "wf1015"]).output().unwrap();
    assert!(lower.status.success(), "{lower:?}");
    assert!(
        String::from_utf8_lossy(&lower.stdout).contains("WF1015 (error)"),
        "{lower:?}"
    );
    let unknown = witforge()
        .args(["explain", "WF9999", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(2), "{unknown:?}");
    let unknown_json: serde_json::Value = serde_json::from_slice(&unknown.stdout).unwrap();
    assert_eq!(unknown_json["diagnostics"][0]["code"], "WF9001");

    fs::remove_dir_all(&base).unwrap();
}
