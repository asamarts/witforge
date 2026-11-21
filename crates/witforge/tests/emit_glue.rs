//! Protobuf glue type-conversion tests.

use std::fs;
use std::path::{Path, PathBuf};

use prost::Message as _;
use witforge::emit::{glue, proto, rust};
use witforge::load;
use witforge::lock::WireLock;
use witforge::model;

mod support;
use support::run_cargo_program;

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

    run_cargo_program(
        &out,
        "prost = \"0.14\"\npbjson = \"0.9\"\nserde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n",
        "generated glue compile/run",
    );
    fs::remove_dir_all(&out).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn byte_list_aliases_compile_and_roundtrip() {
    let root = fixture("emit/bytes");
    let (model, lock) = setup_contract(&root);
    let version = format!(
        "v{}_{}",
        model.package.version.major, model.package.version.minor
    );
    let major = model.package.version.major;
    let out = std::env::temp_dir().join(format!("witforge-glue-bytes-{}", std::process::id()));
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
        format!("pub mod v{major} {{ include!(\"v{major}.rs\"); }}\n"),
    )
    .unwrap();
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
    fs::write(
        out.join("main.rs"),
        r"
mod glue;
mod plain;
mod proto;

fn main() {
    use plain::v0_1::types::{All, Choice};

    let value = All {
        direct: vec![1, 2],
        aliased: vec![3],
        chained: vec![4],
        named: vec![5],
        named_alias: vec![6],
        named_option: Some(vec![7]),
        named_result: Ok(vec![8]),
        nested_direct: vec![vec![vec![9]]],
        nested_aliased: vec![vec![vec![10]]],
        element: vec![vec![11]],
        choice: Choice::Aliased(vec![12]),
        outcome: Ok(vec![13]),
    };
    let encoded = glue::v0::types::all::to_proto(&value);
    let decoded = glue::v0::types::all::from_proto(encoded).unwrap();
    assert_eq!(value, decoded);

    let request = glue::v0::types::functions::get::to_request(&vec![14, 15]);
    let round = glue::v0::types::functions::get::from_request(request).unwrap();
    assert_eq!(round, vec![14, 15]);

    let nested = vec![vec![16], vec![17, 18]];
    let response = glue::v0::types::functions::nested::to_response(&vec![nested.clone()]);
    let round = glue::v0::types::functions::nested::from_response(response).unwrap();
    assert_eq!(round, vec![nested]);
}
",
    )
    .unwrap();

    run_cargo_program(&out, "prost = \"0.14\"\n", "byte-list glue compile/run");
    fs::remove_dir_all(&out).unwrap();
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
    // `check` reports the reserved `functions` glue module with `WF1003`.
    let diags = witforge::subset::check_diagnostics(&load(&dir).unwrap());
    assert!(
        diags
            .iter()
            .any(|d| d.code == "WF1003" && d.message.contains("reserved glue module `functions`")),
        "{diags:#?}"
    );
    // And a valid model still refuses empty module roots in `glue::emit_version`.
    fs::write(
        dir.join("contract.wit"),
        "package example:reserved-module@0.1.0;\n\
interface types {\n\
  record item { value: string }\n\
}\n",
    )
    .unwrap();
    let model = model::build(&load(&dir).unwrap()).unwrap();
    let lock = lock_for(&model);
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
        "::other::proto",
        "other::proto",
        "other",
        "crate::r#type",
        "::r#type::plain",
    ] {
        glue::validate_module_root(valid).unwrap_or_else(|error| panic!("{valid}: {error}"));
    }
    for invalid in [
        "",
        "::",
        "crate::",
        "::crate",
        "::self::plain",
        "::super::proto",
        "self",
        "self::plain",
        "super",
        "super::proto",
        "super::super::nested",
        "witforge_deps",
        "witforge_deps::plain",
        "functions::plain",
        "GlueError::plain",
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
    // A first segment matching a generated module (`v0`, `types`, ...) is refused as a usage
    // error, while `::v0::...` / `::types::...` explicitly targets an external crate.
    for colliding in ["v0::plain", "v0", "types::plain", "user::plain"] {
        let error = glue::plan(&root, colliding, "crate::proto").unwrap_err();
        assert!(!witforge::is_contract_error(&error), "{error:#}");
        assert!(
            format!("{error:#}").contains("starts with generated module"),
            "{colliding}: {error:#}"
        );
    }
    glue::plan(&root, "::v0::plain", "::types::proto").unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn absolute_module_roots_compile_with_crate_global_and_extern_paths() {
    let root = fixture("versions/wit");
    let out = std::env::temp_dir().join(format!(
        "witforge-glue-absolute-roots-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(out.join("plain_crate/src")).unwrap();
    fs::create_dir_all(out.join("proto_crate/src")).unwrap();
    fs::create_dir_all(out.join("proto-work")).unwrap();

    // Put plain Rust types in external crate `ext_plain` (referenced via `::ext_plain`) and
    // prost types in external crate `ext_proto` (referenced via unprefixed `ext_proto`).
    rust::plan(&root)
        .unwrap()
        .write(&out.join("plain_crate/src/generated"))
        .unwrap();
    fs::write(
        out.join("plain_crate/Cargo.toml"),
        "[package]\nname = \"ext_plain\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(out.join("plain_crate/src/lib.rs"), "pub mod generated;\n").unwrap();

    proto::plan(&root)
        .unwrap()
        .write(&out.join("proto-work"))
        .unwrap();
    let descriptors =
        protox::compile([out.join("proto-work/v0.proto")], [out.join("proto-work")]).unwrap();
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
    fs::copy(generated, out.join("proto_crate/src/v0.rs")).unwrap();
    fs::write(
        out.join("proto_crate/Cargo.toml"),
        "[package]\nname = \"ext_proto\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nprost = \"0.14\"\n",
    )
    .unwrap();
    fs::write(
        out.join("proto_crate/src/lib.rs"),
        "pub mod wire { pub mod v0 { include!(\"v0.rs\"); } }\n",
    )
    .unwrap();

    glue::plan(&root, "::ext_plain::generated", "ext_proto::wire")
        .unwrap()
        .write(&out.join("glue_ext"))
        .unwrap();
    glue::plan(&root, "crate::local_plain", "crate::local_proto")
        .unwrap()
        .write(&out.join("glue_crate"))
        .unwrap();

    fs::write(
        out.join("main.rs"),
        r#"
mod glue_ext;
mod glue_crate;
pub use ::ext_plain::generated as local_plain;
pub use ext_proto::wire as local_proto;

fn main() {
    let user = ::ext_plain::generated::v0_2::types::User {
        name: "alice".to_string(),
        new_field: Some("a@example.com".to_string()),
        renamed: None,
    };
    let wire = glue_ext::v0::types::user::to_proto(&user);
    assert_eq!(glue_ext::v0::types::user::from_proto(wire).unwrap(), user);
    let wire2 = glue_crate::v0::types::user::to_proto(&user);
    assert_eq!(glue_crate::v0::types::user::from_proto(wire2).unwrap(), user);
}
"#,
    )
    .unwrap();

    run_cargo_program(
        &out,
        "prost = \"0.14\"\next_plain = { path = \"plain_crate\" }\next_proto = { path = \"proto_crate\" }\n",
        "absolute module roots glue compile/run",
    );
    fs::remove_dir_all(&out).unwrap();
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

    fs::write(out.join("v0.rs"), format!("{}\nchanged\n", glue::HEADER)).unwrap();
    let messages = glue::check(&root, &out, "crate::plain", "crate::proto").unwrap();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("changed generated")),
        "{messages:?}"
    );
    // A file without the header is one generation refuses to overwrite, and --check says so
    // instead of calling it changed output.
    fs::write(out.join("v0.rs"), "changed\n").unwrap();
    let messages = glue::check(&root, &out, "crate::plain", "crate::proto").unwrap();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("refuses to overwrite")),
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

#[test]
fn glue_refuses_a_contract_the_protobuf_schema_refuses() {
    // The record `f-request` and the request message of function `f` are both `IFRequest`:
    // `check` reports `WF1003` and every generator refuses the contract.
    let root =
        std::env::temp_dir().join(format!("witforge-glue-proto-clash-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let version = root.join("v0.1");
    fs::create_dir_all(&version).unwrap();
    fs::write(
        version.join("contract.wit"),
        "package example:clash@0.1.0;\n\ninterface i {\n  record f-request { x: u32 }\n  f: func(x: u32);\n}\n",
    )
    .unwrap();
    fs::write(version.join("wire.lock"), "").unwrap();
    let diags = witforge::subset::check_diagnostics(&load(&version).unwrap());
    assert!(
        diags.iter().any(|d| d.code == "WF1003"
            && d.message
                .contains("proto name `IFRequest` is produced by both")),
        "{diags:#?}"
    );
    let errors = [
        rust::plan(&root).unwrap_err(),
        proto::plan(&root).unwrap_err(),
        glue::plan(&root, "crate::plain", "crate::proto").unwrap_err(),
    ];
    for error in errors {
        let message = format!("{error:#}");
        assert!(witforge::is_contract_error(&error), "{message}");
        assert!(
            message.contains("proto name `IFRequest` is produced by both"),
            "{message}"
        );
    }
    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn unreferenced_named_structural_types_compile_and_roundtrip() {
    let out = std::env::temp_dir().join(format!(
        "witforge-glue-unreferenced-wrappers-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out);
    let wit_root = out.join("wit");
    let v01 = wit_root.join("v0.1");
    fs::create_dir_all(&v01).unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package example:unreferenced@0.1.0;\n\n\
interface types {\n\
  type matrix = list<list<string>>;\n\
  type cube = list<list<list<u16>>>;\n\
  type maybe-list = option<list<string>>;\n\
  type maybe-matrix = option<list<list<string>>>;\n\
  type maybe-maybe = option<option<string>>;\n\
  type maybe-maybe-maybe = option<option<option<u32>>>;\n\
  type list-of-maybe-list = list<maybe-list>;\n\
  type nested-result = result<list<list<u32>>, option<option<string>>>;\n\
}\n",
    )
    .unwrap();
    let (_, lock) = setup_contract(&v01);
    fs::write(v01.join("wire.lock"), lock.render()).unwrap();

    rust::plan(&wit_root)
        .unwrap()
        .write(&out.join("plain"))
        .unwrap();
    glue::plan(&wit_root, "crate::plain", "crate::proto")
        .unwrap()
        .write(&out.join("glue"))
        .unwrap();
    proto::plan(&wit_root)
        .unwrap()
        .write(&out.join("proto-work"))
        .unwrap();

    let descriptors =
        protox::compile([out.join("proto-work/v0.proto")], [out.join("proto-work")]).unwrap();
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
    fs::create_dir_all(out.join("proto")).unwrap();
    fs::copy(generated, out.join("proto/v0.rs")).unwrap();
    fs::write(
        out.join("proto/mod.rs"),
        "pub mod v0 { include!(\"v0.rs\"); }\n",
    )
    .unwrap();
    fs::write(
        out.join("main.rs"),
        r#"
mod glue;
mod plain;
mod proto;

fn main() {
    let matrix = vec![vec!["a".to_string(), "b".to_string()], vec![]];
    let p = glue::v0::types::matrix::to_proto(&matrix);
    assert_eq!(glue::v0::types::matrix::from_proto(p).unwrap(), matrix);

    let cube = vec![vec![vec![1_u16, 2_u16], vec![]]];
    let p = glue::v0::types::cube::to_proto(&cube);
    assert_eq!(glue::v0::types::cube::from_proto(p).unwrap(), cube);

    let maybe_list = Some(vec!["x".to_string()]);
    let p = glue::v0::types::maybe_list::to_proto(&maybe_list);
    assert_eq!(glue::v0::types::maybe_list::from_proto(p).unwrap(), maybe_list);

    let maybe_matrix = Some(matrix.clone());
    let p = glue::v0::types::maybe_matrix::to_proto(&maybe_matrix);
    assert_eq!(glue::v0::types::maybe_matrix::from_proto(p).unwrap(), maybe_matrix);

    let maybe_maybe = Some(Some("hi".to_string()));
    let p = glue::v0::types::maybe_maybe::to_proto(&maybe_maybe);
    assert_eq!(glue::v0::types::maybe_maybe::from_proto(p).unwrap(), maybe_maybe);

    let maybe3: Option<Option<Option<u32>>> = Some(Some(None));
    let p = glue::v0::types::maybe_maybe_maybe::to_proto(&maybe3);
    assert_eq!(glue::v0::types::maybe_maybe_maybe::from_proto(p).unwrap(), maybe3);

    let chained = vec![Some(vec!["k".to_string()]), None];
    let p = glue::v0::types::list_of_maybe_list::to_proto(&chained);
    assert_eq!(glue::v0::types::list_of_maybe_list::from_proto(p).unwrap(), chained);

    let ok_res: Result<Vec<Vec<u32>>, Option<Option<String>>> = Ok(vec![vec![7]]);
    let p = glue::v0::types::nested_result::to_proto(&ok_res);
    assert_eq!(glue::v0::types::nested_result::from_proto(p).unwrap(), ok_res);

    let err_res: Result<Vec<Vec<u32>>, Option<Option<String>>> = Err(Some(None));
    let p = glue::v0::types::nested_result::to_proto(&err_res);
    assert_eq!(glue::v0::types::nested_result::from_proto(p).unwrap(), err_res);
}
"#,
    )
    .unwrap();
    run_cargo_program(
        &out,
        "prost = \"0.14\"\n",
        "unreferenced named structural types glue compile/run",
    );

    // A wrapper synthesized for an unreferenced type still collides with a user type of the same
    // proto name and is refused by `check`, `proto`, and `glue`.
    let clash_root = out.join("clash");
    let clash_v01 = clash_root.join("v0.1");
    fs::create_dir_all(&clash_v01).unwrap();
    fs::write(
        clash_v01.join("contract.wit"),
        "package example:clash-unreferenced@0.1.0;\n\
interface witforge { record list-of-string { value: string } }\n\
interface api { type matrix = list<list<string>>; }\n",
    )
    .unwrap();
    fs::write(clash_v01.join("wire.lock"), "").unwrap();
    let clash_diags = witforge::subset::check_diagnostics(&load(&clash_v01).unwrap());
    assert!(
        clash_diags
            .iter()
            .any(|d| d.code == "WF1003" && d.message.contains("WitforgeListOfString")),
        "{clash_diags:#?}"
    );
    for error in [
        rust::plan(&clash_root).unwrap_err(),
        proto::plan(&clash_root).unwrap_err(),
        glue::plan(&clash_root, "crate::plain", "crate::proto").unwrap_err(),
    ] {
        assert!(witforge::is_contract_error(&error), "{error:#}");
        assert!(
            format!("{error:#}").contains("WitforgeListOfString"),
            "{error:#}"
        );
    }
    fs::remove_dir_all(&out).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn one_letter_upper_case_and_digit_names_compile_and_roundtrip_against_prost() {
    let out = std::env::temp_dir().join(format!(
        "witforge-glue-ident-conversion-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out);
    let wit_root = out.join("wit");
    let v01 = wit_root.join("v0.1");
    fs::create_dir_all(v01.join("deps/dep")).unwrap();
    fs::write(
        v01.join("deps/dep/types.wit"),
        "package ex:API@0.1.0;\n\n\
interface x {\n\
  record y-z { foo-BAR: u32, a1: string }\n\
  enum HTTP-code { HTTP, ok2 }\n\
  get-a-b: func(foo2-bar: u32) -> y-z;\n\
}\n",
    )
    .unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package example:idents@0.1.0;\n\n\
interface types {\n\
  use ex:API/x@0.1.0.{y-z, HTTP-code};\n\
  enum x-kind { foo, bar-baz }\n\
  enum e { HTTP, other }\n\
  variant x-kind-v { x-y(u32), foo-BAR(string), a1 }\n\
  record r { foo-BAR: u32, URL: string, foo2-bar: u32, a1: u32, dep: y-z, code: HTTP-code }\n\
}\n\n\
interface api {\n\
  record x-y { a1: u32, foo2-bar: string }\n\
  get-x-y: func(foo-BAR: u32, a1: string) -> x-y;\n\
}\n\n\
interface a {\n\
  ping: func(x-y: u32) -> u32;\n\
  get-x-y: func(foo-BAR: string) -> string;\n\
}\n\n\
interface HTTP-api {\n\
  record HTTP-item { HTTP-url: string }\n\
}\n",
    )
    .unwrap();
    let contract = load(&v01).unwrap();
    assert!(
        witforge::subset::check_diagnostics(&contract).is_empty(),
        "unexpected subset diagnostics: {:?}",
        witforge::subset::check_diagnostics(&contract)
    );
    let (_, lock) = setup_contract(&v01);
    fs::write(v01.join("wire.lock"), lock.render()).unwrap();

    rust::plan(&wit_root)
        .unwrap()
        .write(&out.join("plain"))
        .unwrap();
    glue::plan(&wit_root, "crate::plain", "crate::proto")
        .unwrap()
        .write(&out.join("glue"))
        .unwrap();
    proto::plan(&wit_root)
        .unwrap()
        .write(&out.join("proto-work"))
        .unwrap();

    // Plain Rust output must compile cleanly with zero non_snake_case or non_camel_case_types.
    let plain_check = std::process::Command::new("rustc")
        .args([
            "--edition=2024",
            "--crate-type=lib",
            "-D",
            "non_snake_case",
            "-D",
            "non_camel_case_types",
            "--emit=metadata",
            "-o",
        ])
        .arg(out.join("plain.rmeta"))
        .arg(out.join("plain/mod.rs"))
        .output()
        .unwrap();
    assert!(
        plain_check.status.success(),
        "plain Rust had case warnings:\n{}",
        String::from_utf8_lossy(&plain_check.stderr)
    );

    // Compile `.proto` with protox + prost-build (default naming).
    let proto_files = [
        out.join("proto-work/v0.proto"),
        out.join("proto-work/ex.API.v0.proto"),
    ];
    let descriptors = protox::compile(proto_files, [out.join("proto-work")]).unwrap();
    let prost_out = out.join("proto-work/out");
    fs::create_dir_all(&prost_out).unwrap();
    prost_build::Config::new()
        .out_dir(&prost_out)
        .compile_fds(descriptors)
        .unwrap();

    fs::create_dir_all(out.join("proto/example/idents")).unwrap();
    fs::create_dir_all(out.join("proto/ex/api")).unwrap();
    fs::copy(
        prost_out.join("example.idents.v0.rs"),
        out.join("proto/example/idents/v0.rs"),
    )
    .unwrap();
    fs::copy(
        prost_out.join("ex.api.v0.rs"),
        out.join("proto/ex/api/v0.rs"),
    )
    .unwrap();
    fs::write(
        out.join("proto/mod.rs"),
        "pub mod example { pub mod idents { pub mod v0 { include!(\"example/idents/v0.rs\"); } } }\n\
         pub mod ex { pub mod api { pub mod v0 { include!(\"ex/api/v0.rs\"); } } }\n",
    )
    .unwrap();

    fs::write(
        out.join("main.rs"),
        r#"
mod glue;
mod plain;
mod proto;

fn main() {
    use plain::v0_1::{api, http_api, types};
    use plain::witforge_deps::ex_api::x::{HttpCode, YZ};

    // enum x-kind { foo } -> prost TypesXKind::TypesXkindFoo
    let k = types::XKind::Foo;
    let kp = glue::v0::types::x_kind::to_proto(&k);
    assert_eq!(glue::v0::types::x_kind::from_proto(kp).unwrap(), k);

    // enum e { HTTP, other } -> prost TypesE::Http
    let e = types::E::Http;
    let ep = glue::v0::types::e::to_proto(&e);
    assert_eq!(glue::v0::types::e::from_proto(ep).unwrap(), e);

    // variant x-kind-v -> oneof module types_x_kind_v
    for v in [
        types::XKindV::XY(7),
        types::XKindV::FooBar("hi".to_string()),
        types::XKindV::A1,
    ] {
        let vp = glue::v0::types::x_kind_v::to_proto(&v);
        assert_eq!(glue::v0::types::x_kind_v::from_proto(vp).unwrap(), v);
    }

    // record with foo-BAR, URL, foo2-bar, a1, and dependency types
    let r = types::R {
        foo_bar: 1,
        url: "https://example.test".to_string(),
        foo2_bar: 2,
        a1: 3,
        dep: YZ { foo_bar: 4, a1: "dep".to_string() },
        code: HttpCode::Http,
    };
    let rp = glue::v0::types::r::to_proto(&r);
    assert_eq!(glue::v0::types::r::from_proto(rp).unwrap(), r);

    // api/x-y and get-x-y -> prost ApiXy, ApiGetXyRequest, ApiMethod::GetXY
    let _m = glue::v0::api::functions::get_x_y::METHOD;
    let req = glue::v0::api::functions::get_x_y::to_request(&9, &"ok".to_string());
    assert_eq!(
        glue::v0::api::functions::get_x_y::from_request(req).unwrap(),
        (9, "ok".to_string())
    );
    let xy = api::XY { a1: 5, foo2_bar: "xy".to_string() };
    let resp = glue::v0::api::functions::get_x_y::to_response(&xy);
    assert_eq!(glue::v0::api::functions::get_x_y::from_response(resp).unwrap(), xy);

    // One-letter interface `a` -> AMethod with AMETHOD_UNSPECIFIED and A_METHOD_PING
    let _am1 = glue::v0::a::functions::ping::METHOD;
    let _am2 = glue::v0::a::functions::get_x_y::METHOD;
    let req = glue::v0::a::functions::ping::to_request(&42);
    assert_eq!(glue::v0::a::functions::ping::from_request(req).unwrap(), 42);

    // Upper-case interface HTTP-api
    let h = http_api::HttpItem { http_url: "u".to_string() };
    let hp = glue::v0::http_api::http_item::to_proto(&h);
    assert_eq!(glue::v0::http_api::http_item::from_proto(hp).unwrap(), h);

    // Dependency function get-a-b
    let dreq = glue::witforge_deps::ex_api::x::functions::get_a_b::to_request(&11);
    assert_eq!(
        glue::witforge_deps::ex_api::x::functions::get_a_b::from_request(dreq).unwrap(),
        11
    );
}
"#,
    )
    .unwrap();
    run_cargo_program(
        &out,
        "prost = \"0.14\"\n",
        "unified ident conversion glue compile/run",
    );

    // Two WIT names mapping to one prost identifier (`api-x/r` vs `api/xr` -> `ApiXr`) are
    // refused by `check` (WF1003), `proto`, and `glue`.
    let clash_root = out.join("clash-prost");
    let clash_v01 = clash_root.join("v0.1");
    fs::create_dir_all(&clash_v01).unwrap();
    fs::write(
        clash_v01.join("contract.wit"),
        "package example:clash-prost@0.1.0;\n\
interface api-x { record r { a: u32 } }\n\
interface api { record xr { a: u32 } }\n",
    )
    .unwrap();
    let clash_contract = load(&clash_v01).unwrap();
    let diagnostics = witforge::subset::check_diagnostics(&clash_contract);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "WF1003" && d.message.contains("ApiXr")),
        "{diagnostics:?}"
    );
    let model_err = model::build(&clash_contract).unwrap_err();
    assert!(format!("{model_err:#}").contains("ApiXr"), "{model_err:#}");
    let proto_err = proto::plan(&clash_root).unwrap_err();
    assert!(witforge::is_contract_error(&proto_err), "{proto_err:#}");
    assert!(format!("{proto_err:#}").contains("ApiXr"), "{proto_err:#}");

    // A new minor that adds a field colliding only in case with a removed field is refused at
    // lock reconciliation.
    let minor_root = out.join("minor-reserved-clash");
    let minor_v01 = minor_root.join("v0.1");
    let minor_v02 = minor_root.join("v0.2");
    fs::create_dir_all(&minor_v01).unwrap();
    fs::create_dir_all(&minor_v02).unwrap();
    fs::write(
        minor_v01.join("contract.wit"),
        "package example:minor-clash@0.1.0;\ninterface types { record r { foo-BAR: u32 } }\n",
    )
    .unwrap();
    let first_minor_model = model::build(&load(&minor_v01).unwrap()).unwrap();
    let first_minor_lock = lock_for(&first_minor_model);
    fs::write(minor_v01.join("wire.lock"), first_minor_lock.render()).unwrap();

    fs::write(
        minor_v02.join("contract.wit"),
        "package example:minor-clash@0.2.0;\ninterface types { record r { foo-bar: u32 } }\n",
    )
    .unwrap();
    let second_minor_model = model::build(&load(&minor_v02).unwrap()).unwrap();
    let mut second_minor_lock = first_minor_lock;
    let lock_err = second_minor_lock
        .reconcile(&second_minor_model.members().unwrap())
        .unwrap_err();
    assert!(
        format!("{lock_err:#}").contains("reserved member `foo-BAR`"),
        "{lock_err:#}"
    );

    // A new minor that adds a type colliding with a removed interface's reserved method enum in
    // the major's shared proto file is refused at that minor by both `proto` and `glue`.
    let major_root = out.join("major-shared-clash");
    let major_v01 = major_root.join("v0.1");
    let major_v02 = major_root.join("v0.2");
    fs::create_dir_all(&major_v01).unwrap();
    fs::create_dir_all(&major_v02).unwrap();
    fs::write(
        major_v01.join("contract.wit"),
        "package example:major-clash@0.1.0;\ninterface api-x { ping: func(); }\n",
    )
    .unwrap();
    let major_first_model = model::build(&load(&major_v01).unwrap()).unwrap();
    let major_first_lock = lock_for(&major_first_model);
    fs::write(major_v01.join("wire.lock"), major_first_lock.render()).unwrap();

    fs::write(
        major_v02.join("contract.wit"),
        "package example:major-clash@0.2.0;\ninterface api { record x-method { a: u32 } }\n",
    )
    .unwrap();
    let major_second_model = model::build(&load(&major_v02).unwrap()).unwrap();
    let mut major_second_lock = major_first_lock;
    major_second_lock
        .reconcile(&major_second_model.members().unwrap())
        .unwrap();
    fs::write(major_v02.join("wire.lock"), major_second_lock.render()).unwrap();

    let minor_proto_err = proto::plan(&major_root).unwrap_err();
    assert!(
        witforge::is_contract_error(&minor_proto_err),
        "{minor_proto_err:#}"
    );
    assert!(
        format!("{minor_proto_err:#}").contains("ApiXMethod"),
        "{minor_proto_err:#}"
    );
    let minor_glue_err = glue::plan(&major_root, "crate::plain", "crate::proto").unwrap_err();
    assert!(
        witforge::is_contract_error(&minor_glue_err),
        "{minor_glue_err:#}"
    );
    assert!(
        format!("{minor_glue_err:#}").contains("ApiXMethod"),
        "{minor_glue_err:#}"
    );

    // Cross-interface collisions involving method enums (`ApiXMethod`) and named option direct
    // wrappers (`ApiOptItem`) are refused by `check` with WF1003.
    let cross_clash = out.join("cross-clash/v0.1");
    fs::create_dir_all(&cross_clash).unwrap();
    fs::write(
        cross_clash.join("contract.wit"),
        "package example:cross-clash@0.1.0;\n\
         interface api-x { ping: func(); }\n\
         interface api {\n\
           record x-method { x: u32 }\n\
           type opt-item = option<list<u32>>;\n\
         }\n\
         interface api-opt { record item { x: u32 } }\n",
    )
    .unwrap();
    let cross_diags = witforge::subset::check_diagnostics(&load(&cross_clash).unwrap());
    for needle in ["ApiXMethod", "ApiOptItem"] {
        assert!(
            cross_diags
                .iter()
                .any(|d| d.code == "WF1003" && d.message.contains(needle)),
            "expected WF1003 for `{needle}`, got {cross_diags:?}"
        );
    }

    fs::remove_dir_all(&out).unwrap();
}

#[test]
#[allow(clippy::too_many_lines)]
fn prost_keyword_escaping_accessor_collisions_and_keyword_dep_paths() {
    let out = std::env::temp_dir().join(format!(
        "witforge-glue-prost-keywords-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out);
    let wit_root = out.join("wit");
    let v01 = wit_root.join("v0.1");

    // Every Rust 2015, 2018, 2021, and 2024 lowercase keyword (strict, reserved, and weak `union`),
    // matching `prost-build` 0.14's escaping table plus plain Rust's `union`.
    let rust_keywords = [
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod",
        "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while", "abstract", "become", "box", "do",
        "final", "macro", "override", "priv", "try", "typeof", "union", "unsized", "virtual",
        "yield",
    ];
    let wit_fields: Vec<String> = rust_keywords
        .iter()
        .map(|kw| format!("%{kw}: u32"))
        .collect();
    let (flag_kw_a, flag_kw_b) = rust_keywords.split_at(26);
    let wit_flags_a: Vec<String> = flag_kw_a.iter().map(|kw| format!("%{kw}")).collect();
    let wit_flags_b: Vec<String> = flag_kw_b.iter().map(|kw| format!("%{kw}")).collect();
    let wit_params: Vec<String> = [
        "extern", "self", "super", "crate", "gen", "type", "union", "async", "await", "try",
    ]
    .iter()
    .map(|kw| format!("%{kw}: u32"))
    .collect();

    // Dependencies whose namespace or package segments are Rust keywords (raw, suffixed, and weak).
    let keyword_deps = [
        ("dep-gen", "example:gen", "gen-item"),
        ("dep-extern", "example:extern", "extern-item"),
        ("dep-type", "example:%type", "type-item"),
        ("dep-self-super", "self:super", "self-item"),
        ("dep-crate-union", "crate:union", "union-item"),
    ];
    for (dir, pkg, item) in &keyword_deps {
        let dep_dir = v01.join("deps").join(dir);
        fs::create_dir_all(&dep_dir).unwrap();
        fs::write(
            dep_dir.join("types.wit"),
            format!(
                "package {pkg}@0.1.0;\n\
                 interface types {{\n\
                   record {item} {{ extern: u32, %type: string, gen: bool, union: u32 }}\n\
                 }}\n"
            ),
        )
        .unwrap();
    }

    let contract_wit = format!(
        "package example:keywords@0.1.0;\n\n\
interface types {{\n\
  use example:gen/types@0.1.0.{{gen-item}};\n\
  use example:extern/types@0.1.0.{{extern-item}};\n\
  use example:%type/types@0.1.0.{{type-item}};\n\
  use self:super/types@0.1.0.{{self-item}};\n\
  use crate:union/types@0.1.0.{{union-item}};\n\
  enum mode {{ a, b, SELF }}\n\
  record inner {{ value: u32 }}\n\
  record all-keywords {{ {fields} }}\n\
  flags flag-keywords-a {{ {flags_a} }}\n\
  flags flag-keywords-b {{ {flags_b} }}\n\
  record dep-holder {{\n\
    g: gen-item,\n\
    e: extern-item,\n\
    t: type-item,\n\
    s: self-item,\n\
    u: union-item,\n\
  }}\n\
  record ok-accessors {{\n\
    mode: mode,\n\
    set-mode: inner,\n\
    modes: list<mode>,\n\
    push-modes: list<u32>,\n\
    extern: mode,\n\
    set-extern: bool,\n\
  }}\n\
  flags flag-accessors {{ mode, set-mode }}\n\
  call-keywords: func({params}) -> u32;\n\
}}\n",
        fields = wit_fields.join(", "),
        flags_a = wit_flags_a.join(", "),
        flags_b = wit_flags_b.join(", "),
        params = wit_params.join(", "),
    );
    fs::write(v01.join("contract.wit"), contract_wit).unwrap();

    let contract = load(&v01).unwrap();
    let diags = witforge::subset::check_diagnostics(&contract);
    assert!(diags.is_empty(), "unexpected subset diagnostics: {diags:?}");
    let (_, lock) = setup_contract(&v01);
    fs::write(v01.join("wire.lock"), lock.render()).unwrap();

    rust::plan(&wit_root)
        .unwrap()
        .write(&out.join("plain"))
        .unwrap();
    glue::plan(&wit_root, "crate::plain", "crate::proto")
        .unwrap()
        .write(&out.join("glue"))
        .unwrap();
    proto::plan(&wit_root)
        .unwrap()
        .write(&out.join("proto-work"))
        .unwrap();

    // Compile all `.proto` files with `protox` + `prost-build` and let `prost-build` generate
    // `proto/mod.rs` itself so every keyword package segment is proven against prost's own tree.
    let proto_files = [
        out.join("proto-work/v0.proto"),
        out.join("proto-work/example.gen.v0.proto"),
        out.join("proto-work/example.extern.v0.proto"),
        out.join("proto-work/example.type.v0.proto"),
        out.join("proto-work/self.super.v0.proto"),
        out.join("proto-work/crate.union.v0.proto"),
    ];
    let descriptors = protox::compile(proto_files, [out.join("proto-work")]).unwrap();
    let prost_out = out.join("proto");
    fs::create_dir_all(&prost_out).unwrap();
    prost_build::Config::new()
        .out_dir(&prost_out)
        .include_file("mod.rs")
        .compile_fds(descriptors)
        .unwrap();

    // Build field literals for `AllKeywords` and `AllFlagKeywords` using plain Rust escaping.
    let plain_escape = |kw: &str| match kw {
        "crate" | "self" | "super" | "Self" => format!("{kw}_"),
        _ => format!("r#{kw}"),
    };
    let kw_struct_fields = rust_keywords
        .iter()
        .enumerate()
        .map(|(i, kw)| format!("        {}: {},", plain_escape(kw), i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    let kw_flag_fields_a = flag_kw_a
        .iter()
        .enumerate()
        .map(|(i, kw)| format!("        {}: {},", plain_escape(kw), i % 2 == 0))
        .collect::<Vec<_>>()
        .join("\n");
    let kw_flag_fields_b = flag_kw_b
        .iter()
        .enumerate()
        .map(|(i, kw)| format!("        {}: {},", plain_escape(kw), i % 2 == 1))
        .collect::<Vec<_>>()
        .join("\n");

    fs::write(
        out.join("main.rs"),
        format!(
            r#"
mod glue;
mod plain;
mod proto;

fn main() {{
    use plain::v0_1::types;
    use plain::witforge_deps::{{
        crate_union::types::UnionItem,
        example_extern::types::ExternItem,
        example_gen::types::GenItem,
        example_type::types::TypeItem,
        self_super::types::SelfItem,
    }};

    let all_kw = types::AllKeywords {{
{kw_struct_fields}    }};
    let kw_proto = glue::v0::types::all_keywords::to_proto(&all_kw);
    assert_eq!(kw_proto.extern_, Some(11));
    assert_eq!(kw_proto.union, Some(49));
    assert_eq!(kw_proto.r#gen, Some(15));
    assert_eq!(kw_proto.self_, Some(28));
    assert_eq!(glue::v0::types::all_keywords::from_proto(kw_proto).unwrap(), all_kw);

    let flags_a = types::FlagKeywordsA {{
{kw_flag_fields_a}    }};
    let flags_a_proto = glue::v0::types::flag_keywords_a::to_proto(&flags_a);
    assert_eq!(glue::v0::types::flag_keywords_a::from_proto(flags_a_proto).unwrap(), flags_a);

    let flags_b = types::FlagKeywordsB {{
{kw_flag_fields_b}    }};
    let flags_b_proto = glue::v0::types::flag_keywords_b::to_proto(&flags_b);
    assert_eq!(glue::v0::types::flag_keywords_b::from_proto(flags_b_proto).unwrap(), flags_b);

    let holder = types::DepHolder {{
        g: GenItem {{ r#extern: 1, r#type: "g".into(), r#gen: true, r#union: 2 }},
        e: ExternItem {{ r#extern: 3, r#type: "e".into(), r#gen: false, r#union: 4 }},
        t: TypeItem {{ r#extern: 5, r#type: "t".into(), r#gen: true, r#union: 6 }},
        s: SelfItem {{ r#extern: 7, r#type: "s".into(), r#gen: false, r#union: 8 }},
        u: UnionItem {{ r#extern: 9, r#type: "u".into(), r#gen: true, r#union: 10 }},
    }};
    let holder_proto = glue::v0::types::dep_holder::to_proto(&holder);
    assert_eq!(glue::v0::types::dep_holder::from_proto(holder_proto).unwrap(), holder);

    let ok_acc = types::OkAccessors {{
        mode: types::Mode::Self_,
        set_mode: types::Inner {{ value: 42 }},
        modes: vec![types::Mode::A, types::Mode::B],
        push_modes: vec![1, 2, 3],
        r#extern: types::Mode::B,
        set_extern: true,
    }};
    let ok_acc_proto = glue::v0::types::ok_accessors::to_proto(&ok_acc);
    assert_eq!(glue::v0::types::ok_accessors::from_proto(ok_acc_proto).unwrap(), ok_acc);

    let flag_acc = types::FlagAccessors {{ mode: true, set_mode: false }};
    let flag_acc_proto = glue::v0::types::flag_accessors::to_proto(&flag_acc);
    assert_eq!(glue::v0::types::flag_accessors::from_proto(flag_acc_proto).unwrap(), flag_acc);

    let req = glue::v0::types::functions::call_keywords::to_request(
        &1, &2, &3, &4, &5, &6, &7, &8, &9, &10,
    );
    assert_eq!(req.extern_, Some(1));
    assert_eq!(req.self_, Some(2));
    assert_eq!(req.super_, Some(3));
    assert_eq!(req.crate_, Some(4));
    assert_eq!(req.r#gen, Some(5));
    assert_eq!(req.r#type, Some(6));
    assert_eq!(req.union, Some(7));
    assert_eq!(
        glue::v0::types::functions::call_keywords::from_request(req).unwrap(),
        (1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
    );
}}
"#
        ),
    )
    .unwrap();

    run_cargo_program(
        &out,
        "prost = \"0.14\"\n",
        "prost keyword escaping and keyword dependency path glue compile/run",
    );

    // Every generated prost accessor collision class must be refused by `check` (WF1003), `proto`,
    // and `glue`, naming both fields and the shared generated accessor.
    let collision_cases: &[(&str, &str, &str, &str, &str)] = &[
        (
            "enum-set-bool",
            "package example:acc1@0.1.0;\n\
             interface types {\n\
               enum mode { a, b }\n\
               record cfg { mode: mode, set-mode: bool }\n\
             }\n",
            "mode",
            "set-mode",
            "set_mode",
        ),
        (
            "opt-enum-set-str",
            "package example:acc2@0.1.0;\n\
             interface types {\n\
               enum mode { a, b }\n\
               record cfg { mode: option<mode>, set-mode: string }\n\
             }\n",
            "mode",
            "set-mode",
            "set_mode",
        ),
        (
            "list-enum-push-bool",
            "package example:acc3@0.1.0;\n\
             interface types {\n\
               enum mode { a, b }\n\
               record cfg { modes: list<mode>, push-modes: bool }\n\
             }\n",
            "modes",
            "push-modes",
            "push_modes",
        ),
        (
            "raw-kw-enum-set",
            "package example:acc4@0.1.0;\n\
             interface types {\n\
               enum mode { a, b }\n\
               record cfg { %type: mode, set-type: list<u8> }\n\
             }\n",
            "type",
            "set-type",
            "set_type",
        ),
        (
            "alias-enum-and-bytes",
            "package example:acc5@0.1.0;\n\
             interface types {\n\
               enum mode { a, b }\n\
               type mode-alias = mode;\n\
               type byte = u8;\n\
               type blob = list<byte>;\n\
               record cfg { mode: mode-alias, set-mode: blob }\n\
             }\n",
            "mode",
            "set-mode",
            "set_mode",
        ),
        (
            "func-param-enum-set",
            "package example:acc6@0.1.0;\n\
             interface api {\n\
               enum mode { a, b }\n\
               apply: func(mode: mode, set-mode: bool);\n\
             }\n",
            "mode",
            "set-mode",
            "set_mode",
        ),
        (
            "func-param-list-enum-push",
            "package example:acc7@0.1.0;\n\
             interface api {\n\
               enum mode { a, b }\n\
               apply: func(modes: list<mode>, push-modes: u32);\n\
             }\n",
            "modes",
            "push-modes",
            "push_modes",
        ),
    ];

    for (name, wit, first_field, second_field, accessor) in collision_cases {
        let case_root = out.join(format!("clash-{name}"));
        let case_v01 = case_root.join("v0.1");
        fs::create_dir_all(&case_v01).unwrap();
        fs::write(case_v01.join("contract.wit"), wit).unwrap();
        let c = load(&case_v01).unwrap();
        let diagnostics = witforge::subset::check_diagnostics(&c);
        assert!(
            diagnostics.iter().any(|d| {
                d.code == "WF1003"
                    && d.message.contains(first_field)
                    && d.message.contains(second_field)
                    && d.message.contains(accessor)
            }),
            "{name}: expected WF1003 naming `{first_field}`, `{second_field}`, `{accessor}`, got {diagnostics:?}"
        );
        let proto_err = proto::plan(&case_root).unwrap_err();
        assert!(
            witforge::is_contract_error(&proto_err) && format!("{proto_err:#}").contains(accessor),
            "{name}: unexpected proto error: {proto_err:#}"
        );
        let glue_err = glue::plan(&case_root, "crate::plain", "crate::proto").unwrap_err();
        assert!(
            witforge::is_contract_error(&glue_err) && format!("{glue_err:#}").contains(accessor),
            "{name}: unexpected glue error: {glue_err:#}"
        );
    }

    // Accessor collision inside a dependency package is also refused by `check`.
    let dep_clash_root = out.join("clash-dep-acc");
    let dep_clash_v01 = dep_clash_root.join("v0.1");
    fs::create_dir_all(dep_clash_v01.join("deps/bad")).unwrap();
    fs::write(
        dep_clash_v01.join("deps/bad/types.wit"),
        "package example:baddep@0.1.0;\n\
         interface types {\n\
           enum mode { a, b }\n\
           record cfg { mode: mode, set-mode: bool }\n\
         }\n",
    )
    .unwrap();
    fs::write(
        dep_clash_v01.join("contract.wit"),
        "package example:usesbad@0.1.0;\n\
         interface types {\n\
           use example:baddep/types@0.1.0.{cfg};\n\
           record holder { c: cfg }\n\
         }\n",
    )
    .unwrap();
    let dep_diags = witforge::subset::check_diagnostics(&load(&dep_clash_v01).unwrap());
    assert!(
        dep_diags.iter().any(|d| {
            d.code == "WF1003"
                && d.message.contains("mode")
                && d.message.contains("set-mode")
                && d.message.contains("set_mode")
        }),
        "expected dependency WF1003 for set_mode, got {dep_diags:?}"
    );

    fs::remove_dir_all(&out).unwrap();
}

#[test]
#[allow(clippy::too_many_lines)]
fn unset_option_wrappers_doc_comments_and_no_trailing_whitespace() {
    let out = std::env::temp_dir().join(format!(
        "witforge-glue-unset-wrappers-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out);
    let wit_root = out.join("wit");
    let v01 = wit_root.join("v0.1");
    fs::create_dir_all(v01.join("deps/dep")).unwrap();
    fs::write(
        v01.join("deps/dep/types.wit"),
        "package example:dep@0.1.0;\n\n\
/// Dependency types.\n\
///\n\
/// Second paragraph.\n\
interface types {\n\
  /// A foreign named option.\n\
  ///\n\
  /// With a blank doc line.\n\
  type dep-opt = option<u32>;\n\
}\n",
    )
    .unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package example:wrappers@0.1.0;\n\n\
/// Root types interface.\n\
///\n\
/// Paragraph two.\n\
interface types {\n\
  use example:dep/types@0.1.0.{dep-opt};\n\n\
  /// Local named option.\n\
  ///\n\
  /// Paragraph two.\n\
  type local-opt = option<u32>;\n\
  type nested-local = option<local-opt>;\n\
  type nested-anon = option<option<u32>>;\n\
  type nested-dep = option<dep-opt>;\n\n\
  /// Variant covering all three option kinds in oneof position.\n\
  ///\n\
  /// Blank line above.\n\
  variant choice {\n\
    local(local-opt),\n\
    anon(option<u32>),\n\
    dep(dep-opt),\n\
  }\n\n\
  record holder {\n\
    field-local: nested-local,\n\
    field-anon: nested-anon,\n\
    field-dep: nested-dep,\n\
    list-local: list<local-opt>,\n\
    list-anon: list<option<u32>>,\n\
    list-dep: list<dep-opt>,\n\
    choice-req: choice,\n\
    res-local: result<local-opt, string>,\n\
    res-anon: result<option<u32>, string>,\n\
    res-dep: result<dep-opt, string>,\n\
  }\n\n\
  /// Function taking and returning options across all three paths.\n\
  ///\n\
  /// Second line.\n\
  echo-local: func(a: nested-local, b: list<local-opt>, c: choice) -> list<local-opt>;\n\
  echo-anon: func(a: nested-anon, b: list<option<u32>>) -> nested-anon;\n\
  echo-dep: func(a: nested-dep, b: list<dep-opt>) -> list<dep-opt>;\n\
  ping: func();\n\
}\n",
    )
    .unwrap();

    let (_, lock) = setup_contract(&v01);
    fs::write(v01.join("wire.lock"), lock.render()).unwrap();

    let plain_files = rust::plan(&wit_root).unwrap();
    let proto_files_plan = proto::plan(&wit_root).unwrap();
    let glue_files = glue::plan(&wit_root, "crate::plain", "crate::proto").unwrap();

    // Every generated file across plain Rust, proto, and glue must have zero trailing whitespace,
    // and empty WIT doc lines must emit bare `///` or `//`.
    for (kind, files) in [
        ("rust", plain_files.files()),
        ("proto", proto_files_plan.files()),
        ("glue", glue_files.files()),
    ] {
        for (path, content) in files {
            for (idx, line) in content.lines().enumerate() {
                assert!(
                    !line.ends_with(' ') && !line.ends_with('\t'),
                    "{kind} `{path}` line {} ends with whitespace: {line:?}",
                    idx + 1
                );
            }
        }
    }
    assert!(
        plain_files.files()["v0_1.rs"].contains("\n///\n"),
        "expected bare `///` for empty doc line in plain Rust"
    );
    assert!(
        proto_files_plan.files()["v0.proto"].contains("\n//\n"),
        "expected bare `//` for empty doc line in proto"
    );
    let glue_v0 = &glue_files.files()["v0.rs"];
    assert!(
        glue_v0.contains("\n///\n") || glue_v0.contains("\n    ///\n"),
        "expected bare `///` for empty doc line in glue"
    );
    // Synthesized glue helpers carry doc comments naming their WIT type or function.
    for expected_doc in [
        "/// Convert `types/holder` to its protobuf representation.",
        "/// Convert `types/holder` from its protobuf representation.",
        "/// Convert `types/local-opt` to its protobuf representation.",
        "/// Convert `types/local-opt` from its protobuf representation.",
        "/// Protobuf conversion helpers for `types` functions.",
        "/// Protobuf method identifier for `types#echo-local`.",
        "/// Encode request parameters for `types#echo-local`.",
        "/// Decode request parameters for `types#echo-local`.",
        "/// Encode the response for `types#echo-local`.",
        "/// Decode the response for `types#echo-local`.",
        "/// Encode the response for `types#ping`.",
        "/// Decode the response for `types#ping`.",
    ] {
        assert!(
            glue_v0.contains(expected_doc),
            "missing synthesized doc `{expected_doc}` in glue v0.rs"
        );
    }

    plain_files.write(&out.join("plain")).unwrap();
    proto_files_plan.write(&out.join("proto-work")).unwrap();
    glue_files.write(&out.join("glue")).unwrap();

    // Compile `.proto` with prost-build AND pbjson-build so we also test proto3 JSON `null` vs
    // absent vs `{}` (unset wrapper oneof).
    let descriptors = protox::compile(
        [
            out.join("proto-work/v0.proto"),
            out.join("proto-work/example.dep.v0.proto"),
        ],
        [out.join("proto-work")],
    )
    .unwrap();
    let descriptor_bytes = prost::Message::encode_to_vec(&descriptors);
    let prost_out = out.join("proto");
    fs::create_dir_all(&prost_out).unwrap();
    prost_build::Config::new()
        .out_dir(&prost_out)
        .include_file("mod.rs")
        .compile_fds(descriptors)
        .unwrap();
    pbjson_build::Builder::new()
        .register_descriptors(&descriptor_bytes)
        .unwrap()
        .out_dir(&prost_out)
        .build(&[".example"])
        .unwrap();
    // Include the generated serde impls inside each prost module.
    let mod_rs = fs::read_to_string(prost_out.join("mod.rs")).unwrap();
    let patched_mod = mod_rs
        .replace(
            "include!(\"example.wrappers.v0.rs\");",
            "include!(\"example.wrappers.v0.rs\");\ninclude!(\"example.wrappers.v0.serde.rs\");",
        )
        .replace(
            "include!(\"example.dep.v0.rs\");",
            "include!(\"example.dep.v0.rs\");\ninclude!(\"example.dep.v0.serde.rs\");",
        );
    fs::write(prost_out.join("mod.rs"), patched_mod).unwrap();

    fs::write(
        out.join("main.rs"),
        r##"
mod glue;
mod plain;
mod proto;

use glue::GlueError;
use proto::example::wrappers::v0 as p;

fn valid_holder() -> p::TypesHolder {
    glue::v0::types::holder::to_proto(&plain::v0_1::types::Holder {
        field_local: Some(Some(1)),
        field_anon: Some(None),
        field_dep: None,
        list_local: vec![Some(2), None],
        list_anon: vec![Some(3), None],
        list_dep: vec![Some(4), None],
        choice_req: plain::v0_1::types::Choice::Dep(Some(5)),
        res_local: Ok(Some(6)),
        res_anon: Ok(None),
        res_dep: Ok(Some(7)),
    })
}

fn main() {
    let base = valid_holder();
    assert!(glue::v0::types::holder::from_proto(base.clone()).is_ok());

    // 1. Direct record field: absent outer option is Ok(None), unset inner wrapper is InvalidState.
    let mut f = base.clone();
    f.field_local = None;
    assert_eq!(glue::v0::types::holder::from_proto(f).unwrap().field_local, None);
    let mut f = base.clone();
    f.field_local = Some(p::TypesNestedLocal { value: None });
    assert!(matches!(glue::v0::types::holder::from_proto(f), Err(GlueError::InvalidState { .. })));

    let mut f = base.clone();
    f.field_anon = None;
    assert_eq!(glue::v0::types::holder::from_proto(f).unwrap().field_anon, None);
    let mut f = base.clone();
    f.field_anon = Some(p::TypesNestedAnon { value: None });
    assert!(matches!(glue::v0::types::holder::from_proto(f), Err(GlueError::InvalidState { .. })));

    let mut f = base.clone();
    f.field_dep = None;
    assert_eq!(glue::v0::types::holder::from_proto(f).unwrap().field_dep, None);
    let mut f = base.clone();
    f.field_dep = Some(p::TypesNestedDep { value: None });
    assert!(matches!(glue::v0::types::holder::from_proto(f), Err(GlueError::InvalidState { .. })));

    // Required message field absent stays MissingField (distinct from InvalidState).
    let mut f = base.clone();
    f.choice_req = None;
    assert!(matches!(glue::v0::types::holder::from_proto(f), Err(GlueError::MissingField { .. })));

    // 2. List element: local named, anonymous, and foreign named option unset wrappers.
    let mut f = base.clone();
    f.list_local = vec![p::WitforgeOptionOfU32 { value: None }];
    assert!(matches!(glue::v0::types::holder::from_proto(f), Err(GlueError::InvalidState { .. })));

    let mut f = base.clone();
    f.list_anon = vec![p::WitforgeOptionOfU32 { value: None }];
    assert!(matches!(glue::v0::types::holder::from_proto(f), Err(GlueError::InvalidState { .. })));

    let mut f = base.clone();
    f.list_dep = vec![p::WitforgeOptionOfU32 { value: None }];
    assert!(matches!(glue::v0::types::holder::from_proto(f), Err(GlueError::InvalidState { .. })));

    // 3. Oneof member (variant & result): local named, anonymous, and foreign named option.
    for variant in [
        p::types_choice::WitforgeValue::Local(p::WitforgeOptionOfU32 { value: None }),
        p::types_choice::WitforgeValue::Anon(p::WitforgeOptionOfU32 { value: None }),
        p::types_choice::WitforgeValue::Dep(p::WitforgeOptionOfU32 { value: None }),
    ] {
        let mut f = base.clone();
        f.choice_req = Some(p::TypesChoice { witforge_value: Some(variant) });
        assert!(matches!(glue::v0::types::holder::from_proto(f), Err(GlueError::InvalidState { .. })));
    }

    let mut f = base.clone();
    f.res_local = Some(p::WitforgeResultOfOptionOfU32AndString {
        value: Some(p::witforge_result_of_option_of_u32_and_string::Value::Ok(
            p::WitforgeOptionOfU32 { value: None },
        )),
    });
    assert!(matches!(glue::v0::types::holder::from_proto(f), Err(GlueError::InvalidState { .. })));

    let mut f = base.clone();
    f.res_anon = Some(p::WitforgeResultOfOptionOfU32AndString {
        value: Some(p::witforge_result_of_option_of_u32_and_string::Value::Ok(
            p::WitforgeOptionOfU32 { value: None },
        )),
    });
    assert!(matches!(glue::v0::types::holder::from_proto(f), Err(GlueError::InvalidState { .. })));

    let mut f = base.clone();
    f.res_dep = Some(p::WitforgeResultOfOptionOfU32AndString {
        value: Some(p::witforge_result_of_option_of_u32_and_string::Value::Ok(
            p::WitforgeOptionOfU32 { value: None },
        )),
    });
    assert!(matches!(glue::v0::types::holder::from_proto(f), Err(GlueError::InvalidState { .. })));

    // 4. Function parameters: local named, anonymous, and foreign named option.
    let bad_req_local_a = p::TypesEchoLocalRequest {
        a: Some(p::TypesNestedLocal { value: None }),
        b: vec![],
        c: Some(p::TypesChoice {
            witforge_value: Some(p::types_choice::WitforgeValue::Local(
                p::WitforgeOptionOfU32 {
                    value: Some(p::witforge_option_of_u32::Value::None(p::Empty {})),
                },
            )),
        }),
    };
    assert!(matches!(
        glue::v0::types::functions::echo_local::from_request(bad_req_local_a),
        Err(GlueError::InvalidState { .. })
    ));
    let bad_req_local_b = p::TypesEchoLocalRequest {
        a: None,
        b: vec![p::WitforgeOptionOfU32 { value: None }],
        c: Some(p::TypesChoice {
            witforge_value: Some(p::types_choice::WitforgeValue::Local(
                p::WitforgeOptionOfU32 {
                    value: Some(p::witforge_option_of_u32::Value::None(p::Empty {})),
                },
            )),
        }),
    };
    assert!(matches!(
        glue::v0::types::functions::echo_local::from_request(bad_req_local_b),
        Err(GlueError::InvalidState { .. })
    ));
    let bad_req_local_c = p::TypesEchoLocalRequest {
        a: None,
        b: vec![],
        c: Some(p::TypesChoice {
            witforge_value: Some(p::types_choice::WitforgeValue::Dep(
                p::WitforgeOptionOfU32 { value: None },
            )),
        }),
    };
    assert!(matches!(
        glue::v0::types::functions::echo_local::from_request(bad_req_local_c),
        Err(GlueError::InvalidState { .. })
    ));
    assert!(matches!(
        glue::v0::types::functions::echo_anon::from_request(p::TypesEchoAnonRequest {
            a: Some(p::TypesNestedAnon { value: None }),
            b: vec![],
        }),
        Err(GlueError::InvalidState { .. })
    ));
    assert!(matches!(
        glue::v0::types::functions::echo_anon::from_request(p::TypesEchoAnonRequest {
            a: None,
            b: vec![p::WitforgeOptionOfU32 { value: None }],
        }),
        Err(GlueError::InvalidState { .. })
    ));
    assert!(matches!(
        glue::v0::types::functions::echo_dep::from_request(p::TypesEchoDepRequest {
            a: Some(p::TypesNestedDep { value: None }),
            b: vec![],
        }),
        Err(GlueError::InvalidState { .. })
    ));
    assert!(matches!(
        glue::v0::types::functions::echo_dep::from_request(p::TypesEchoDepRequest {
            a: None,
            b: vec![p::WitforgeOptionOfU32 { value: None }],
        }),
        Err(GlueError::InvalidState { .. })
    ));

    // 5. Function results: local named, anonymous, and foreign named option.
    assert!(matches!(
        glue::v0::types::functions::echo_local::from_response(p::TypesEchoLocalResponse {
            result: vec![p::WitforgeOptionOfU32 { value: None }],
        }),
        Err(GlueError::InvalidState { .. })
    ));
    assert!(matches!(
        glue::v0::types::functions::echo_anon::from_response(p::TypesEchoAnonResponse {
            result: Some(p::TypesNestedAnon { value: None }),
        }),
        Err(GlueError::InvalidState { .. })
    ));
    assert!(matches!(
        glue::v0::types::functions::echo_dep::from_response(p::TypesEchoDepResponse {
            result: vec![p::WitforgeOptionOfU32 { value: None }],
        }),
        Err(GlueError::InvalidState { .. })
    ));

    // 6. Proto3 JSON: absent and `null` wrapper mean outer `None`, while `{}` is InvalidState.
    let json_absent: p::TypesEchoAnonResponse = serde_json::from_str("{}").unwrap();
    assert_eq!(
        glue::v0::types::functions::echo_anon::from_response(json_absent).unwrap(),
        None
    );
    let json_null: p::TypesEchoAnonResponse =
        serde_json::from_str(r#"{"result": null}"#).unwrap();
    assert_eq!(
        glue::v0::types::functions::echo_anon::from_response(json_null).unwrap(),
        None
    );
    let json_empty_wrapper: p::TypesEchoAnonResponse =
        serde_json::from_str(r#"{"result": {}}"#).unwrap();
    assert!(matches!(
        glue::v0::types::functions::echo_anon::from_response(json_empty_wrapper),
        Err(GlueError::InvalidState { .. })
    ));
    let json_none: p::TypesEchoAnonResponse =
        serde_json::from_str(r#"{"result": {"none": {}}}"#).unwrap();
    assert_eq!(
        glue::v0::types::functions::echo_anon::from_response(json_none).unwrap(),
        Some(None)
    );
    let json_some: p::TypesEchoAnonResponse =
        serde_json::from_str(r#"{"result": {"some": 99}}"#).unwrap();
    assert_eq!(
        glue::v0::types::functions::echo_anon::from_response(json_some).unwrap(),
        Some(Some(99))
    );
}
"##,
    )
    .unwrap();

    run_cargo_program(
        &out,
        "pbjson = \"0.9\"\nprost = \"0.14\"\nserde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n",
        "unset option wrapper and proto3 JSON glue compile/run",
    );

    // Scan every committed golden file and generated example file for trailing whitespace.
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let scan_roots = [
        manifest_dir.join("tests/fixtures"),
        repo_root.join("examples"),
    ];
    let mut stack: Vec<PathBuf> = scan_roots.into_iter().collect();
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| matches!(ext, "rs" | "proto" | "json" | "lock"))
            {
                let content = fs::read_to_string(&path).unwrap();
                for (idx, line) in content.lines().enumerate() {
                    assert!(
                        !line.ends_with(' ') && !line.ends_with('\t'),
                        "committed file `{}` line {} ends with whitespace: {line:?}",
                        path.display(),
                        idx + 1
                    );
                }
            }
        }
    }

    fs::remove_dir_all(&out).unwrap();
}
