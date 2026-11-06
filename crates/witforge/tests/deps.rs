//! Standard `wit/deps` dependencies and foreign type references.

use std::fs;
use std::path::{Path, PathBuf};

use witforge::emit::{proto, rust};
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
