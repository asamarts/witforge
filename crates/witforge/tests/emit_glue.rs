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
    // The record `f-request` and the request message of function `f` are both `IFRequest`: proto
    // refuses the schema, and glue, which converts to and from its types, must refuse it too.
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
    let (_, lock) = setup_contract(&version);
    fs::write(version.join("wire.lock"), lock.render()).unwrap();
    rust::plan(&root).unwrap();
    let errors = [
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
    // proto name and is refused by both proto and glue.
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
    let (_, clash_lock) = setup_contract(&clash_v01);
    fs::write(clash_v01.join("wire.lock"), clash_lock.render()).unwrap();
    for error in [
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

    fs::remove_dir_all(&out).unwrap();
}
