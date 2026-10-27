//! Model construction tests: shape, ordering, docs, stability, aliases, and refusal.

use std::fs;
use std::path::{Path, PathBuf};

use witforge::load;
use witforge::model::{self, Model, QualifiedName, ResultExpr, Stability, TypeDefKind, TypeExpr};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn build(name: &str) -> Model {
    model::build(&load(&fixture(name)).unwrap()).unwrap()
}

#[test]
fn build_preserves_package_shape_order_and_docs() {
    let model = build("ok");
    assert_package_and_types(&model);
    assert_analyzer(&model);
    assert_world_and_lookup(&model);
}

fn assert_package_and_types(model: &Model) {
    assert_eq!(model.package.namespace, "example");
    assert_eq!(model.package.name, "plugin");
    assert_eq!(model.package.version.to_string(), "0.1.0");

    let interfaces: Vec<&str> = model
        .interfaces
        .iter()
        .map(|interface| interface.name.as_str())
        .collect();
    assert_eq!(interfaces, ["types", "analyzer"]);
    assert_eq!(
        model.interface("types").unwrap().docs.trim(),
        "Shared contract types."
    );

    let types: Vec<&str> = model.interfaces[0]
        .types
        .iter()
        .map(|def| def.name.as_str())
        .collect();
    assert_eq!(types, ["span", "applicability", "caps", "edit", "finding"]);

    let span = &model.interfaces[0].types[0];
    assert_eq!(span.docs.trim(), "A byte range in a resource.");
    match &span.kind {
        TypeDefKind::Record(fields) => {
            let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
            assert_eq!(names, ["start", "end"]);
            assert_eq!(fields[0].ty, TypeExpr::U64);
            assert_eq!(fields[1].ty, TypeExpr::U64);
        }
        other => panic!("span should be a record, got {other:?}"),
    }

    let edit = &model.interfaces[0].types[3];
    match &edit.kind {
        TypeDefKind::Variant(cases) => {
            assert_eq!(cases[0].name, "replace");
            assert_eq!(
                cases[0].payload,
                Some(TypeExpr::Named(QualifiedName {
                    interface: "types".into(),
                    name: "span".into(),
                }))
            );
            assert_eq!(cases[1].name, "delete-file");
            assert_eq!(cases[1].payload, None);
        }
        other => panic!("edit should be a variant, got {other:?}"),
    }
}

fn assert_analyzer(model: &Model) {
    let analyzer = &model.interfaces[1];
    let alias = &analyzer.types[0];
    assert_eq!(alias.name, "finding");
    match &alias.kind {
        TypeDefKind::Alias(TypeExpr::Named(target)) => {
            assert_eq!(target.interface, "types");
            assert_eq!(target.name, "finding");
        }
        other => panic!("finding alias should be an alias, got {other:?}"),
    }

    let analyze = &analyzer.functions[0];
    assert_eq!(analyze.name, "analyze");
    let params: Vec<&str> = analyze
        .params
        .iter()
        .map(|param| param.name.as_str())
        .collect();
    assert_eq!(params, ["path", "bytes"]);
    assert_eq!(analyze.params[0].ty, TypeExpr::String);
    assert_eq!(analyze.params[1].ty, TypeExpr::List(Box::new(TypeExpr::U8)));
    match &analyze.result {
        Some(TypeExpr::Result(ResultExpr {
            ok: Some(ok),
            err: Some(err),
        })) => {
            assert_eq!(**err, TypeExpr::String);
            match &**ok {
                TypeExpr::List(element) => match &**element {
                    TypeExpr::Named(target) => {
                        assert_eq!(target.interface, "analyzer");
                        assert_eq!(target.name, "finding");
                    }
                    other => panic!("result list element should be a named alias, got {other:?}"),
                },
                other => panic!("result ok should be a list, got {other:?}"),
            }
        }
        other => panic!("analyze should return result<list<finding>, string>, got {other:?}"),
    }
}

fn assert_world_and_lookup(model: &Model) {
    assert_eq!(model.worlds.len(), 1);
    let world = &model.worlds[0];
    assert_eq!(world.name, "plugin");
    // `use types.{finding}` makes the interface available; the type alias itself is not a
    // world item, but the interface dependency is.
    let imports: Vec<&str> = world
        .imports
        .iter()
        .map(|interface| interface.interface.as_str())
        .collect();
    assert_eq!(imports, ["types"]);
    let exports: Vec<&str> = world
        .exports
        .iter()
        .map(|interface| interface.interface.as_str())
        .collect();
    assert_eq!(exports, ["analyzer"]);

    assert!(model.interface("missing").is_none());
    assert_eq!(
        model
            .type_def(&QualifiedName {
                interface: "types".into(),
                name: "span".into(),
            })
            .unwrap()
            .name,
        "span"
    );
}

#[test]
fn build_carries_docs_and_stability_and_keeps_unstable_items() {
    let (dir, model) = stability_model();
    let types = model.interface("types").unwrap();
    assert_eq!(types.docs.trim(), "A stable interface.");
    assert_eq!(
        types.stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    let span = model
        .type_def(&QualifiedName {
            interface: "types".into(),
            name: "span".into(),
        })
        .unwrap();
    assert_eq!(span.docs.trim(), "A stable span.");
    assert_eq!(
        span.stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    let mode = model
        .type_def(&QualifiedName {
            interface: "types".into(),
            name: "mode".into(),
        })
        .unwrap();
    assert_eq!(
        mode.stability,
        Stability::Unstable {
            feature: "experimental".into()
        }
    );
    assert!(matches!(&mode.kind, TypeDefKind::Enum(cases) if cases.len() == 2));

    let function_names: Vec<&str> = types
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(function_names, ["read", "write"]);

    let read = &types.functions[0];
    assert_eq!(read.name, "read");
    assert_eq!(read.docs.trim(), "A stable function.");
    assert_eq!(
        read.stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    assert_eq!(
        read.params[0].ty,
        TypeExpr::Named(QualifiedName {
            interface: "types".into(),
            name: "span".into(),
        })
    );
    assert_eq!(
        read.result,
        Some(TypeExpr::Named(QualifiedName {
            interface: "types".into(),
            name: "span".into(),
        }))
    );
    assert_eq!(types.functions[1].name, "write");
    assert_eq!(types.functions[1].result, None);

    let world = &model.worlds[0];
    assert_eq!(world.docs.trim(), "A stable world.");
    assert_eq!(
        world.stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    assert_eq!(
        world.exports[0].docs.trim(),
        "The exported types interface."
    );
    assert_eq!(
        world.exports[0].stability,
        Stability::Stable {
            since: model.package.version
        }
    );

    fs::remove_dir_all(&dir).unwrap();
}

fn stability_model() -> (PathBuf, Model) {
    let dir = std::env::temp_dir().join(format!("witforge-model-stability-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("contract.wit"),
        r"package example:stability@0.1.0;

/// A stable interface.
@since(version = 0.1.0)
interface types {
    /// A stable span.
    @since(version = 0.1.0)
    record span {
        start: u64,
    }

    /// An experimental mode.
    @unstable(feature = experimental)
    enum mode {
        on,
        off,
    }

    /// A stable function.
    @since(version = 0.1.0)
    read: func(value: span) -> span;

    write: func(value: span);
}

/// A stable world.
@since(version = 0.1.0)
world plugin {
    /// The exported types interface.
    @since(version = 0.1.0)
    export types;
}
",
    )
    .unwrap();

    let model = model::build(&load(&dir).unwrap()).unwrap();
    (dir, model)
}

#[test]
fn build_refuses_subsets_the_converter_cannot_represent() {
    for name in [
        "resource",
        "handle",
        "tuple",
        "map",
        "fixed-length-list",
        "future",
        "stream",
        "char",
        "error-context",
        "world-type",
        "world-function",
        "inline-interface",
        "foreign-type",
        "foreign-interface",
        "unversioned",
        "version-patch",
    ] {
        let contract = load(&fixture(&format!("refused/{name}"))).unwrap();
        assert!(
            model::build(&contract).is_err(),
            "`{name}` should be refused by the model"
        );
    }
}

#[test]
fn public_model_api_is_usable_without_wit_parser() {
    // This integration test can name every public model type and needs no `wit-parser` import
    // or `Contract` field access: the model's construction boundary is opaque.
    let contract = load(&fixture("ok")).unwrap();
    let model: Model = model::build(&contract).unwrap();
    let _: &model::Package = &model.package;
    let _: &model::Interface = &model.interfaces[0];
    let _: &model::TypeDef = &model.interfaces[0].types[0];
    let _: &model::TypeExpr = &model.interfaces[1].functions[0].params[0].ty;
    let _: &model::World = &model.worlds[0];
    assert_eq!(model.package.version.to_string(), "0.1.0");
}

#[test]
fn qualified_names_render_as_interface_slash_type() {
    assert_eq!(
        QualifiedName {
            interface: "types".into(),
            name: "span".into(),
        }
        .to_string(),
        "types/span"
    );
}
