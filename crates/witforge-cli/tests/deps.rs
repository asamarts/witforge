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
