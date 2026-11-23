//! Protobuf glue type-conversion tests.

use std::fs;
use std::path::{Path, PathBuf};

use witforge::emit::{glue, proto, rust};
use witforge::load;
use witforge::lock::WireLock;
use witforge::model;

mod support;
use support::{
    ScratchDir, compile_and_run_wit_root, compile_proto_module, run_cargo_program,
    stage_single_version_root,
};

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

#[test]
fn conversions_compile_and_run_against_generated_types() {
    let root = fixture("emit/glue/contract");
    let scratch = ScratchDir::new("glue-conv");
    let wit_root = scratch.path().join("wit");
    stage_single_version_root(&root, &wit_root);
    let main_body = fs::read_to_string(root.join("main.rs")).unwrap();
    compile_and_run_wit_root(
        "glue-run",
        &wit_root,
        true,
        &main_body,
        "generated glue compile/run",
    );
    scratch.finish();
}

#[test]
fn byte_list_aliases_compile_and_roundtrip() {
    let root = fixture("emit/bytes");
    let scratch = ScratchDir::new("glue-bytes");
    let wit_root = scratch.path().join("wit");
    stage_single_version_root(&root, &wit_root);
    compile_and_run_wit_root(
        "glue-bytes-run",
        &wit_root,
        false,
        r"
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
        "byte-list glue compile/run",
    );
    scratch.finish();
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
    compile_proto_module(
        &out.join("proto-work"),
        &out.join("proto_crate/src/wire"),
        false,
        false,
    );
    fs::write(
        out.join("proto_crate/Cargo.toml"),
        "[package]\nname = \"ext_proto\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nprost = \"0.14\"\n",
    )
    .unwrap();
    fs::write(out.join("proto_crate/src/lib.rs"), "pub mod wire;\n").unwrap();

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
        []
    );
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "keep me\n");

    fs::write(out.join("v0.rs"), format!("{}\nchanged\n", glue::HEADER)).unwrap();
    let messages = glue::check(&root, &out, "crate::plain", "crate::proto").unwrap();
    assert!(
        messages
            .iter()
            .any(|finding| finding.message().contains("changed generated")),
        "{messages:?}"
    );
    // A file without the header is one generation refuses to overwrite, and --check says so
    // instead of calling it changed output.
    fs::write(out.join("v0.rs"), "changed\n").unwrap();
    let messages = glue::check(&root, &out, "crate::plain", "crate::proto").unwrap();
    assert!(
        messages
            .iter()
            .any(|finding| finding.message().contains("refuses to overwrite")),
        "{messages:?}"
    );

    let extra = out.join("stale.rs");
    fs::write(&extra, "// @generated by witforge; do not edit.\n").unwrap();
    let messages = glue::check(&root, &out, "crate::plain", "crate::proto").unwrap();
    assert!(
        messages
            .iter()
            .any(|finding| finding.message().contains("extra generated")),
        "{messages:?}"
    );

    fs::remove_file(out.join("v0.rs")).unwrap();
    let messages = glue::check(&root, &out, "crate::plain", "crate::proto").unwrap();
    assert!(
        messages
            .iter()
            .any(|finding| finding.message().contains("missing generated")),
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

    compile_and_run_wit_root(
        "glue-unreferenced",
        &wit_root,
        false,
        r#"
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

    // Compile and round-trip against prost via the shared harness.
    compile_and_run_wit_root(
        "glue-idents",
        &wit_root,
        false,
        r#"
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

    let main_body = format!(
        r#"
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
    );

    compile_and_run_wit_root(
        "glue-kw",
        &wit_root,
        false,
        &main_body,
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

    // Compile `.proto` with prost-build AND pbjson-build via the shared harness so we also test
    // proto3 JSON `null` vs absent vs `{}` (unset wrapper oneof).
    compile_and_run_wit_root(
        "glue-unset",
        &wit_root,
        true,
        r##"
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

#[allow(clippy::too_many_lines)]
#[test]
fn cross_version_glue_decode_matrix_observes_both_directions() {
    use support::{HarnessBatch, QualityChecks, RustEdition};
    use witforge::lock::LockMode;

    let scratch = ScratchDir::new("cross-ver-decode");
    let old_root = scratch.path().join("old_wit");
    let new_root = scratch.path().join("new_wit");
    let old_v01 = old_root.join("v0.1");
    let new_v02 = new_root.join("v0.2");
    fs::create_dir_all(&old_v01).unwrap();
    fs::create_dir_all(&new_v02).unwrap();

    // Every single-major evolution category that keeps valid append-only wire.lock continuity.
    let old_wit = "\
package example:compat@0.1.0;

interface types {
    type byte = u8;

    record rec-add-req { id: u32 }
    record rec-add-opt { id: u32 }
    record rec-add-list { id: u32 }
    record rec-add-bytes { id: u32 }
    record rec-add-byte-alias { id: u32 }

    record rec-rem-req { id: u32, gone: string }
    record rec-rem-opt { id: u32, gone: option<string> }
    record rec-rem-list { id: u32, gone: list<string> }
    record rec-rem-bytes { id: u32, gone: list<u8> }

    record rec-reord { a: u32, b: string }

    enum status-add { open, closed }
    enum status-rem { open, closed, legacy }

    variant event-add { ping, msg(string) }
    variant event-rem { ping, msg(string), legacy(u32) }

    flags perms-add { read }
    flags perms-rem { read, legacy }

    keep-fn: func(x: u32) -> u32;
    rem-fn: func(x: u32) -> u32;
    param-add-req: func(x: u32);
    param-add-opt: func(x: u32);
    param-add-list: func(x: u32);
    param-add-bytes: func(x: u32);
    param-rem-req: func(x: u32, gone: string);
    param-rem-opt: func(x: u32, gone: option<string>);
    param-reord: func(first: u32, second: string);
}
";

    let new_wit = "\
package example:compat@0.2.0;

interface types {
    type byte = u8;

    record rec-add-req { id: u32, req: string }
    record rec-add-opt { id: u32, opt: option<string> }
    record rec-add-list { id: u32, tags: list<string> }
    record rec-add-bytes { id: u32, raw: list<u8> }
    record rec-add-byte-alias { id: u32, raw: list<byte> }

    record rec-rem-req { id: u32 }
    record rec-rem-opt { id: u32 }
    record rec-rem-list { id: u32 }
    record rec-rem-bytes { id: u32 }

    record rec-reord { b: string, a: u32 }

    enum status-add { open, closed, pending }
    enum status-rem { open, closed }

    variant event-add { ping, msg(string), done(u32) }
    variant event-rem { ping, msg(string) }

    flags perms-add { read, write }
    flags perms-rem { read }

    keep-fn: func(x: u32) -> u32;
    add-fn: func(x: u32) -> u32;
    param-add-req: func(x: u32, req: string);
    param-add-opt: func(x: u32, opt: option<string>);
    param-add-list: func(x: u32, tags: list<string>);
    param-add-bytes: func(x: u32, raw: list<u8>);
    param-rem-req: func(x: u32);
    param-rem-opt: func(x: u32);
    param-reord: func(second: string, first: u32);
}
";

    fs::write(old_v01.join("contract.wit"), old_wit).unwrap();
    fs::write(new_v02.join("contract.wit"), new_wit).unwrap();

    let old_model = model::build(&load(&old_v01).unwrap()).unwrap();
    let mut old_lock = WireLock::default();
    old_lock
        .reconcile_with(&old_model.members().unwrap(), LockMode::First)
        .unwrap();
    fs::write(old_v01.join("wire.lock"), old_lock.render()).unwrap();

    let new_model = model::build(&load(&new_v02).unwrap()).unwrap();
    // Strip the order section from the copied baseline so the single-version v0.2 root can
    // exercise reordered record fields / parameters while preserving every wire number and
    // reservation from v0.1.
    let old_rendered = old_lock.render();
    let without_order = old_rendered
        .split_once("# witforge order v1")
        .map_or(old_rendered.as_str(), |(head, _)| head);
    let mut new_lock = WireLock::parse(without_order).unwrap();
    new_lock.reconcile(&new_model.members().unwrap()).unwrap();
    fs::write(new_v02.join("wire.lock"), new_lock.render()).unwrap();

    let mut batch = HarnessBatch::new(
        "cross-ver-decode",
        RustEdition::E2024,
        QualityChecks {
            fmt: false,
            clippy: false,
            missing_docs: false,
            rustdoc: false,
            doctest: false,
        },
    );

    // Mount old (v0.1) and new (v0.2) in the same binary and run real prost binary + pbjson
    // cross-version decode tests in BOTH directions!
    batch.add_wit_root("old_ver", &old_root, true, false, "");
    batch.add_wit_root(
        "new_ver",
        &new_root,
        true,
        false,
        r#"
        use prost::Message as _;
        use crate::old_ver::{glue as og, plain as op, proto as opr};
        use crate::new_ver::{glue as ng, plain as np, proto as npr};

        // 1. Added required field (`rec-add-req`):
        //    new_reads_old = FALSE (MissingField), old_reads_new = TRUE (unknown field ignored in binary).
        let old_bytes = og::v0::types::rec_add_req::to_proto(&op::v0_1::types::RecAddReq { id: 1 }).encode_to_vec();
        let new_from_old = ng::v0::types::rec_add_req::from_proto(
            npr::v0::TypesRecAddReq::decode(old_bytes.as_slice()).unwrap()
        );
        assert!(matches!(new_from_old, Err(ng::GlueError::MissingField { .. })), "{new_from_old:?}");

        let new_bytes = ng::v0::types::rec_add_req::to_proto(&np::v0_2::types::RecAddReq {
            id: 1,
            req: "hi".to_string(),
        }).encode_to_vec();
        let old_from_new = og::v0::types::rec_add_req::from_proto(
            opr::v0::TypesRecAddReq::decode(new_bytes.as_slice()).unwrap()
        ).unwrap();
        assert_eq!(old_from_new.id, 1);

        // 2. Added option<T> field (`rec-add-opt`): BOTH directions succeed in binary!
        let old_bytes = og::v0::types::rec_add_opt::to_proto(&op::v0_1::types::RecAddOpt { id: 2 }).encode_to_vec();
        let new_from_old = ng::v0::types::rec_add_opt::from_proto(
            npr::v0::TypesRecAddOpt::decode(old_bytes.as_slice()).unwrap()
        ).unwrap();
        assert_eq!(new_from_old, np::v0_2::types::RecAddOpt { id: 2, opt: None });

        let new_proto = ng::v0::types::rec_add_opt::to_proto(&np::v0_2::types::RecAddOpt {
            id: 2,
            opt: Some("x".to_string()),
        });
        let new_bytes = new_proto.encode_to_vec();
        let old_from_new = og::v0::types::rec_add_opt::from_proto(
            opr::v0::TypesRecAddOpt::decode(new_bytes.as_slice()).unwrap()
        ).unwrap();
        assert_eq!(old_from_new.id, 2);

        //    Verify binary vs strict proto3-JSON (pbjson) difference:
        //    pbjson rejects the unknown JSON field `opt` when old reads new JSON!
        let new_json = serde_json::to_string(&new_proto).unwrap();
        let old_json_res: Result<opr::v0::TypesRecAddOpt, _> = serde_json::from_str(&new_json);
        assert!(old_json_res.is_err(), "strict pbjson should reject unknown field in {new_json}");

        // 3. Added list<string> field (`rec-add-list`): BOTH directions succeed!
        let old_bytes = og::v0::types::rec_add_list::to_proto(&op::v0_1::types::RecAddList { id: 3 }).encode_to_vec();
        let new_from_old = ng::v0::types::rec_add_list::from_proto(
            npr::v0::TypesRecAddList::decode(old_bytes.as_slice()).unwrap()
        ).unwrap();
        assert_eq!(new_from_old, np::v0_2::types::RecAddList { id: 3, tags: vec![] });

        let new_bytes = ng::v0::types::rec_add_list::to_proto(&np::v0_2::types::RecAddList {
            id: 3,
            tags: vec!["t".to_string()],
        }).encode_to_vec();
        let old_from_new = og::v0::types::rec_add_list::from_proto(
            opr::v0::TypesRecAddList::decode(new_bytes.as_slice()).unwrap()
        ).unwrap();
        assert_eq!(old_from_new.id, 3);

        // 4. Added required list<u8> and list<byte> (`optional bytes`):
        //    new_reads_old = FALSE (MissingField!), old_reads_new = TRUE!
        let old_bytes = og::v0::types::rec_add_bytes::to_proto(&op::v0_1::types::RecAddBytes { id: 4 }).encode_to_vec();
        let new_from_old = ng::v0::types::rec_add_bytes::from_proto(
            npr::v0::TypesRecAddBytes::decode(old_bytes.as_slice()).unwrap()
        );
        assert!(matches!(new_from_old, Err(ng::GlueError::MissingField { .. })), "{new_from_old:?}");

        let new_bytes = ng::v0::types::rec_add_bytes::to_proto(&np::v0_2::types::RecAddBytes {
            id: 4,
            raw: vec![1, 2],
        }).encode_to_vec();
        assert_eq!(
            og::v0::types::rec_add_bytes::from_proto(
                opr::v0::TypesRecAddBytes::decode(new_bytes.as_slice()).unwrap()
            ).unwrap().id,
            4
        );

        let old_bytes = og::v0::types::rec_add_byte_alias::to_proto(&op::v0_1::types::RecAddByteAlias { id: 4 }).encode_to_vec();
        let new_from_old = ng::v0::types::rec_add_byte_alias::from_proto(
            npr::v0::TypesRecAddByteAlias::decode(old_bytes.as_slice()).unwrap()
        );
        assert!(matches!(new_from_old, Err(ng::GlueError::MissingField { .. })), "{new_from_old:?}");

        // 5. Removed required field (`rec-rem-req`):
        //    new_reads_old = TRUE, old_reads_new = FALSE (MissingField).
        let old_bytes = og::v0::types::rec_rem_req::to_proto(&op::v0_1::types::RecRemReq {
            id: 5,
            gone: "bye".to_string(),
        }).encode_to_vec();
        assert_eq!(
            ng::v0::types::rec_rem_req::from_proto(
                npr::v0::TypesRecRemReq::decode(old_bytes.as_slice()).unwrap()
            ).unwrap().id,
            5
        );
        let new_bytes = ng::v0::types::rec_rem_req::to_proto(&np::v0_2::types::RecRemReq { id: 5 }).encode_to_vec();
        let old_from_new = og::v0::types::rec_rem_req::from_proto(
            opr::v0::TypesRecRemReq::decode(new_bytes.as_slice()).unwrap()
        );
        assert!(matches!(old_from_new, Err(og::GlueError::MissingField { .. })), "{old_from_new:?}");

        // 6. Removed option<T> / list<string> fields: BOTH directions succeed!
        let new_bytes = ng::v0::types::rec_rem_opt::to_proto(&np::v0_2::types::RecRemOpt { id: 6 }).encode_to_vec();
        assert_eq!(
            og::v0::types::rec_rem_opt::from_proto(
                opr::v0::TypesRecRemOpt::decode(new_bytes.as_slice()).unwrap()
            ).unwrap(),
            op::v0_1::types::RecRemOpt { id: 6, gone: None }
        );
        let new_bytes = ng::v0::types::rec_rem_list::to_proto(&np::v0_2::types::RecRemList { id: 7 }).encode_to_vec();
        assert_eq!(
            og::v0::types::rec_rem_list::from_proto(
                opr::v0::TypesRecRemList::decode(new_bytes.as_slice()).unwrap()
            ).unwrap(),
            op::v0_1::types::RecRemList { id: 7, gone: vec![] }
        );

        // 7. Removed list<u8> (`optional bytes`): new_reads_old = TRUE, old_reads_new = FALSE (MissingField)!
        let old_bytes = og::v0::types::rec_rem_bytes::to_proto(&op::v0_1::types::RecRemBytes {
            id: 8,
            gone: vec![9],
        }).encode_to_vec();
        assert_eq!(
            ng::v0::types::rec_rem_bytes::from_proto(
                npr::v0::TypesRecRemBytes::decode(old_bytes.as_slice()).unwrap()
            ).unwrap().id,
            8
        );
        let new_bytes = ng::v0::types::rec_rem_bytes::to_proto(&np::v0_2::types::RecRemBytes { id: 8 }).encode_to_vec();
        let old_from_new = og::v0::types::rec_rem_bytes::from_proto(
            opr::v0::TypesRecRemBytes::decode(new_bytes.as_slice()).unwrap()
        );
        assert!(matches!(old_from_new, Err(og::GlueError::MissingField { .. })), "{old_from_new:?}");

        // 8. Reordered record fields (`rec-reord`): BOTH directions succeed!
        let old_bytes = og::v0::types::rec_reord::to_proto(&op::v0_1::types::RecReord {
            a: 10,
            b: "ok".to_string(),
        }).encode_to_vec();
        assert_eq!(
            ng::v0::types::rec_reord::from_proto(
                npr::v0::TypesRecReord::decode(old_bytes.as_slice()).unwrap()
            ).unwrap(),
            np::v0_2::types::RecReord { b: "ok".to_string(), a: 10 }
        );

        // 9. Added enum case (`status-add`): new_reads_old = TRUE, old_reads_new = FALSE (UnknownEnumValue).
        let old_val = og::v0::types::status_add::to_proto(&op::v0_1::types::StatusAdd::Open);
        assert_eq!(ng::v0::types::status_add::from_proto(old_val).unwrap(), np::v0_2::types::StatusAdd::Open);
        let new_val = ng::v0::types::status_add::to_proto(&np::v0_2::types::StatusAdd::Pending);
        let old_res = og::v0::types::status_add::from_proto(new_val);
        assert!(matches!(old_res, Err(og::GlueError::UnknownEnumValue { .. })), "{old_res:?}");

        // 10. Removed enum case (`status-rem`): new_reads_old = FALSE (UnknownEnumValue), old_reads_new = TRUE.
        let old_val = og::v0::types::status_rem::to_proto(&op::v0_1::types::StatusRem::Legacy);
        let new_res = ng::v0::types::status_rem::from_proto(old_val);
        assert!(matches!(new_res, Err(ng::GlueError::UnknownEnumValue { .. })), "{new_res:?}");
        let new_val = ng::v0::types::status_rem::to_proto(&np::v0_2::types::StatusRem::Closed);
        assert_eq!(og::v0::types::status_rem::from_proto(new_val).unwrap(), op::v0_1::types::StatusRem::Closed);

        // 11. Added variant case (`event-add`): new_reads_old = TRUE, old_reads_new = FALSE (UnknownCase).
        let old_bytes = og::v0::types::event_add::to_proto(&op::v0_1::types::EventAdd::Ping).encode_to_vec();
        assert_eq!(
            ng::v0::types::event_add::from_proto(
                npr::v0::TypesEventAdd::decode(old_bytes.as_slice()).unwrap()
            ).unwrap(),
            np::v0_2::types::EventAdd::Ping
        );
        let new_bytes = ng::v0::types::event_add::to_proto(&np::v0_2::types::EventAdd::Done(7)).encode_to_vec();
        let old_res = og::v0::types::event_add::from_proto(
            opr::v0::TypesEventAdd::decode(new_bytes.as_slice()).unwrap()
        );
        assert!(matches!(old_res, Err(og::GlueError::UnknownCase { .. })), "{old_res:?}");

        // 12. Removed variant case (`event-rem`): new_reads_old = FALSE (UnknownCase), old_reads_new = TRUE.
        let old_bytes = og::v0::types::event_rem::to_proto(&op::v0_1::types::EventRem::Legacy(9)).encode_to_vec();
        let new_res = ng::v0::types::event_rem::from_proto(
            npr::v0::TypesEventRem::decode(old_bytes.as_slice()).unwrap()
        );
        assert!(matches!(new_res, Err(ng::GlueError::UnknownCase { .. })), "{new_res:?}");
        let new_bytes = ng::v0::types::event_rem::to_proto(&np::v0_2::types::EventRem::Msg("m".to_string())).encode_to_vec();
        assert_eq!(
            og::v0::types::event_rem::from_proto(
                opr::v0::TypesEventRem::decode(new_bytes.as_slice()).unwrap()
            ).unwrap(),
            op::v0_1::types::EventRem::Msg("m".to_string())
        );

        // 13. Added & removed flags (`perms-add`, `perms-rem`): BOTH directions succeed!
        let old_bytes = og::v0::types::perms_add::to_proto(&op::v0_1::types::PermsAdd { read: true }).encode_to_vec();
        assert_eq!(
            ng::v0::types::perms_add::from_proto(
                npr::v0::TypesPermsAdd::decode(old_bytes.as_slice()).unwrap()
            ).unwrap(),
            np::v0_2::types::PermsAdd { read: true, write: false }
        );
        let new_bytes = ng::v0::types::perms_add::to_proto(&np::v0_2::types::PermsAdd {
            read: true,
            write: true,
        }).encode_to_vec();
        assert_eq!(
            og::v0::types::perms_add::from_proto(
                opr::v0::TypesPermsAdd::decode(new_bytes.as_slice()).unwrap()
            ).unwrap(),
            op::v0_1::types::PermsAdd { read: true }
        );
        let old_bytes = og::v0::types::perms_rem::to_proto(&op::v0_1::types::PermsRem {
            read: true,
            legacy: true,
        }).encode_to_vec();
        assert_eq!(
            ng::v0::types::perms_rem::from_proto(
                npr::v0::TypesPermsRem::decode(old_bytes.as_slice()).unwrap()
            ).unwrap(),
            np::v0_2::types::PermsRem { read: true }
        );
        let new_bytes = ng::v0::types::perms_rem::to_proto(&np::v0_2::types::PermsRem { read: true }).encode_to_vec();
        assert_eq!(
            og::v0::types::perms_rem::from_proto(
                opr::v0::TypesPermsRem::decode(new_bytes.as_slice()).unwrap()
            ).unwrap(),
            op::v0_1::types::PermsRem { read: true, legacy: false }
        );

        // 14. Added & removed functions (method enum):
        //     Added: new accepts old keep-fn, old rejects new add-fn method id.
        //     Removed: new rejects old rem-fn method id, old accepts remaining keep-fn.
        let add_method_num = ng::v0::types::functions::add_fn::METHOD as i32;
        assert!(opr::v0::TypesMethod::try_from(add_method_num).is_err());
        let rem_method_num = og::v0::types::functions::rem_fn::METHOD as i32;
        assert!(npr::v0::TypesMethod::try_from(rem_method_num).is_err());
        let keep_method_num = og::v0::types::functions::keep_fn::METHOD as i32;
        assert_eq!(
            npr::v0::TypesMethod::try_from(keep_method_num).unwrap(),
            ng::v0::types::functions::keep_fn::METHOD
        );

        // 15. Function parameters (required / option / list<string> / list<u8> / reordered):
        //     Added required param: new_reads_old = FALSE, old_reads_new = TRUE.
        let old_req = og::v0::types::functions::param_add_req::to_request(&1).encode_to_vec();
        assert!(matches!(
            ng::v0::types::functions::param_add_req::from_request(
                npr::v0::TypesParamAddReqRequest::decode(old_req.as_slice()).unwrap()
            ),
            Err(ng::GlueError::MissingField { .. })
        ));
        let new_req = ng::v0::types::functions::param_add_req::to_request(&1, &"s".to_string()).encode_to_vec();
        assert_eq!(
            og::v0::types::functions::param_add_req::from_request(
                opr::v0::TypesParamAddReqRequest::decode(new_req.as_slice()).unwrap()
            ).unwrap(),
            1
        );

        //     Added option<T> / list<string> param: BOTH directions succeed.
        let old_req = og::v0::types::functions::param_add_opt::to_request(&2).encode_to_vec();
        assert_eq!(
            ng::v0::types::functions::param_add_opt::from_request(
                npr::v0::TypesParamAddOptRequest::decode(old_req.as_slice()).unwrap()
            ).unwrap(),
            (2, None)
        );
        let old_req = og::v0::types::functions::param_add_list::to_request(&3).encode_to_vec();
        assert_eq!(
            ng::v0::types::functions::param_add_list::from_request(
                npr::v0::TypesParamAddListRequest::decode(old_req.as_slice()).unwrap()
            ).unwrap(),
            (3, vec![])
        );

        //     Added list<u8> param: new_reads_old = FALSE (MissingField)!
        let old_req = og::v0::types::functions::param_add_bytes::to_request(&4).encode_to_vec();
        assert!(matches!(
            ng::v0::types::functions::param_add_bytes::from_request(
                npr::v0::TypesParamAddBytesRequest::decode(old_req.as_slice()).unwrap()
            ),
            Err(ng::GlueError::MissingField { .. })
        ));

        //     Removed required param: new_reads_old = TRUE, old_reads_new = FALSE.
        let new_req = ng::v0::types::functions::param_rem_req::to_request(&5).encode_to_vec();
        assert!(matches!(
            og::v0::types::functions::param_rem_req::from_request(
                opr::v0::TypesParamRemReqRequest::decode(new_req.as_slice()).unwrap()
            ),
            Err(og::GlueError::MissingField { .. })
        ));

        //     Removed option<T> param: BOTH directions succeed.
        let new_req = ng::v0::types::functions::param_rem_opt::to_request(&6).encode_to_vec();
        assert_eq!(
            og::v0::types::functions::param_rem_opt::from_request(
                opr::v0::TypesParamRemOptRequest::decode(new_req.as_slice()).unwrap()
            ).unwrap(),
            (6, None)
        );

        //     Reordered parameters: BOTH directions succeed!
        let old_req = og::v0::types::functions::param_reord::to_request(&7, &"hi".to_string()).encode_to_vec();
        assert_eq!(
            ng::v0::types::functions::param_reord::from_request(
                npr::v0::TypesParamReordRequest::decode(old_req.as_slice()).unwrap()
            ).unwrap(),
            ("hi".to_string(), 7)
        );
        "#,
    );

    batch.verify_and_run("cross_ver_decode");

    // Verify that `witforge::diff::compare_full` assigns the exact same (new_reads_old, old_reads_new)
    // verdict to every single change kind observed above!
    let full = witforge::diff::compare_full(&old_v01, &new_v02).unwrap();
    let verdict_for = |path: &str, element: &str| -> witforge::diff::DecodeVerdict {
        full.analysis
            .report
            .changes
            .iter()
            .zip(full.decode_verdicts.iter())
            .find(|(change, _)| change.path == path && change.element.as_str() == element)
            .map_or_else(
                || {
                    panic!(
                        "missing change ({path}, {element}) in {:?}",
                        full.analysis.report.changes
                    )
                },
                |(_, v)| *v,
            )
    };
    let expected: &[(&str, &str, bool, bool)] = &[
        // (path, element, new_reads_old, old_reads_new)
        ("types/rec-add-req.req", "record field", false, true),
        ("types/rec-add-opt.opt", "record field", true, true),
        ("types/rec-add-list.tags", "record field", true, true),
        ("types/rec-add-bytes.raw", "record field", false, true),
        ("types/rec-add-byte-alias.raw", "record field", false, true),
        ("types/rec-rem-req.gone", "record field", true, false),
        ("types/rec-rem-opt.gone", "record field", true, true),
        ("types/rec-rem-list.gone", "record field", true, true),
        ("types/rec-rem-bytes.gone", "record field", true, false),
        ("types/rec-reord", "record field order", true, true),
        ("types/status-add.pending", "enum case", true, false),
        ("types/status-rem.legacy", "enum case", false, true),
        ("types/event-add.done", "variant case", true, false),
        ("types/event-rem.legacy", "variant case", false, true),
        ("types/perms-add.write", "flag", true, true),
        ("types/perms-rem.legacy", "flag", true, true),
        ("types.add-fn", "function", true, false),
        ("types.rem-fn", "function", false, true),
        ("types.param-add-req.req", "parameter", false, true),
        ("types.param-add-opt.opt", "parameter", true, true),
        ("types.param-add-list.tags", "parameter", true, true),
        ("types.param-add-bytes.raw", "parameter", false, true),
        ("types.param-rem-req.gone", "parameter", true, false),
        ("types.param-rem-opt.gone", "parameter", true, true),
        ("types.param-reord", "parameter order", true, true),
    ];
    for &(path, element, new_reads_old, old_reads_new) in expected {
        let actual = verdict_for(path, element);
        assert_eq!(
            actual,
            witforge::diff::DecodeVerdict {
                new_reads_old,
                old_reads_new,
            },
            "decode verdict mismatch for ({path}, {element})"
        );
    }
    assert_eq!(full.decode_summary.breaking, 14);
    assert_eq!(full.decode_summary.new_reads_old_breaking, 8);
    assert_eq!(full.decode_summary.old_reads_new_breaking, 6);
}
