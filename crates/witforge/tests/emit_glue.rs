//! Protobuf glue type-conversion tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::emit::{glue, proto, rust};
use witforge::load;
use witforge::lock::WireLock;
use witforge::model;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn lock_for(model: &model::Model) -> WireLock {
    let members = model.members().unwrap();
    let mut lock = WireLock::default();
    lock.reconcile(&members).unwrap();
    lock
}

fn setup_contract(root: &Path) -> (model::Model, WireLock) {
    let model = model::build(&load(root).unwrap()).unwrap();
    let lock = lock_for(&model);
    (model, lock)
}

#[test]
fn emits_glue_error_and_type_modules() {
    let root = fixture("emit/glue/contract");
    let (model, lock) = setup_contract(&root);
    let version = glue::emit_version(&model, &lock, "crate::plain", "crate::proto").unwrap();
    let module = glue::emit_root(&model, &[0]).unwrap();
    for expected in [
        "pub enum GlueError",
        "#[non_exhaustive]",
        "IntegerOutOfRange",
        "MissingField",
        "UnknownEnumValue",
        "UnknownCase",
        "InvalidState",
        "impl ::core::fmt::Display for GlueError",
        "impl ::std::error::Error for GlueError",
    ] {
        assert!(
            module.contains(expected),
            "missing `{expected}` in:\n{module}"
        );
    }
    assert!(module.contains("pub mod v0;"), "{module}");
    for expected in [
        "pub mod small {",
        "pub mod numbers {",
        "pub mod status {",
        "pub mod choice {",
        "pub mod caps {",
        "pub mod small_list {",
        "pub mod maybe_small {",
        "pub mod maybe_list {",
        "pub mod maybe_maybe {",
        "pub mod outcome {",
        "pub mod all {",
        "pub mod singular {",
        "pub fn to_proto(",
        "pub fn from_proto(",
        "super::super::types::small::to_proto",
    ] {
        assert!(
            version.contains(expected),
            "missing `{expected}` in:\n{version}"
        );
    }
    assert!(!version.contains('\r'), "glue output contains CRLF");
    assert!(version.ends_with('\n'), "glue output has no final newline");
}

#[test]
fn conversions_compile_and_run_against_generated_types() {
    let root = fixture("emit/glue/contract");
    let (model, lock) = setup_contract(&root);
    let version = format!(
        "v{}_{}",
        model.package.version.major, model.package.version.minor
    );
    let major = model.package.version.major;
    let out = std::env::temp_dir().join(format!("witforge-glue-run-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(out.join("plain")).unwrap();
    fs::create_dir_all(out.join("proto")).unwrap();
    fs::create_dir_all(out.join("glue")).unwrap();
    fs::create_dir_all(out.join("proto-work")).unwrap();

    fs::write(out.join("plain/mod.rs"), format!("pub mod {version};\n")).unwrap();
    fs::write(
        out.join(format!("plain/{version}.rs")),
        rust::emit_version(&model).unwrap(),
    )
    .unwrap();
    fs::write(out.join("proto/mod.rs"), format!("pub mod v{major};\n")).unwrap();

    let schema = proto::emit_major(&model, &lock).unwrap();
    let proto_path = out.join("proto-work/schema.proto");
    fs::write(&proto_path, schema).unwrap();
    let descriptors = protox::compile([&proto_path], [out.join("proto-work")]).unwrap();
    let prost_out = out.join("proto-work/out");
    fs::create_dir_all(&prost_out).unwrap();
    prost_build::Config::new()
        .out_dir(&prost_out)
        .compile_fds(descriptors)
        .unwrap();
    let generated = fs::read_dir(&prost_out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .expect("prost generated a Rust file");
    fs::copy(generated, out.join(format!("proto/v{major}.rs"))).unwrap();

    fs::write(
        out.join("glue/mod.rs"),
        glue::emit_root(&model, &[major]).unwrap(),
    )
    .unwrap();
    fs::write(
        out.join(format!("glue/v{major}.rs")),
        glue::emit_version(&model, &lock, "crate::plain", "crate::proto").unwrap(),
    )
    .unwrap();
    let mut main = String::from("mod glue;\nmod plain;\nmod proto;\n\n");
    main.push_str(&fs::read_to_string(root.join("main.rs")).unwrap());
    fs::write(out.join("main.rs"), main).unwrap();

    let executable = out.join("glue-run");
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let mut candidates: Vec<PathBuf> = fs::read_dir(&deps)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_string_lossy();
            (name.starts_with("libprost-") && name.ends_with(".rlib")).then_some(path)
        })
        .collect();
    candidates.sort();
    let mut last_error = None;
    for prost in candidates {
        let output = Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-type=bin",
                "-L",
                &format!("dependency={}", deps.display()),
                "--extern",
                &format!("prost={}", prost.display()),
                "-o",
            ])
            .arg(&executable)
            .arg(out.join("main.rs"))
            .output()
            .unwrap();
        if output.status.success() {
            let run = Command::new(&executable).output().unwrap();
            assert!(
                run.status.success(),
                "generated glue program failed:\n{}",
                String::from_utf8_lossy(&run.stderr)
            );
            fs::remove_dir_all(&out).unwrap();
            return;
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("E0514"),
            "generated glue did not compile:\n{stderr}"
        );
        last_error = Some(stderr.into_owned());
    }
    panic!("no usable prost rlib found: {last_error:?}");
}

#[test]
fn functions_module_and_empty_roots_are_refused() {
    let dir = std::env::temp_dir().join(format!("witforge-glue-functions-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("contract.wit"),
        "package example:reserved-module@0.1.0;\n\
interface types {\n\
  record functions { value: string }\n\
}\n",
    )
    .unwrap();
    let model = model::build(&load(&dir).unwrap()).unwrap();
    let lock = lock_for(&model);
    let error = glue::emit_version(&model, &lock, "crate::plain", "crate::proto").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("reserved glue module `functions`"),
        "{error}"
    );
    assert!(glue::emit_version(&model, &lock, "", "crate::proto").is_err());
    assert!(glue::emit_version(&model, &lock, "crate::plain", "").is_err());
    fs::remove_dir_all(&dir).unwrap();
}
