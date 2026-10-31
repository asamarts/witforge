//! Protobuf glue type-conversion tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use prost::Message as _;
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
    let again = glue::emit_version(&model, &lock, "crate::plain", "crate::proto").unwrap();
    assert_eq!(version, again, "glue emission is not deterministic");
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
        "pub mod service {",
        "pub mod functions {",
        "pub mod ping {",
        "pub mod echo {",
        "pub mod many {",
        "pub mod no_result {",
        "pub mod maybe_result {",
        "pub mod list_result {",
        "pub const METHOD:",
        "pub fn to_request(",
        "pub fn from_request(",
        "pub fn to_response(",
        "pub fn from_response(",
    ] {
        assert!(
            version.contains(expected),
            "missing `{expected}` in:\n{version}"
        );
    }
    assert!(!version.contains('\r'), "glue output contains CRLF");
    assert!(version.ends_with('\n'), "glue output has no final newline");
}

#[allow(clippy::too_many_lines)]
#[test]
fn glue_output_matches_golden() {
    let root = fixture("emit/glue/contract");
    let (model, lock) = setup_contract(&root);
    let version = glue::emit_version(&model, &lock, "crate::plain", "crate::proto").unwrap();
    let expected = fs::read_to_string(root.join("expected.rs")).unwrap();
    assert_eq!(version, expected, "glue golden drifted");
    assert!(!version.contains('\r'), "glue output contains CRLF");
    assert!(version.ends_with('\n'), "glue output has no final newline");
}

#[allow(clippy::too_many_lines)]
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
    fs::write(
        out.join("proto/mod.rs"),
        format!(
            "pub mod v{major} {{\n    include!(\"v{major}.rs\");\n    include!(\"v{major}.serde.rs\");\n}}\n"
        ),
    )
    .unwrap();

    let schema = proto::emit_major(&model, &lock).unwrap();
    let proto_path = out.join("proto-work/schema.proto");
    fs::write(&proto_path, schema).unwrap();
    let descriptors = protox::compile([&proto_path], [out.join("proto-work")]).unwrap();
    let encoded_descriptors = descriptors.encode_to_vec();
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

    let pbjson_out = out.join("proto-work/pbjson");
    fs::create_dir_all(&pbjson_out).unwrap();
    pbjson_build::Builder::new()
        .out_dir(&pbjson_out)
        .register_descriptors(&encoded_descriptors)
        .unwrap()
        .build(&[".example.glue.v0"])
        .unwrap();
    let pbjson_generated = fs::read_dir(&pbjson_out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .expect("pbjson generated a Rust file");
    fs::copy(
        pbjson_generated,
        out.join(format!("proto/v{major}.serde.rs")),
    )
    .unwrap();

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

    let executable = out.join(format!("glue-run{}", std::env::consts::EXE_SUFFIX));
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let candidates = |prefix: &str| {
        let mut paths: Vec<PathBuf> = fs::read_dir(&deps)
            .unwrap()
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                let name = path.file_name()?.to_string_lossy();
                (name.starts_with(&format!("lib{prefix}-")) && name.ends_with(".rlib"))
                    .then_some(path)
            })
            .collect();
        paths.sort();
        paths
    };
    let prosts = candidates("prost");
    let pbjsons = candidates("pbjson");
    let serdes = candidates("serde");
    let serde_jsons = candidates("serde_json");
    let mut last_error = None;
    for prost in &prosts {
        for pbjson in &pbjsons {
            for serde in &serdes {
                for serde_json in &serde_jsons {
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
                    if stderr.contains("E0514") {
                        last_error = Some(stderr.into_owned());
                        continue;
                    }
                    panic!("generated glue did not compile:\n{stderr}");
                }
            }
        }
    }
    panic!("no usable prost/pbjson/serde rlib combination found: {last_error:?}");
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

#[test]
fn plan_uses_highest_minor_and_validates_module_roots() {
    let root = fixture("versions/wit");
    let files = glue::plan(&root, "crate::plain", "crate::proto").unwrap();
    assert_eq!(
        files.files().keys().cloned().collect::<Vec<_>>(),
        vec!["mod.rs".to_string(), "v0.rs".to_string()]
    );
    let module = files.files().get("mod.rs").unwrap();
    assert!(module.contains("pub mod v0;"), "{module}");
    assert!(!module.contains("v0_1"), "{module}");
    let version = files.files().get("v0.rs").unwrap();
    assert!(version.contains("example:evolve@0.2.0"), "{version}");
    assert!(version.contains("crate::plain::v0_2"), "{version}");
    assert!(version.contains("crate::proto::v0"), "{version}");
    assert!(!version.contains("crate::plain::v0_1"), "{version}");

    for valid in [
        "crate",
        "crate::generated",
        "self::plain",
        "super::proto",
        "super::super::nested",
        "crate::r#type",
    ] {
        glue::validate_module_root(valid).unwrap_or_else(|error| panic!("{valid}: {error}"));
    }
    for invalid in [
        "",
        "crate::",
        "::crate",
        "crate:::plain",
        "crate::foo::",
        "crate::foo::bar<T>",
        "crate ::plain",
        "crate::1plain",
        "crate::type",
        "crate::foo-bar",
        "crate::r#self",
        "crate::super",
        "crate::_",
        "crate::r#_",
    ] {
        assert!(
            glue::validate_module_root(invalid).is_err(),
            "accepted invalid module root `{invalid}`"
        );
    }
}

#[test]
fn glue_layout_check_and_ownership_match_other_emitters() {
    let root = fixture("versions/wit");
    let out = std::env::temp_dir().join(format!("witforge-glue-layout-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out).unwrap();
    let unrelated = out.join("README.txt");
    fs::write(&unrelated, "keep me\n").unwrap();

    glue::generate(&root, &out, "crate::plain", "crate::proto").unwrap();
    assert!(out.join("mod.rs").is_file());
    assert!(out.join("v0.rs").is_file());
    assert_eq!(
        glue::check(&root, &out, "crate::plain", "crate::proto").unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "keep me\n");

    fs::write(out.join("v0.rs"), "changed\n").unwrap();
    let messages = glue::check(&root, &out, "crate::plain", "crate::proto").unwrap();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("changed generated")),
        "{messages:?}"
    );

    let extra = out.join("stale.rs");
    fs::write(&extra, "// @generated by witforge; do not edit.\n").unwrap();
    let messages = glue::check(&root, &out, "crate::plain", "crate::proto").unwrap();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("extra generated")),
        "{messages:?}"
    );

    fs::remove_file(out.join("v0.rs")).unwrap();
    let messages = glue::check(&root, &out, "crate::plain", "crate::proto").unwrap();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("missing generated")),
        "{messages:?}"
    );

    glue::generate(&root, &out, "crate::plain", "crate::proto").unwrap();
    assert!(!extra.exists());
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "keep me\n");
    assert!(glue::generate(&root, &root.join("inside"), "crate::plain", "crate::proto").is_err());

    let empty = std::env::temp_dir().join(format!("witforge-glue-empty-{}", std::process::id()));
    let _ = fs::remove_dir_all(&empty);
    fs::create_dir_all(&empty).unwrap();
    assert!(glue::plan(&empty, "crate::plain", "crate::proto").is_err());
    assert!(glue::plan(&root, "", "crate::proto").is_err());
    assert!(glue::plan(&root, "crate::plain", "crate::proto::").is_err());
    fs::remove_dir_all(&empty).unwrap();
    fs::remove_dir_all(&out).unwrap();
}
