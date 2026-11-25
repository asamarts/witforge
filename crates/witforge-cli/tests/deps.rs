//! CLI integration with the standard `wit/deps` layout.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn witforge() -> Command {
    Command::new(env!("CARGO_BIN_EXE_witforge"))
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../witforge/tests/fixtures/deps/wit")
}

fn duplicate_version_graph(root: &Path) {
    fs::create_dir_all(root.join("v0.1/deps/shared-a")).unwrap();
    fs::create_dir_all(root.join("v0.1/deps/shared-b")).unwrap();
    fs::write(
        root.join("v0.1/deps/shared-a/types.wit"),
        "package example:shared@0.1.0;\ninterface types { record item { a: u32 } }\n",
    )
    .unwrap();
    fs::write(
        root.join("v0.1/deps/shared-b/types.wit"),
        "package example:shared@1.0.0;\ninterface types { enum item { x, y } }\n",
    )
    .unwrap();
    fs::write(
        root.join("v0.1/contract.wit"),
        "package example:root@0.1.0;\ninterface api {\n\
         use example:shared/types@0.1.0.{item as old-item};\n\
         use example:shared/types@1.0.0.{item as new-item};\n\
         record r { x: old-item, y: new-item }\n}\n",
    )
    .unwrap();
}

fn root_name_graph(root: &Path) {
    fs::create_dir_all(root.join("v0.1/deps/self")).unwrap();
    fs::write(
        root.join("v0.1/contract.wit"),
        "package example:root@0.1.0;\ninterface api { record item { a: u32 } }\n",
    )
    .unwrap();
    fs::write(
        root.join("v0.1/deps/self/types.wit"),
        "package example:root@0.1.0;\ninterface types { record item { b: u32 } }\n",
    )
    .unwrap();
}

fn assert_refused(name: &str, mut command: Command, expected: &str) {
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{name}: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(expected), "{name}: {stderr}");
}

#[test]
fn every_command_refuses_duplicate_or_root_named_dependencies() {
    for (case, write, expected) in [
        (
            "duplicate-version",
            duplicate_version_graph as fn(&Path),
            "duplicate dependency package `example:shared`",
        ),
        (
            "root-name",
            root_name_graph as fn(&Path),
            "dependency package `example:root` has the root package's namespace:name",
        ),
    ] {
        let root = std::env::temp_dir().join(format!(
            "witforge-deps-refusal-{case}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        write(&root);
        // `diff` checks for a lockfile before it loads the contract; the graph check runs in
        // `load`, so a syntactically valid placeholder is enough to reach it.
        fs::write(root.join("v0.1/wire.lock"), "# witforge wire lockfile v2\n").unwrap();
        let out = std::env::temp_dir().join(format!(
            "witforge-deps-refusal-out-{case}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&out);

        for (name, command) in [
            ("check", {
                let mut command = witforge();
                command.arg("check").arg(root.join("v0.1"));
                command
            }),
            ("lock", {
                let mut command = witforge();
                command.arg("lock").arg(root.join("v0.1")).arg("--first");
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
            ("diff", {
                let mut command = witforge();
                command
                    .arg("diff")
                    .arg(root.join("v0.1"))
                    .arg(root.join("v0.1"));
                command
            }),
        ] {
            assert_refused(&format!("{case}: {name}"), command, expected);
        }
        fs::remove_dir_all(&root).unwrap();
        let _ = fs::remove_dir_all(&out);
    }
}

#[test]
fn dependency_contract_checks_and_emits_rust_and_proto() {
    let root = fixture();
    let check = witforge()
        .arg("check")
        .arg(root.join("v0.1"))
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "check failed: {}",
        String::from_utf8_lossy(&check.stderr)
    );

    let out = std::env::temp_dir().join(format!("witforge-deps-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    for command in ["rust", "proto"] {
        let output = witforge()
            .arg(command)
            .arg(&root)
            .arg("--out")
            .arg(out.join(command))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{command} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let rust = fs::read_to_string(out.join("rust/v0_1.rs")).unwrap();
    assert!(
        rust.contains("super::super::witforge_deps::example_shared::types::Item"),
        "{rust}"
    );
    let proto = fs::read_to_string(out.join("proto/v0.proto")).unwrap();
    assert!(proto.contains("example.shared.v0.TypesItem"), "{proto}");

    let deps_source = out.join("rust/witforge_deps.rs");
    let version_source = out.join("rust/v0_1.rs");
    let wrapper = out.join("generated.rs");
    fs::write(
        &wrapper,
        format!(
            "mod witforge_deps {{ include!({deps_source:?}); }}\nmod v0_1 {{ include!({version_source:?}); }}\n"
        ),
    )
    .unwrap();
    let metadata = out.join("generated.rmeta");
    let status = Command::new("rustc")
        .args([
            "--edition=2024",
            "--crate-type=lib",
            "--emit=metadata",
            "-o",
        ])
        .arg(&metadata)
        .arg(&wrapper)
        .status()
        .unwrap();
    assert!(status.success(), "dependency Rust bindings did not compile");

    fs::remove_dir_all(&out).unwrap();
}

#[test]
fn check_against_compares_the_pinned_dependency_graph() {
    let root =
        std::env::temp_dir().join(format!("witforge-deps-check-graph-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for (version, alias_target) in [("v0.1", "u32"), ("v0.2", "u64")] {
        let shared = root.join(version).join("deps/shared");
        fs::create_dir_all(&shared).unwrap();
        fs::write(
            shared.join("types.wit"),
            format!(
                "package example:shared@0.1.0;\ninterface types {{ type id = {alias_target}; }}\n"
            ),
        )
        .unwrap();
        fs::write(
            root.join(version).join("contract.wit"),
            format!(
                "package example:root@{}.0;\ninterface api {{\n\
                   use example:shared/types@0.1.0.{{id}};\n\
                   record r {{ a: id }}\n\
                 }}\n",
                &version[1..]
            ),
        )
        .unwrap();
    }
    let old = root.join("v0.1");
    let new = root.join("v0.2");
    let first = witforge()
        .arg("lock")
        .arg(&old)
        .arg("--first")
        .output()
        .unwrap();
    assert!(first.status.success(), "{first:?}");
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();
    let lock = witforge()
        .arg("lock")
        .arg(&new)
        .arg("--against")
        .arg(old.join("wire.lock"))
        .output()
        .unwrap();
    assert!(lock.status.success(), "{lock:?}");

    let check = witforge()
        .arg("check")
        .arg(&new)
        .arg("--against")
        .arg(old.join("wire.lock"))
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(stderr.contains("dependency graph differs"), "{stderr}");

    // A copied baseline has no WIT beside it; check says the graph cannot be compared.
    let copied = root.join("baseline.lock");
    fs::copy(old.join("wire.lock"), &copied).unwrap();
    let output = witforge()
        .arg("check")
        .arg(&new)
        .arg("--against")
        .arg(&copied)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("cannot compare the pinned dependency graph"),
        "{output:?}"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn deps_sync_updates_only_unreleased_versions_per_major_checks_drift_and_refuses_symlinks() {
    let base = std::env::temp_dir().join(format!("witforge-deps-sync-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let wit = base.join("wit");

    // Layout:
    // - v0.1 (released): deps/shared/types.wit -> u32
    // - v0.2 (unreleased, drifted): deps/shared/types.wit -> u64, plus extra stale file
    // - v0.3 (unreleased, missing deps/ entirely)
    // - v1.0 (released, major upgrade): deps/shared/types.wit -> string (v1.0.0)
    // - v1.1 (unreleased, drifted from v1.0): deps/shared/types.wit -> bool
    let write_ver = |ver: &str, dep_ver: &str, dep_field: Option<&str>| {
        let dir = wit.join(ver);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("contract.wit"),
            format!(
                "package example:sync@{}.0;\n\
                 interface api {{\n\
                   use example:shared/types@{dep_ver}.{{id}};\n\
                   record r {{ a: id }}\n\
                 }}\n",
                &ver[1..]
            ),
        )
        .unwrap();
        if let Some(field) = dep_field {
            let dep_dir = dir.join("deps/shared");
            fs::create_dir_all(&dep_dir).unwrap();
            fs::write(
                dep_dir.join("types.wit"),
                format!(
                    "package example:shared@{dep_ver};\ninterface types {{ type id = {field}; }}\n"
                ),
            )
            .unwrap();
        }
    };

    write_ver("v0.1", "0.1.0", Some("u32"));
    write_ver("v0.2", "0.1.0", Some("u64"));
    fs::write(wit.join("v0.2/deps/shared/stale.wit"), "// stale\n").unwrap();
    write_ver("v0.3", "0.1.0", None);
    write_ver("v1.0", "1.0.0", Some("string"));
    write_ver("v1.1", "1.0.0", Some("bool"));
    for rel_ver in ["v0.1", "v1.0"] {
        let locked = witforge()
            .args(["lock"])
            .arg(wit.join(rel_ver))
            .arg("--first")
            .output()
            .unwrap();
        assert!(locked.status.success(), "{rel_ver}: {locked:?}");
    }
    fs::write(wit.join("released"), "v0.1\nv1.0\n").unwrap();

    let v01_before = fs::read_to_string(wit.join("v0.1/deps/shared/types.wit")).unwrap();
    let v10_before = fs::read_to_string(wit.join("v1.0/deps/shared/types.wit")).unwrap();

    // 1. `--check` reports drift across v0.2, v0.3, and v1.1 with WF1201 (exit 1) and writes nothing.
    let check_drift = witforge()
        .args(["deps", "sync"])
        .arg(&wit)
        .args(["--check", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(check_drift.status.code(), Some(1), "{check_drift:?}");
    let drift_json: serde_json::Value = serde_json::from_slice(&check_drift.stdout).unwrap();
    let diags = drift_json["diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), 3, "{drift_json}");
    for diag in diags {
        assert_eq!(diag["code"], "WF1033", "{diag}");
    }
    assert!(!wit.join("v0.3/deps").exists(), "--check must not write");
    assert!(wit.join("v0.2/deps/shared/stale.wit").exists());

    // 2. `witforge deps sync wit` synchronizes v0.2 & v0.3 from v0.1 AND v1.1 from v1.0,
    // preserving released v0.1 and v1.0 byte-for-byte without cross-major overwrite!
    let sync_ok = witforge()
        .args(["deps", "sync"])
        .arg(&wit)
        .output()
        .unwrap();
    assert!(sync_ok.status.success(), "{sync_ok:?}");
    let sync_stdout = String::from_utf8_lossy(&sync_ok.stdout);
    assert!(
        sync_stdout.contains("synced v0.1/deps -> v0.2/deps"),
        "{sync_stdout}"
    );
    assert!(
        sync_stdout.contains("synced v0.1/deps -> v0.3/deps"),
        "{sync_stdout}"
    );
    assert!(
        sync_stdout.contains("synced v1.0/deps -> v1.1/deps"),
        "{sync_stdout}"
    );

    assert_eq!(
        fs::read_to_string(wit.join("v0.2/deps/shared/types.wit")).unwrap(),
        v01_before
    );
    assert!(!wit.join("v0.2/deps/shared/stale.wit").exists());
    assert_eq!(
        fs::read_to_string(wit.join("v0.3/deps/shared/types.wit")).unwrap(),
        v01_before
    );
    assert_eq!(
        fs::read_to_string(wit.join("v1.1/deps/shared/types.wit")).unwrap(),
        v10_before
    );
    assert_eq!(
        fs::read_to_string(wit.join("v0.1/deps/shared/types.wit")).unwrap(),
        v01_before
    );
    assert_eq!(
        fs::read_to_string(wit.join("v1.0/deps/shared/types.wit")).unwrap(),
        v10_before
    );

    // 3. `--check` now passes cleanly (exit 0)!
    let check_clean = witforge()
        .args(["deps", "sync"])
        .arg(&wit)
        .arg("--check")
        .output()
        .unwrap();
    assert!(check_clean.status.success(), "{check_clean:?}");

    // 4. `--from v0.2` after modifying v0.2/deps updates ONLY unreleased v0.3 (NEVER released v0.1,
    // and NEVER major 1's v1.1)!
    let custom_dep = "package example:shared@0.1.0;\ninterface types { type id = u16; }\n";
    fs::write(wit.join("v0.2/deps/shared/types.wit"), custom_dep).unwrap();
    let sync_from = witforge()
        .args(["deps", "sync"])
        .arg(&wit)
        .args(["--from", "v0.2"])
        .output()
        .unwrap();
    assert!(sync_from.status.success(), "{sync_from:?}");
    assert_eq!(
        fs::read_to_string(wit.join("v0.3/deps/shared/types.wit")).unwrap(),
        custom_dep
    );
    assert_eq!(
        fs::read_to_string(wit.join("v0.1/deps/shared/types.wit")).unwrap(),
        v01_before,
        "released v0.1 must NEVER be modified"
    );
    assert_eq!(
        fs::read_to_string(wit.join("v1.1/deps/shared/types.wit")).unwrap(),
        v10_before,
        "major 1 must not be touched by --from v0.2"
    );

    // 5. Removing `deps/` in `--from v0.2` removes stale `deps/` in unreleased v0.3 cleanly.
    fs::remove_dir_all(wit.join("v0.2/deps")).unwrap();
    let sync_remove = witforge()
        .args(["deps", "sync"])
        .arg(&wit)
        .args(["--from", "v0.2"])
        .output()
        .unwrap();
    assert!(sync_remove.status.success(), "{sync_remove:?}");
    assert!(!wit.join("v0.3/deps").exists());
    assert!(wit.join("v0.1/deps").is_dir());

    // 6. Config-file discovery works when WIT_ROOT is omitted.
    fs::write(
        base.join("witforge.toml"),
        "version = 1\n[project.sync]\nroot = \"wit\"\n",
    )
    .unwrap();
    let cfg_sync = witforge()
        .current_dir(&base)
        .args(["deps", "sync"])
        .output()
        .unwrap();
    assert!(cfg_sync.status.success(), "{cfg_sync:?}");
    assert_eq!(
        fs::read_to_string(wit.join("v0.2/deps/shared/types.wit")).unwrap(),
        v01_before
    );
    assert_eq!(
        fs::read_to_string(wit.join("v0.3/deps/shared/types.wit")).unwrap(),
        v01_before
    );

    // 7. Symlink guard (`WF9004`, exit 2) refuses symlinks inside target `deps/`.
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let link_path = wit.join("v0.3/deps/shared/evil.wit");
        symlink("types.wit", &link_path).unwrap();
        let sym_err = witforge()
            .args(["deps", "sync"])
            .arg(&wit)
            .args(["--format", "json"])
            .output()
            .unwrap();
        assert_eq!(sym_err.status.code(), Some(2), "{sym_err:?}");
        let sym_json: serde_json::Value = serde_json::from_slice(&sym_err.stdout).unwrap();
        assert_eq!(sym_json["diagnostics"][0]["code"], "WF9004", "{sym_json}");
        fs::remove_file(&link_path).unwrap();
    }

    // 8. Retired `--from` version is refused with WF9004 (exit 2).
    fs::write(wit.join("released"), "v0.1 retired\nv1.0\n").unwrap();
    let retired_err = witforge()
        .args(["deps", "sync"])
        .arg(&wit)
        .args(["--from", "v0.1", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(retired_err.status.code(), Some(2), "{retired_err:?}");
    let retired_json: serde_json::Value = serde_json::from_slice(&retired_err.stdout).unwrap();
    assert_eq!(
        retired_json["diagnostics"][0]["code"], "WF9004",
        "{retired_json}"
    );

    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn eighth_wave_audit_deps_sync_empty_subdir_preflight_and_fix_hint() {
    let base =
        std::env::temp_dir().join(format!("witforge-deps-sync-audit-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let wit = base.join("wit");

    for (ver, field) in [("v0.1", "u32"), ("v0.2", "u32"), ("v0.3", "u32")] {
        let dir = wit.join(ver);
        let dep_dir = dir.join("deps/shared");
        fs::create_dir_all(&dep_dir).unwrap();
        fs::write(
            dir.join("contract.wit"),
            format!(
                "package example:syncaudit@{}.0;\n\
                 interface api {{\n\
                   use example:shared/types@0.1.0.{{id}};\n\
                   record r {{ a: id }}\n\
                 }}\n",
                &ver[1..]
            ),
        )
        .unwrap();
        fs::write(
            dep_dir.join("types.wit"),
            format!("package example:shared@0.1.0;\ninterface types {{ type id = {field}; }}\n"),
        )
        .unwrap();
    }
    let locked = witforge()
        .args(["lock"])
        .arg(wit.join("v0.1"))
        .arg("--first")
        .output()
        .unwrap();
    assert!(locked.status.success(), "{locked:?}");
    let custom_rel = wit.join("custom-released");
    fs::write(&custom_rel, "v0.1\n").unwrap();

    // 1. Stale empty subdirectory `v0.2/deps/old-pkg/` is detected by `--check` AND cleaned up by
    // `deps sync`, and `--check` fix-it hint preserves `--from` and `--released`!
    fs::create_dir_all(wit.join("v0.2/deps/old-pkg")).unwrap();
    let check_empty = witforge()
        .args(["deps", "sync"])
        .arg(&wit)
        .args(["--from", "v0.1", "--released"])
        .arg(&custom_rel)
        .arg("--check")
        .output()
        .unwrap();
    assert_eq!(check_empty.status.code(), Some(1), "{check_empty:?}");
    let stderr = String::from_utf8_lossy(&check_empty.stderr);
    assert!(
        stderr.contains("extra deps/old-pkg/"),
        "expected empty subdir drift in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("--from v0.1") && stderr.contains("--released"),
        "expected --from and --released preserved in fix-it hint:\n{stderr}"
    );

    let sync_clean = witforge()
        .args(["deps", "sync"])
        .arg(&wit)
        .args(["--from", "v0.1", "--released"])
        .arg(&custom_rel)
        .output()
        .unwrap();
    assert!(sync_clean.status.success(), "{sync_clean:?}");
    assert!(
        !wit.join("v0.2/deps/old-pkg").exists(),
        "stale empty subdir must be removed on sync"
    );

    // 2. Two-phase preflight: drift in v0.2 + symlink in v0.3 MUST fail with WF9004 without
    // modifying v0.2!
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let drifted = "package example:shared@0.1.0;\ninterface types { type id = u64; }\n";
        fs::write(wit.join("v0.2/deps/shared/types.wit"), drifted).unwrap();
        let bad_link = wit.join("v0.3/deps/shared/evil.wit");
        symlink("types.wit", &bad_link).unwrap();

        let preflight_err = witforge()
            .args(["deps", "sync"])
            .arg(&wit)
            .args(["--released"])
            .arg(&custom_rel)
            .args(["--format", "json"])
            .output()
            .unwrap();
        assert_eq!(preflight_err.status.code(), Some(2), "{preflight_err:?}");
        let err_json: serde_json::Value = serde_json::from_slice(&preflight_err.stdout).unwrap();
        assert_eq!(err_json["diagnostics"][0]["code"], "WF9004");
        assert_eq!(
            fs::read_to_string(wit.join("v0.2/deps/shared/types.wit")).unwrap(),
            drifted,
            "preflight failure on later target v0.3 must not have mutated earlier target v0.2"
        );
        fs::remove_file(&bad_link).unwrap();
    }

    fs::remove_dir_all(&base).unwrap();
}
