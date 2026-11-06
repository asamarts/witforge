//! Standard `wit/deps` dependencies and foreign type references.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::emit::{glue, proto, rust};
use witforge::is_contract_error;
use witforge::load;
use witforge::lock::WireLock;
use witforge::model::{self, TypeDefKind, TypeExpr};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deps/wit")
}

#[test]
fn dependency_types_are_qualified_and_emitted_distinctly() {
    let root = fixture();
    let contract = load(&root.join("v0.1")).unwrap();
    let model = model::build(&contract).unwrap();
    let api = model.interface("api").unwrap();
    let item = api.types.iter().find(|def| def.name == "item").unwrap();
    let TypeDefKind::Alias(TypeExpr::Named(name)) = &item.kind else {
        panic!("dependency use should be an alias: {item:?}");
    };
    assert_eq!(name.package(), Some(("example", "shared")));
    assert_eq!(name.interface_name(), "types");
    assert_eq!(name.to_string(), "example:shared/types/item");
    assert!(name.is_foreign());

    let rust = rust::emit_version(&model).unwrap();
    assert!(
        rust.contains("super::super::witforge_deps::example_shared::types::Item"),
        "{rust}"
    );
    let lock = WireLock::parse(&fs::read_to_string(root.join("v0.1/wire.lock")).unwrap()).unwrap();
    let proto = proto::emit_major(&model, &lock).unwrap();
    assert!(proto.contains("example.shared.v0.TypesItem"), "{proto}");
    assert!(
        proto.contains("example.shared.v0.TypesItem result = 1;"),
        "{proto}"
    );
}

#[test]
fn missing_dependency_is_an_error() {
    let root = std::env::temp_dir().join(format!("witforge-deps-missing-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("v0.1")).unwrap();
    fs::write(
        root.join("v0.1/contract.wit"),
        "package example:app@0.1.0;\ninterface api { use missing:types/types@0.1.0.{item}; get: func() -> item; }\n",
    )
    .unwrap();
    let error = load(&root.join("v0.1")).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("missing:types"), "{message}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn ambiguous_dependency_is_an_error() {
    let root = std::env::temp_dir().join(format!("witforge-deps-ambiguous-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("v0.1/deps/one")).unwrap();
    fs::create_dir_all(root.join("v0.1/deps/two")).unwrap();
    let package = "package example:shared@0.1.0;\ninterface types { record item { id: u64 } }\n";
    fs::write(root.join("v0.1/deps/one/types.wit"), package).unwrap();
    fs::write(root.join("v0.1/deps/two/types.wit"), package).unwrap();
    fs::write(
        root.join("v0.1/contract.wit"),
        "package example:app@0.1.0;\ninterface api { use example:shared/types@0.1.0.{item}; get: func() -> item; }\n",
    )
    .unwrap();
    let error = load(&root.join("v0.1")).unwrap_err();
    let message = format!("{error:#}");
    assert!(
        message.contains("duplicate") || message.contains("defined"),
        "{message}"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn generated_protobuf_dependency_bindings_compile() {
    let root = fixture();
    let out = std::env::temp_dir().join(format!("witforge-deps-proto-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    proto::plan(&root).unwrap().write(&out).unwrap();
    assert!(out.join("example.shared.v0.proto").is_file());
    protox::compile([out.join("v0.proto")], [&out]).unwrap();
    fs::remove_dir_all(&out).unwrap();
}

fn advanced_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deps-full/wit")
}

fn unsupported_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deps-unsupported/wit")
}

fn convert_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deps-convert/wit")
}

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("witforge-{name}-{}", std::process::id()))
}

#[allow(clippy::unnecessary_debug_formatting)]
fn compile_generated_rust(root: &Path, name: &str) {
    let out = temp_dir(name);
    let _ = fs::remove_dir_all(&out);
    rust::plan(root).unwrap().write(&out).unwrap();
    let source = out.join("wrapper.rs");
    fs::write(
        &source,
        format!("#[path = {:?}] mod generated;\n", out.join("mod.rs")),
    )
    .unwrap();
    let status = Command::new("rustc")
        .args([
            "--edition=2024",
            "--crate-type=lib",
            "--emit=metadata",
            "-o",
        ])
        .arg(out.join("generated.rmeta"))
        .arg(&source)
        .status()
        .unwrap();
    fs::remove_dir_all(&out).unwrap();
    assert!(status.success(), "generated Rust did not compile: {name}");
}

#[test]
fn advanced_dependency_bindings_are_complete() {
    let root = advanced_fixture();
    let contract = load(&root.join("v0.1")).unwrap();
    let model = model::build(&contract).unwrap();
    assert_eq!(model.dependencies.len(), 3);
    assert!(model.dependency_package("my-ns", "my-dep").is_some());

    let out = temp_dir("deps-full-proto");
    let _ = fs::remove_dir_all(&out);
    proto::plan(&root).unwrap().write(&out).unwrap();
    let root_proto = fs::read_to_string(out.join("v0.proto")).unwrap();
    assert!(
        root_proto.contains("example.shared.v0.TypesThing result = 1;"),
        "{root_proto}"
    );
    assert!(
        root_proto.contains("optional uint64 result = 1;"),
        "{root_proto}"
    );
    assert!(
        root_proto.contains("optional example.shared.v0.TypesChoice result = 1;"),
        "{root_proto}"
    );
    assert!(
        root_proto.contains("repeated uint64 result = 1;"),
        "{root_proto}"
    );
    assert!(
        root_proto.contains("my_ns.my_dep.v0.TypesHyphenThing result = 1;"),
        "{root_proto}"
    );
    let shared = fs::read_to_string(out.join("example.shared.v0.proto")).unwrap();
    assert!(
        shared.contains("import \"example.other.v0.proto\";"),
        "{shared}"
    );
    assert!(
        shared.contains("example.other.v0.TypesOtherThing other = 1;"),
        "{shared}"
    );
    assert!(out.join("my_ns.my_dep.v0.proto").is_file());
    protox::compile([out.join("v0.proto")], [&out]).unwrap();
    fs::remove_dir_all(&out).unwrap();

    compile_generated_rust(&root, "deps-full-rust");
}

#[test]
fn unsupported_dependency_items_do_not_break_the_root() {
    let root = unsupported_fixture();
    let contract = load(&root.join("v0.1")).unwrap();
    let model = model::build(&contract).unwrap();
    let shared = model.dependency_package("example", "shared").unwrap();
    assert!(
        shared
            .interfaces
            .iter()
            .any(|interface| interface.name == "extra")
    );
    assert!(rust::plan(&root).is_ok());
    assert!(proto::plan(&root).is_ok());
}

#[test]
fn foreign_types_convert_across_versions() {
    let root = convert_fixture();
    let out = temp_dir("deps-convert-rust");
    let _ = fs::remove_dir_all(&out);
    let files = rust::plan(&root).unwrap();
    files.write(&out).unwrap();
    let convert = fs::read_to_string(out.join("convert.rs")).unwrap();
    assert!(
        convert.contains("::core::result::Result::Ok((value).clone())"),
        "{convert}"
    );
    fs::remove_dir_all(&out).unwrap();
    compile_generated_rust(&root, "deps-convert-compile");
}

#[test]
fn glue_refuses_foreign_values_as_a_contract_error() {
    let error = glue::plan(&advanced_fixture(), "crate::plain", "crate::pb").unwrap_err();
    assert!(is_contract_error(&error), "{error:#}");
    let message = format!("{error:#}");
    assert!(message.contains("glue for dependency value"), "{message}");
    assert!(message.contains("W9-005"), "{message}");
}
