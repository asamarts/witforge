//! Model construction tests: shape, ordering, docs, stability, aliases, and refusal.

use std::fs;
use std::path::{Path, PathBuf};

use witforge::load;
use witforge::model::{
    self, Model, QualifiedName, ResultExpr, Stability, TypeDef, TypeDefKind, TypeExpr,
};

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
    assert_eq!(model.package.docs.trim(), "Package docs.");
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
        r"/// Package docs.
package example:stability@0.1.0;

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

fn named(interface: &str, name: &str) -> TypeExpr {
    TypeExpr::Named(QualifiedName {
        interface: interface.into(),
        name: name.into(),
    })
}

#[test]
fn canonical_types_cover_primitives_and_nesting() {
    let model = build("ok");
    let primitives = [
        (TypeExpr::Bool, "bool"),
        (TypeExpr::U8, "u8"),
        (TypeExpr::U16, "u16"),
        (TypeExpr::U32, "u32"),
        (TypeExpr::U64, "u64"),
        (TypeExpr::S8, "s8"),
        (TypeExpr::S16, "s16"),
        (TypeExpr::S32, "s32"),
        (TypeExpr::S64, "s64"),
        (TypeExpr::F32, "f32"),
        (TypeExpr::F64, "f64"),
        (TypeExpr::String, "string"),
    ];
    for (ty, expected) in primitives {
        assert_eq!(model.canonical_type(&ty).unwrap(), expected);
    }

    for name in ["span", "applicability", "caps", "edit"] {
        assert_eq!(
            model.canonical_type(&named("types", name)).unwrap(),
            format!("types/{name}")
        );
    }
    assert_eq!(
        model.canonical_type(&named("analyzer", "finding")).unwrap(),
        "types/finding"
    );
    assert_eq!(
        named("analyzer", "finding").canonical(&model).unwrap(),
        "types/finding"
    );
    assert_eq!(
        model
            .canonical_type(&model.interfaces[1].functions[0].result.clone().unwrap())
            .unwrap(),
        "result<list<types/finding>,string>"
    );

    let nested = TypeExpr::List(Box::new(TypeExpr::Option(Box::new(TypeExpr::Result(
        ResultExpr {
            ok: Some(Box::new(TypeExpr::U8)),
            err: Some(Box::new(TypeExpr::String)),
        },
    )))));
    assert_eq!(
        model.canonical_type(&nested).unwrap(),
        "list<option<result<u8,string>>>"
    );
    assert_eq!(
        model
            .canonical_type(&TypeExpr::Result(ResultExpr::default()))
            .unwrap(),
        "result<_,_>"
    );
    assert_eq!(
        model
            .canonical_type(&TypeExpr::Result(ResultExpr {
                ok: Some(Box::new(TypeExpr::U32)),
                err: None,
            }))
            .unwrap(),
        "result<u32,_>"
    );
    assert_eq!(
        model
            .canonical_type(&TypeExpr::Result(ResultExpr {
                ok: None,
                err: Some(Box::new(TypeExpr::String)),
            }))
            .unwrap(),
        "result<_,string>"
    );
    assert_eq!(
        model
            .canonical_type(&TypeExpr::Result(ResultExpr {
                ok: Some(Box::new(named("analyzer", "finding"))),
                err: Some(Box::new(named("analyzer", "finding"))),
            }))
            .unwrap(),
        "result<types/finding,types/finding>"
    );
}

#[test]
fn canonical_types_expand_named_structural_types_and_use_aliases() {
    let (dir, model) = canonical_model();
    let cases = [
        ("span", "base/span"),
        ("tags", "list<string>"),
        ("maybe-span", "option<base/span>"),
        ("outcome", "result<base/span,string>"),
        ("bare", "result<_,_>"),
        ("only-ok", "result<base/span,_>"),
        ("only-err", "result<_,string>"),
        ("number", "u64"),
        ("alias-number", "u64"),
        ("bytes", "list<u8>"),
        ("result-tags", "result<list<string>,string>"),
        ("span-alias", "base/span"),
        ("outcome-alias", "result<base/span,string>"),
        ("alias-tags", "list<string>"),
        ("alias-alias", "list<string>"),
        ("list-maybe", "list<option<base/span>>"),
        ("maybe-tags", "option<list<string>>"),
        ("list-tags", "list<list<string>>"),
        ("maybe-outcome", "option<result<base/span,string>>"),
    ];
    for (name, expected) in cases {
        let canonical = model.canonical_type(&named("base", name)).unwrap();
        assert_eq!(canonical, expected, "base/{name}");
        assert!(
            !canonical.chars().any(char::is_whitespace),
            "canonical `{canonical}` contains whitespace"
        );
    }
    assert_eq!(
        model.canonical_type(&named("user", "local-span")).unwrap(),
        "base/span"
    );
    assert_eq!(
        model.canonical_type(&named("user", "tags")).unwrap(),
        "list<string>"
    );
    assert_eq!(
        model.canonical_type(&named("user", "maybe-span")).unwrap(),
        "option<base/span>"
    );
    assert_eq!(
        model.canonical_type(&named("user", "outcome")).unwrap(),
        "result<base/span,string>"
    );
    assert_eq!(
        model.canonical_type(&named("user", "alias-alias")).unwrap(),
        "list<string>"
    );
    assert_eq!(
        model.canonical_type(&named("user", "local-tags")).unwrap(),
        "list<string>"
    );
    assert_eq!(
        model.canonical_type(&named("user", "local-maybe")).unwrap(),
        "option<base/span>"
    );
    assert_eq!(
        model
            .canonical_type(&named("user", "local-outcome"))
            .unwrap(),
        "result<base/span,string>"
    );
    let read = &model.interface("user").unwrap().functions[0];
    assert_eq!(
        model.canonical_type(&read.params[0].ty).unwrap(),
        "base/span"
    );
    assert_eq!(
        model.canonical_type(read.result.as_ref().unwrap()).unwrap(),
        "list<string>"
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn canonical_types_reject_missing_types_and_cycles() {
    let mut model = build("ok");
    assert!(model.canonical_type(&named("types", "missing")).is_err());

    let cycle = QualifiedName {
        interface: "analyzer".into(),
        name: "cycle".into(),
    };
    model.interfaces[1].types.push(TypeDef {
        name: "cycle".into(),
        docs: String::new(),
        stability: Stability::Unknown,
        kind: TypeDefKind::Alias(TypeExpr::Named(cycle.clone())),
    });
    assert!(model.canonical_type(&TypeExpr::Named(cycle)).is_err());

    for (name, kind) in [
        (
            "cycle-list",
            TypeDefKind::List(TypeExpr::Named(QualifiedName {
                interface: "analyzer".into(),
                name: "cycle-list".into(),
            })),
        ),
        (
            "cycle-option",
            TypeDefKind::Option(TypeExpr::Named(QualifiedName {
                interface: "analyzer".into(),
                name: "cycle-option".into(),
            })),
        ),
        (
            "cycle-result",
            TypeDefKind::Result(ResultExpr {
                ok: Some(Box::new(TypeExpr::Named(QualifiedName {
                    interface: "analyzer".into(),
                    name: "cycle-result".into(),
                }))),
                err: None,
            }),
        ),
    ] {
        let name = QualifiedName {
            interface: "analyzer".into(),
            name: name.into(),
        };
        model.interfaces[1].types.push(TypeDef {
            name: name.name.clone(),
            docs: String::new(),
            stability: Stability::Unknown,
            kind,
        });
        assert!(model.canonical_type(&TypeExpr::Named(name)).is_err());
    }
}

#[test]
fn build_preserves_escaped_names_and_use_as_aliases() {
    let dir = std::env::temp_dir().join(format!("witforge-model-escaped-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("contract.wit"),
        r"package example:escaped@0.1.0;

interface %type {
    record %where {
        %self: u32,
    }

    %func: func(%self: u32) -> %where;
}

interface user {
    use %type.{%where as local-where};

    call: func(value: local-where) -> local-where;
}
",
    )
    .unwrap();

    let model = model::build(&load(&dir).unwrap()).unwrap();
    let types = model.interface("type").unwrap();
    let record = &types.types[0];
    assert_eq!(record.name, "where");
    match &record.kind {
        TypeDefKind::Record(fields) => assert_eq!(fields[0].name, "self"),
        other => panic!("where should be a record, got {other:?}"),
    }
    assert_eq!(types.functions[0].name, "func");
    assert_eq!(types.functions[0].params[0].name, "self");

    let user = model.interface("user").unwrap();
    let alias = &user.types[0];
    assert_eq!(alias.name, "local-where");
    match &alias.kind {
        TypeDefKind::Alias(TypeExpr::Named(target)) => {
            assert_eq!(target.interface, "type");
            assert_eq!(target.name, "where");
        }
        other => panic!("local-where should be an alias, got {other:?}"),
    }
    let call = &user.functions[0];
    assert_eq!(call.params[0].name, "value");
    assert_eq!(
        call.params[0].ty,
        TypeExpr::Named(QualifiedName {
            interface: "user".into(),
            name: "local-where".into(),
        })
    );
    assert_eq!(call.result, Some(call.params[0].ty.clone()));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn build_accepts_included_world_use_aliases_and_interfaces() {
    let dir = std::env::temp_dir().join(format!("witforge-model-include-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("contract.wit"),
        r"package example:included@0.1.0;

interface types {
    record span {
        start: u64,
    }
}

world base {
    export types;
    use types.{span};
}

world plugin {
    include base;
}
",
    )
    .unwrap();

    let model = model::build(&load(&dir).unwrap()).unwrap();
    let world_names: Vec<&str> = model
        .worlds
        .iter()
        .map(|world| world.name.as_str())
        .collect();
    assert_eq!(world_names, ["base", "plugin"]);
    let plugin = model
        .worlds
        .iter()
        .find(|world| world.name == "plugin")
        .unwrap();
    let exports: Vec<&str> = plugin
        .exports
        .iter()
        .map(|interface| interface.interface.as_str())
        .collect();
    assert_eq!(exports, ["types"]);

    fs::remove_dir_all(&dir).unwrap();
}

fn canonical_model() -> (PathBuf, Model) {
    let dir = std::env::temp_dir().join(format!("witforge-model-canonical-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("contract.wit"),
        r"package example:canonical@0.1.0;

interface base {
    record span {
        start: u64,
    }

    type tags = list<string>;
    type maybe-span = option<span>;
    type outcome = result<span, string>;
    type bare = result;
    type only-ok = result<span>;
    type only-err = result<_, string>;
    type number = u64;
    type alias-number = number;
    type bytes = list<u8>;
    type result-tags = result<tags, string>;
    type span-alias = span;
    type outcome-alias = outcome;
    type alias-tags = tags;
    type alias-alias = alias-tags;
    type list-maybe = list<maybe-span>;
    type maybe-tags = option<tags>;
    type list-tags = list<tags>;
    type maybe-outcome = option<outcome>;
}

interface user {
    use base.{span as local-span, tags, tags as local-tags, maybe-span, maybe-span as local-maybe, outcome, outcome as local-outcome, bare, only-ok, only-err, number, alias-number, bytes, result-tags, span-alias, outcome-alias, alias-tags, alias-alias, list-maybe, maybe-tags, list-tags, maybe-outcome};

    read: func(value: local-span) -> tags;
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
