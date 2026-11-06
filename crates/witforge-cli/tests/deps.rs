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
        rust.contains("crate::witforge_deps::example_shared::types::Item"),
        "{rust}"
    );
    let proto = fs::read_to_string(out.join("proto/v0.proto")).unwrap();
    assert!(proto.contains("example.shared.TypesItem"), "{proto}");
    fs::remove_dir_all(&out).unwrap();
}
