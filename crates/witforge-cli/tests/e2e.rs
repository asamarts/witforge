//! End-to-end coverage for the representative alint plugin contract.

use std::fs;
use std::path::Path;
use std::process::Command;

mod support;
use support::{ScratchDir, compile_and_run_generated, copy_dir_tree};

fn witforge() -> Command {
    Command::new(env!("CARGO_BIN_EXE_witforge"))
}

#[allow(clippy::too_many_lines)]
#[test]
fn alint_contract_generates_compiles_and_roundtrips() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../witforge/tests/fixtures/contracts/alint-plugin");
    let scratch = ScratchDir::new("alint-e2e");
    let temp = scratch.path();
    let version = temp.join("wit/v0.1");
    fs::create_dir_all(&version).unwrap();
    copy_dir_tree(&source, &version);

    let wit_root = temp.join("wit");
    let check = witforge().arg("check").arg(&version).output().unwrap();
    assert!(
        check.status.success(),
        "alint check failed: {}",
        String::from_utf8_lossy(&check.stderr)
    );
    let lock = witforge().arg("lock").arg(&version).output().unwrap();
    assert!(
        lock.status.success(),
        "alint lock failed: {}",
        String::from_utf8_lossy(&lock.stderr)
    );

    let plain = temp.join("plain");
    let proto = temp.join("proto");
    let glue = temp.join("glue");
    let rust = witforge()
        .arg("rust")
        .arg(temp.join("wit"))
        .arg("--out")
        .arg(&plain)
        .output()
        .unwrap();
    assert!(rust.status.success(), "{rust:?}");
    let proto_output = witforge()
        .arg("proto")
        .arg(temp.join("wit"))
        .arg("--out")
        .arg(&proto)
        .output()
        .unwrap();
    assert!(proto_output.status.success(), "{proto_output:?}");
    let glue_output = witforge()
        .arg("glue")
        .arg(temp.join("wit"))
        .arg("--out")
        .arg(&glue)
        .arg("--plain-root")
        .arg("crate::plain")
        .arg("--proto-root")
        .arg("crate::proto")
        .output()
        .unwrap();
    assert!(glue_output.status.success(), "{glue_output:?}");

    let checks = [
        vec![
            "rust",
            wit_root.to_str().unwrap(),
            "--out",
            plain.to_str().unwrap(),
            "--check",
        ],
        vec![
            "proto",
            wit_root.to_str().unwrap(),
            "--out",
            proto.to_str().unwrap(),
            "--check",
        ],
        vec![
            "glue",
            wit_root.to_str().unwrap(),
            "--out",
            glue.to_str().unwrap(),
            "--plain-root",
            "crate::plain",
            "--proto-root",
            "crate::proto",
            "--check",
        ],
    ];
    for args in checks {
        let output = witforge().args(&args).output().unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
    }

    let generated = |path: &Path| -> Vec<(String, Vec<u8>)> {
        fs::read_dir(path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    fs::read(entry.path()).unwrap(),
                )
            })
            .collect()
    };
    let before = (generated(&plain), generated(&proto), generated(&glue));
    let rerun = witforge()
        .arg("rust")
        .arg(temp.join("wit"))
        .arg("--out")
        .arg(&plain)
        .output()
        .unwrap();
    assert!(rerun.status.success(), "{rerun:?}");
    let rerun = witforge()
        .arg("proto")
        .arg(temp.join("wit"))
        .arg("--out")
        .arg(&proto)
        .output()
        .unwrap();
    assert!(rerun.status.success(), "{rerun:?}");
    let rerun = witforge()
        .arg("glue")
        .arg(temp.join("wit"))
        .arg("--out")
        .arg(&glue)
        .arg("--plain-root")
        .arg("crate::plain")
        .arg("--proto-root")
        .arg("crate::proto")
        .output()
        .unwrap();
    assert!(rerun.status.success(), "{rerun:?}");
    assert_eq!(
        before,
        (generated(&plain), generated(&proto), generated(&glue))
    );

    let main_body = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../witforge/tests/e2e/alint_main.rs"),
    )
    .unwrap();
    compile_and_run_generated(
        "alint-run",
        &plain,
        &proto,
        &glue,
        true,
        &main_body,
        "alint generated glue compile/run",
    );
    scratch.finish();
}
