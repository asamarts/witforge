//! Standard `wit/deps` dependencies and foreign type references.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::emit::{glue, json_schema, proto, rust};
use witforge::load;
use witforge::lock::WireLock;
use witforge::model::{self, TypeDefKind, TypeExpr};
use witforge::subset;

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
    assert!(witforge::is_contract_error(&error), "{message}");
    assert!(
        message.contains("duplicate dependency package"),
        "{message}"
    );
    assert!(message.contains("example:shared"), "{message}");
    assert!(message.contains("deps/one"), "{message}");
    assert!(message.contains("deps/two"), "{message}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn foreign_aliases_and_named_structures_keep_their_qualified_name() {
    let root = std::env::temp_dir().join(format!(
        "witforge-deps-canonical-foreign-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("v0.1/deps/shared")).unwrap();
    fs::write(
        root.join("v0.1/deps/shared/types.wit"),
        "package example:shared@0.1.0;\n\
         interface types {\n\
           record item { id: u64 }\n\
           type id = u64;\n\
           type id-chain = id;\n\
           type items = list<item>;\n\
           type maybe = option<item>;\n\
           type outcome = result<item, string>;\n\
         }\n",
    )
    .unwrap();
    fs::write(
        root.join("v0.1/contract.wit"),
        "package example:root@0.1.0;\n\
         interface api {\n\
           use example:shared/types@0.1.0.{id, id-chain, items, maybe, outcome};\n\
           type local-id = id;\n\
           record r {\n\
             a: id, b: id-chain, c: items, d: maybe, e: outcome, f: local-id\n\
           }\n\
         }\n",
    )
    .unwrap();
    let model = model::build(&load(&root.join("v0.1")).unwrap()).unwrap();
    let record = model
        .interface("api")
        .unwrap()
        .types
        .iter()
        .find(|def| def.name == "r")
        .unwrap();
    let TypeDefKind::Record(fields) = &record.kind else {
        panic!("r should be a record");
    };
    let expected = [
        ("a", "example:shared/types/id"),
        ("b", "example:shared/types/id-chain"),
        ("c", "example:shared/types/items"),
        ("d", "example:shared/types/maybe"),
        ("e", "example:shared/types/outcome"),
        ("f", "example:shared/types/id"),
    ];
    for (name, canonical) in expected {
        let field = fields.iter().find(|field| field.name == name).unwrap();
        assert_eq!(
            model.canonical_type(&field.ty).unwrap(),
            canonical,
            "{name}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn two_versions_of_one_dependency_are_refused() {
    let root =
        std::env::temp_dir().join(format!("witforge-deps-two-versions-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("v0.1/deps/shared-a")).unwrap();
    fs::create_dir_all(root.join("v0.1/deps/shared-b")).unwrap();
    fs::write(
        root.join("v0.1/deps/shared-a/types.wit"),
        "package example:shared@0.1.0;\ninterface types { record item { a: u32 } }\n",
    )
    .unwrap();
    fs::write(
        root.join("v0.1/deps/shared-b/types.wit"),
        "package example:shared@1.0.0;\ninterface types { enum item { x, y } }\n",
    )
    .unwrap();
    fs::write(
        root.join("v0.1/contract.wit"),
        "package example:root@0.1.0;\ninterface api {\n\
         use example:shared/types@0.1.0.{item as old-item};\n\
         use example:shared/types@1.0.0.{item as new-item};\n\
         record r { x: old-item, y: new-item }\n}\n",
    )
    .unwrap();
    let error = load(&root.join("v0.1")).unwrap_err();
    let message = format!("{error:#}");
    assert!(witforge::is_contract_error(&error), "{message}");
    assert!(
        message.contains("duplicate dependency package `example:shared`"),
        "{message}"
    );
    assert!(message.contains("0.1.0"), "{message}");
    assert!(message.contains("1.0.0"), "{message}");
    assert!(message.contains("deps/shared-a"), "{message}");
    assert!(message.contains("deps/shared-b"), "{message}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_dependency_may_not_use_the_root_package_name() {
    for dependency_version in ["0.1.0", "0.2.0", "0.3.0"] {
        let root = std::env::temp_dir().join(format!(
            "witforge-deps-root-name-{}-{}",
            dependency_version.replace('.', "-"),
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("v0.1/deps/self")).unwrap();
        fs::write(
            root.join("v0.1/contract.wit"),
            "package example:root@0.2.0;\ninterface api { record item { a: u32 } }\n",
        )
        .unwrap();
        fs::write(
            root.join("v0.1/deps/self/types.wit"),
            format!(
                "package example:root@{dependency_version};\n\
                 interface types {{ record item {{ b: u32 }} }}\n"
            ),
        )
        .unwrap();
        let error = load(&root.join("v0.1")).unwrap_err();
        let message = format!("{error:#}");
        assert!(witforge::is_contract_error(&error), "{message}");
        assert!(
            message.contains("dependency package `example:root`"),
            "{message}"
        );
        assert!(
            message.contains("root package's namespace:name"),
            "{message}"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn equal_names_in_different_namespaces_are_accepted() {
    let root =
        std::env::temp_dir().join(format!("witforge-deps-namespaces-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("v0.1/deps/shared-one")).unwrap();
    fs::create_dir_all(root.join("v0.1/deps/shared-two")).unwrap();
    fs::write(
        root.join("v0.1/deps/shared-one/types.wit"),
        "package example:shared@0.1.0;\ninterface types { record item { a: u32 } }\n",
    )
    .unwrap();
    fs::write(
        root.join("v0.1/deps/shared-two/types.wit"),
        "package acme:shared@0.1.0;\ninterface types { record item { b: u32 } }\n",
    )
    .unwrap();
    fs::write(
        root.join("v0.1/contract.wit"),
        "package example:root@0.1.0;\ninterface api { record r { a: u32 } }\n",
    )
    .unwrap();
    let contract = load(&root.join("v0.1")).unwrap();
    let model = model::build(&contract).unwrap();
    assert_eq!(model.dependencies.len(), 2);
    fs::remove_dir_all(root).unwrap();
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

fn world_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deps-world/wit")
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
    fs::remove_dir_all(&out).unwrap();
    compile_generated_rust(&root, "deps-convert-compile");
}

#[test]
fn foreign_world_interfaces_keep_package_identity() {
    let root = world_fixture();
    let contract = load(&root.join("v0.1")).unwrap();
    let model = model::build(&contract).unwrap();
    let world = &model.worlds[0];
    assert_eq!(world.name, "plugin");
    assert_eq!(world.imports[0].qualified_name(), "example:shared/types");
    assert_eq!(world.imports[0].package.as_ref().unwrap().name, "shared");
    assert_eq!(world.exports[0].qualified_name(), "example:other/types");

    let files = rust::plan(&root).unwrap();
    let worlds = files.files().get("worlds.rs").unwrap();
    assert!(worlds.contains("example:shared/types"), "{worlds}");
    assert!(worlds.contains("example:other/types"), "{worlds}");
    let dependencies = files.files().get("witforge_deps.rs").unwrap();
    assert!(dependencies.contains("pub trait Types"), "{dependencies}");
    compile_generated_rust(&root, "deps-world-rust");

    let proto_out = temp_dir("deps-world-proto");
    let _ = fs::remove_dir_all(&proto_out);
    proto::plan(&root).unwrap().write(&proto_out).unwrap();
    let schema = fs::read_to_string(proto_out.join("v0.proto")).unwrap();
    assert!(
        schema.contains("import \"example.shared.v0.proto\";"),
        "{schema}"
    );
    assert!(
        schema.contains("import \"example.other.v0.proto\";"),
        "{schema}"
    );
    protox::compile([proto_out.join("v0.proto")], [&proto_out]).unwrap();
    fs::remove_dir_all(&proto_out).unwrap();
}

#[test]
fn dependency_schema_files_are_self_contained() {
    let root = world_fixture();
    let out = temp_dir("deps-schema");
    let _ = fs::remove_dir_all(&out);
    json_schema::plan(&root).unwrap().write(&out).unwrap();
    assert!(out.join("v0_1.schema.json").is_file());
    assert!(out.join("example.shared.v0.schema.json").is_file());
    assert!(out.join("example.other.v0.schema.json").is_file());
    let root_schema = fs::read_to_string(out.join("v0_1.schema.json")).unwrap();
    assert!(
        root_schema.contains("example.shared.v0.schema.json#/definitions/types~1thing"),
        "{root_schema}"
    );
    assert!(!root_schema.contains("urn:witforge:"), "{root_schema}");
    let dependency = serde_json::from_str::<serde_json::Value>(
        &fs::read_to_string(out.join("example.shared.v0.schema.json")).unwrap(),
    )
    .unwrap();
    jsonschema::validator_for(&dependency).unwrap();
    fs::remove_dir_all(&out).unwrap();
}

#[test]
fn exposed_dependency_interface_refuses_unsupported_items() {
    let root = temp_dir("deps-world-unsupported");
    let _ = fs::remove_dir_all(&root);
    let version = root.join("v0.1");
    let dependency = version.join("deps/shared");
    fs::create_dir_all(&dependency).unwrap();
    fs::write(
        version.join("contract.wit"),
        r"package example:app@0.1.0;

world plugin {
    import example:shared/types@0.1.0;
}
",
    )
    .unwrap();
    fs::write(
        dependency.join("types.wit"),
        r"package example:shared@0.1.0;

interface types {
    resource session;
    open: func() -> session;
}
",
    )
    .unwrap();
    fs::write(version.join("wire.lock"), WireLock::default().render()).unwrap();

    let contract = load(&version).unwrap();
    let violations = subset::check(&contract);
    assert!(
        violations.iter().any(|violation| {
            violation.location == "example:shared/types/session"
                && violation.message.contains("resources")
        }),
        "{violations:#?}"
    );
    let error = rust::plan(&root).unwrap_err();
    let message = format!("{error:#}");
    assert!(witforge::is_contract_error(&error), "{message}");
    assert!(
        message.contains("example:shared/types/session"),
        "{message}"
    );
    assert!(message.contains("resources"), "{message}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn referenced_unsupported_dependency_type_is_refused() {
    let root = temp_dir("deps-required-unsupported-type");
    let _ = fs::remove_dir_all(&root);
    let version = root.join("v0.1");
    let dependency = version.join("deps/shared");
    fs::create_dir_all(&dependency).unwrap();
    fs::write(
        version.join("contract.wit"),
        r"package example:app@0.1.0;

interface api {
    use example:shared/types@0.1.0.{bad};
    get: func() -> bad;
}
",
    )
    .unwrap();
    fs::write(
        dependency.join("types.wit"),
        r"package example:shared@0.1.0;

interface types {
    record hidden {
        value: char,
    }
    type bad = hidden;
}
",
    )
    .unwrap();
    fs::write(version.join("wire.lock"), WireLock::default().render()).unwrap();

    let contract = load(&version).unwrap();
    let violations = subset::check(&contract);
    assert!(
        violations.iter().any(|violation| {
            violation.location == "example:shared/types/hidden.value"
                && violation.message.contains("`char` is not supported")
        }),
        "{violations:#?}"
    );
    let errors = [
        rust::plan(&root).unwrap_err(),
        proto::plan(&root).unwrap_err(),
        json_schema::plan(&root).unwrap_err(),
        glue::plan(&root, "plain", "proto").unwrap_err(),
    ];
    for error in errors {
        let message = format!("{error:#}");
        assert!(witforge::is_contract_error(&error), "{message}");
        assert!(
            message.contains("example:shared/types/hidden.value"),
            "{message}"
        );
        assert!(message.contains("`char` is not supported"), "{message}");
    }
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn glue_refuses_colliding_dependency_module_names() {
    let root = temp_dir("deps-glue-module-collision");
    let _ = fs::remove_dir_all(&root);
    let version = root.join("v0.1");
    fs::create_dir_all(version.join("deps/first")).unwrap();
    fs::create_dir_all(version.join("deps/second")).unwrap();
    fs::write(
        version.join("contract.wit"),
        r"package example:app@0.1.0;

interface api {
    use my-ns:dep/types@0.1.0.{thing as first-thing};
    use my:ns-dep/types@0.1.0.{thing as second-thing};
}
",
    )
    .unwrap();
    fs::write(
        version.join("deps/first/types.wit"),
        r"package my-ns:dep@0.1.0;

interface types {
    record thing {
        value: string,
    }
}
",
    )
    .unwrap();
    fs::write(
        version.join("deps/second/types.wit"),
        r"package my:ns-dep@0.1.0;

interface types {
    record thing {
        value: string,
    }
}
",
    )
    .unwrap();
    fs::write(version.join("wire.lock"), WireLock::default().render()).unwrap();

    let error = glue::plan(&root, "plain", "proto").unwrap_err();
    let message = format!("{error:#}");
    assert!(witforge::is_contract_error(&error), "{message}");
    assert!(message.contains("my-ns:dep"), "{message}");
    assert!(message.contains("my:ns-dep"), "{message}");
    assert!(message.contains("my_ns_dep"), "{message}");
    fs::remove_dir_all(&root).unwrap();
}

#[derive(Clone, Copy)]
struct DependencySpec {
    version: &'static str,
    docs: &'static str,
    field_docs: &'static str,
    field_type: &'static str,
}

const DEFAULT_DEPENDENCY: DependencySpec = DependencySpec {
    version: "0.1.0",
    docs: "",
    field_docs: "",
    field_type: "string",
};

fn write_dependency_version(root: &Path, minor: u64, dependency_spec: Option<DependencySpec>) {
    let version = root.join(format!("v0.{minor}"));
    fs::create_dir_all(&version).unwrap();
    let use_item = dependency_spec.map_or_else(String::new, |spec| {
        format!("    use example:shared/types@{}.{{thing}};\n", spec.version)
    });
    fs::write(
        version.join("contract.wit"),
        format!(
            r"package example:app@0.{minor}.0;

interface api {{
{use_item}}}
"
        ),
    )
    .unwrap();
    if let Some(spec) = dependency_spec {
        let dependency = version.join("deps/shared");
        fs::create_dir_all(&dependency).unwrap();
        fs::write(
            dependency.join("types.wit"),
            format!(
                r"package example:shared@{version};

interface types {{
    {docs}record thing {{
        {field_docs}value: {field_type},
    }}
}}
",
                version = spec.version,
                docs = spec.docs,
                field_docs = spec.field_docs,
                field_type = spec.field_type
            ),
        )
        .unwrap();
    }
    fs::write(version.join("wire.lock"), WireLock::default().render()).unwrap();
}

#[test]
fn generators_and_diff_refuse_dependency_graph_changes() {
    let changed_docs = DependencySpec {
        docs: "/// Changed dependency docs.\n    ",
        ..DEFAULT_DEPENDENCY
    };
    let changed_version = DependencySpec {
        version: "0.2.0",
        ..DEFAULT_DEPENDENCY
    };
    let changed_shape = DependencySpec {
        field_type: "u64",
        ..DEFAULT_DEPENDENCY
    };
    let changed_field_docs = DependencySpec {
        field_docs: "/// Changed field docs.\n        ",
        ..DEFAULT_DEPENDENCY
    };
    let cases = [
        (
            "added",
            None,
            Some(DEFAULT_DEPENDENCY),
            "dependency package `example:shared` was added",
        ),
        (
            "removed",
            Some(DEFAULT_DEPENDENCY),
            None,
            "dependency package `example:shared` was removed",
        ),
        (
            "version",
            Some(DEFAULT_DEPENDENCY),
            Some(changed_version),
            "dependency package `example:shared` version changes from 0.1.0 to 0.2.0",
        ),
        (
            "shape",
            Some(DEFAULT_DEPENDENCY),
            Some(changed_shape),
            "dependency type `example:shared/types/thing` shape changes",
        ),
        (
            "docs",
            Some(DEFAULT_DEPENDENCY),
            Some(changed_docs),
            "dependency type `example:shared/types/thing` metadata changes",
        ),
        (
            "field-docs",
            Some(DEFAULT_DEPENDENCY),
            Some(changed_field_docs),
            "dependency type `example:shared/types/thing` member metadata changes",
        ),
    ];

    for (case, old, new, expected) in cases {
        let root = temp_dir(&format!("deps-graph-{case}"));
        let _ = fs::remove_dir_all(&root);
        write_dependency_version(&root, 1, old);
        write_dependency_version(&root, 2, new);

        let errors = [
            rust::plan(&root).unwrap_err(),
            proto::plan(&root).unwrap_err(),
            json_schema::plan(&root).unwrap_err(),
            glue::plan(&root, "plain", "proto").unwrap_err(),
            witforge::diff::compare(&root.join("v0.1"), &root.join("v0.2")).unwrap_err(),
        ];
        for error in errors {
            let message = format!("{error:#}");
            assert!(witforge::is_contract_error(&error), "{case}: {message}");
            assert!(message.contains(expected), "{case}: {message}");
            assert!(
                message.contains("identical pinned dependency graph"),
                "{case}: {message}"
            );
        }
        fs::remove_dir_all(&root).unwrap();
    }
}
