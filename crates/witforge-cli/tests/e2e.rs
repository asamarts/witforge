//! End-to-end coverage for the representative alint plugin contract.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use prost::Message as _;

fn witforge() -> Command {
    Command::new(env!("CARGO_BIN_EXE_witforge"))
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

fn rlibs(deps: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(deps)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_string_lossy();
            (name.starts_with(&format!("lib{prefix}-")) && name.ends_with(".rlib")).then_some(path)
        })
        .collect();
    paths.sort();
    paths
}

#[allow(clippy::too_many_lines)]
#[test]
fn alint_contract_generates_compiles_and_roundtrips() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../witforge/tests/fixtures/contracts/alint-plugin");
    let temp = std::env::temp_dir().join(format!("witforge-alint-e2e-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp);
    let version = temp.join("wit/v0.1");
    fs::create_dir_all(&version).unwrap();
    copy_tree(&source, &version);

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

    let proto_path = proto.join("v0.proto");
    let descriptors = protox::compile([&proto_path], [&proto]).unwrap();
    let encoded = descriptors.encode_to_vec();
    let prost_out = temp.join("build/prost");
    fs::create_dir_all(&prost_out).unwrap();
    prost_build::Config::new()
        .out_dir(&prost_out)
        .compile_fds(descriptors)
        .unwrap();
    let prost_generated = fs::read_dir(&prost_out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .unwrap();
    let pbjson_out = temp.join("build/pbjson");
    fs::create_dir_all(&pbjson_out).unwrap();
    pbjson_build::Builder::new()
        .out_dir(&pbjson_out)
        .register_descriptors(&encoded)
        .unwrap()
        .build(&[".alint.plugin.v0"])
        .unwrap();
    let pbjson_generated = fs::read_dir(&pbjson_out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .unwrap();

    let crate_root = temp.join("crate");
    fs::create_dir_all(crate_root.join("plain")).unwrap();
    fs::create_dir_all(crate_root.join("proto")).unwrap();
    fs::create_dir_all(crate_root.join("glue")).unwrap();
    copy_tree(&plain, &crate_root.join("plain"));
    copy_tree(&glue, &crate_root.join("glue"));
    fs::copy(prost_generated, crate_root.join("proto/v0.rs")).unwrap();
    fs::copy(pbjson_generated, crate_root.join("proto/v0.serde.rs")).unwrap();
    fs::write(
        crate_root.join("proto/mod.rs"),
        "pub mod v0 {\n    include!(\"v0.rs\");\n    include!(\"v0.serde.rs\");\n}\n",
    )
    .unwrap();
    let mut main = String::from("mod glue;\nmod plain;\nmod proto;\n\n");
    main.push_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../witforge/tests/e2e/alint_main.rs"),
        )
        .unwrap(),
    );
    fs::write(crate_root.join("main.rs"), main).unwrap();

    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let executable = temp.join(format!("alint-e2e{}", std::env::consts::EXE_SUFFIX));
    let mut last_error = None;
    for prost in rlibs(&deps, "prost") {
        for pbjson in rlibs(&deps, "pbjson") {
            for serde in rlibs(&deps, "serde") {
                for serde_json in rlibs(&deps, "serde_json") {
                    let output = Command::new("rustc")
                        .args([
                            "--edition=2024",
                            "--crate-type=bin",
                            "-L",
                            &format!("dependency={}", deps.display()),
                            "--extern",
                            &format!("prost={}", prost.display()),
                            "--extern",
                            &format!("pbjson={}", pbjson.display()),
                            "--extern",
                            &format!("serde={}", serde.display()),
                            "--extern",
                            &format!("serde_json={}", serde_json.display()),
                            "-o",
                        ])
                        .arg(&executable)
                        .arg(crate_root.join("main.rs"))
                        .output()
                        .unwrap();
                    if output.status.success() {
                        let run = Command::new(&executable).output().unwrap();
                        assert!(
                            run.status.success(),
                            "alint e2e failed:\n{}",
                            String::from_utf8_lossy(&run.stderr)
                        );
                        fs::remove_dir_all(&temp).unwrap();
                        return;
                    }
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if stderr.contains("E0514") {
                        last_error = Some(stderr.into_owned());
                        continue;
                    }
                    panic!("alint glue did not compile:\n{stderr}");
                }
            }
        }
    }
    panic!("no usable prost/pbjson/serde rlib combination: {last_error:?}");
}
