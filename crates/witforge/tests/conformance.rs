//! 1.0 conformance matrix: every supported WIT shape and every emitter.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use witforge::emit::{glue, json_schema, proto, rust};
use witforge::load;
use witforge::lock::WireLock;
use witforge::model::{self, TypeDefKind, TypeExpr};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn collect_expr(expr: &TypeExpr, primitives: &mut BTreeSet<&'static str>, nested: &mut bool) {
    match expr {
        TypeExpr::Bool => {
            primitives.insert("bool");
        }
        TypeExpr::U8 => {
            primitives.insert("u8");
        }
        TypeExpr::U16 => {
            primitives.insert("u16");
        }
        TypeExpr::U32 => {
            primitives.insert("u32");
        }
        TypeExpr::U64 => {
            primitives.insert("u64");
        }
        TypeExpr::S8 => {
            primitives.insert("s8");
        }
        TypeExpr::S16 => {
            primitives.insert("s16");
        }
        TypeExpr::S32 => {
            primitives.insert("s32");
        }
        TypeExpr::S64 => {
            primitives.insert("s64");
        }
        TypeExpr::F32 => {
            primitives.insert("f32");
        }
        TypeExpr::F64 => {
            primitives.insert("f64");
        }
        TypeExpr::String => {
            primitives.insert("string");
        }
        TypeExpr::Named(_) => {}
        TypeExpr::List(element) => {
            *nested = true;
            collect_expr(element, primitives, nested);
        }
        TypeExpr::Option(payload) => {
            *nested = true;
            collect_expr(payload, primitives, nested);
        }
        TypeExpr::Result(result) => {
            *nested = true;
            if let Some(ok) = &result.ok {
                collect_expr(ok, primitives, nested);
            }
            if let Some(err) = &result.err {
                collect_expr(err, primitives, nested);
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
#[test]
fn the_conformance_fixture_covers_the_model_and_emitter_surface() {
    let model = model::build(&load(&fixture("model")).unwrap()).unwrap();
    let mut primitives = BTreeSet::new();
    let mut nested = false;
    let mut kinds = BTreeSet::new();
    let mut stability = BTreeSet::new();
    let mut members = Vec::new();

    for interface in &model.interfaces {
        stability.insert(format!("{:?}", interface.stability));
        for def in &interface.types {
            kinds.insert(match &def.kind {
                TypeDefKind::Record(_) => "record",
                TypeDefKind::Variant(_) => "variant",
                TypeDefKind::Enum(_) => "enum",
                TypeDefKind::Flags(_) => "flags",
                TypeDefKind::Alias(_) => "alias",
                TypeDefKind::List(_) => "list",
                TypeDefKind::Option(_) => "option",
                TypeDefKind::Result(_) => "result",
            });
            stability.insert(format!("{:?}", def.stability));
            match &def.kind {
                TypeDefKind::Record(fields) => {
                    for field in fields {
                        collect_expr(&field.ty, &mut primitives, &mut nested);
                    }
                }
                TypeDefKind::Variant(cases) | TypeDefKind::Enum(cases) => {
                    for case in cases {
                        if let Some(payload) = &case.payload {
                            collect_expr(payload, &mut primitives, &mut nested);
                        }
                    }
                }
                TypeDefKind::Alias(target)
                | TypeDefKind::List(target)
                | TypeDefKind::Option(target) => collect_expr(target, &mut primitives, &mut nested),
                TypeDefKind::Result(result) => {
                    if let Some(ok) = &result.ok {
                        collect_expr(ok, &mut primitives, &mut nested);
                    }
                    if let Some(err) = &result.err {
                        collect_expr(err, &mut primitives, &mut nested);
                    }
                }
                TypeDefKind::Flags(_) => {}
            }
        }
        for function in &interface.functions {
            stability.insert(format!("{:?}", function.stability));
            for param in &function.params {
                collect_expr(&param.ty, &mut primitives, &mut nested);
            }
            if let Some(result) = &function.result {
                collect_expr(result, &mut primitives, &mut nested);
            }
        }
    }

    assert_eq!(
        primitives,
        BTreeSet::from([
            "bool", "u8", "u16", "u32", "u64", "s8", "s16", "s32", "s64", "f32", "f64", "string",
        ])
    );
    assert!(nested, "the fixture must include nested wrappers");
    assert_eq!(
        kinds,
        BTreeSet::from([
            "record", "variant", "enum", "flags", "alias", "list", "option", "result",
        ])
    );
    assert!(stability.iter().any(|value| value.starts_with("Stable")));
    assert!(stability.iter().any(|value| value.starts_with("Unstable")));
    assert!(stability.iter().any(|value| value == "Unknown"));
    assert_eq!(model.worlds.len(), 1);
    assert_eq!(model.worlds[0].imports.len(), 1);
    assert_eq!(model.worlds[0].exports.len(), 1);
    assert_eq!(model.worlds[0].imports[0].interface, "primitives");
    assert_eq!(model.worlds[0].exports[0].interface, "aliases");

    members.extend(model.members().unwrap());
    let member_kinds: BTreeSet<_> = members.iter().map(|member| member.kind.as_str()).collect();
    assert_eq!(
        member_kinds,
        BTreeSet::from([
            "field",
            "variant-case",
            "enum-case",
            "flag",
            "method",
            "param"
        ])
    );

    let mut lock = WireLock::default();
    lock.reconcile(&members).unwrap();
    let generated = [
        rust::emit_version(&model).unwrap(),
        json_schema::emit_version(&model).unwrap(),
        proto::emit_major(&model, &lock).unwrap(),
        glue::emit_version(&model, &lock, "crate::plain", "crate::proto").unwrap(),
    ];
    for source in generated {
        assert_ne!(source, "");
        assert!(!source.contains('\r'));
    }
}

#[test]
fn the_committed_conformance_matrix_names_every_fixture() {
    let matrix = std::fs::read_to_string(fixture("CONFORMANCE.md")).unwrap_or_else(|_| {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/conformance.md"),
        )
        .unwrap()
    });
    for fixture in [
        "fixtures/model/all.wit",
        "fixtures/emit/rust/all/contract.wit",
        "fixtures/emit/proto/mapping/contract.wit",
        "fixtures/emit/glue/contract/contract.wit",
        "fixtures/versions/wit",
        "fixtures/versions/convert",
        "fixtures/deps-full/wit",
        "fixtures/deps-world/wit",
        "fixtures/deps-convert/wit",
        "fixtures/deps-unsupported/wit",
        "fixtures/serde",
    ] {
        assert!(
            matrix.contains(fixture),
            "conformance matrix lacks {fixture}"
        );
    }
}

#[test]
fn dependency_and_serde_fixtures_cover_every_emitter() {
    for name in [
        "deps-full",
        "deps-world",
        "deps-convert",
        "deps-unsupported",
    ] {
        let root = fixture(name).join("wit");
        let contract = load(&root.join("v0.1")).unwrap();
        let model = model::build(&contract).unwrap();
        assert!(!model.dependencies.is_empty(), "{name}");
        assert!(
            !rust::plan(&root).unwrap().files().is_empty(),
            "{name} rust"
        );
        assert!(
            !proto::plan(&root).unwrap().files().is_empty(),
            "{name} proto"
        );
        assert!(
            !json_schema::plan(&root).unwrap().files().is_empty(),
            "{name} schema"
        );
        assert!(
            !glue::plan(&root, "crate::plain", "crate::proto")
                .unwrap()
                .files()
                .is_empty(),
            "{name} glue"
        );
    }

    let world_contract = load(&fixture("deps-world").join("wit/v0.1")).unwrap();
    let world_model = model::build(&world_contract).unwrap();
    assert_eq!(
        world_model.worlds[0].imports[0].qualified_name(),
        "example:shared/types"
    );

    let serde_model = model::build(&load(&fixture("serde")).unwrap()).unwrap();
    assert_eq!(
        rust::emit_version(&serde_model).unwrap(),
        rust::emit_version_with_serde(&serde_model, false).unwrap()
    );
    assert!(
        rust::emit_version_with_serde(&serde_model, true)
            .unwrap()
            .contains("::serde::Serialize")
    );
}
