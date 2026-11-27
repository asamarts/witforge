//! 1.0 conformance matrix: every supported WIT shape and every emitter.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use witforge::emit::{
    glue, json_schema, proto, python, python_glue, rust, typescript, typescript_glue,
};
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
        TypeExpr::Char => {
            primitives.insert("char");
        }
        TypeExpr::String => {
            primitives.insert("string");
        }
        TypeExpr::Named(_) => {}
        TypeExpr::List(element) | TypeExpr::FixedLengthList(element, _) => {
            *nested = true;
            collect_expr(element, primitives, nested);
        }
        TypeExpr::Map(key, value) => {
            *nested = true;
            collect_expr(key, primitives, nested);
            collect_expr(value, primitives, nested);
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
        other => panic!("uncovered type expression in the conformance fixture: {other:?}"),
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
                TypeDefKind::FixedLengthList(_, _) => "fixed-length-list",
                TypeDefKind::Map(_, _) => "map",
                TypeDefKind::Option(_) => "option",
                TypeDefKind::Result(_) => "result",
                other => panic!("uncovered type definition in the conformance fixture: {other:?}"),
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
                | TypeDefKind::FixedLengthList(target, _)
                | TypeDefKind::Option(target) => collect_expr(target, &mut primitives, &mut nested),
                TypeDefKind::Map(key, value) => {
                    collect_expr(key, &mut primitives, &mut nested);
                    collect_expr(value, &mut primitives, &mut nested);
                }
                TypeDefKind::Result(result) => {
                    if let Some(ok) = &result.ok {
                        collect_expr(ok, &mut primitives, &mut nested);
                    }
                    if let Some(err) = &result.err {
                        collect_expr(err, &mut primitives, &mut nested);
                    }
                }
                TypeDefKind::Flags(_) => {}
                other => panic!("uncovered type definition in the conformance fixture: {other:?}"),
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
    let rust_no_std =
        rust::emit_version_with_options(&model, &rust::RustOptions::new().with_no_std(true))
            .unwrap();
    assert!(!rust_no_std.contains("::std::"));
    let schema_2020 = json_schema::emit_version_with_options(
        &model,
        &json_schema::SchemaOptions::new().with_draft(json_schema::JsonSchemaDraft::Draft202012),
    )
    .unwrap();
    assert!(schema_2020.contains("https://json-schema.org/draft/2020-12/schema"));
    let glue_no_std = glue::emit_version_with_options(
        &model,
        &lock,
        "crate::plain",
        "crate::proto",
        &glue::GlueOptions::new().with_no_std(true),
    )
    .unwrap();
    assert!(!glue_no_std.contains("::std::"));
    let ir_json = witforge::inspect::emit_version_json(&model, Some(&lock)).unwrap();
    assert!(ir_json.contains("\"version\": 1"));
    let generated = [
        rust::emit_version(&model).unwrap(),
        rust_no_std,
        json_schema::emit_version(&model).unwrap(),
        schema_2020,
        proto::emit_major(&model, &lock).unwrap(),
        glue::emit_version(&model, &lock, "crate::plain", "crate::proto").unwrap(),
        glue_no_std,
        ir_json,
    ];
    for source in generated {
        assert_ne!(source, "");
        assert!(!source.contains('\r'));
    }
    let ts_files = typescript::emit_version(&model).unwrap();
    assert!(!ts_files.is_empty());
    for (file_name, source) in &ts_files {
        assert_ne!(source, "", "{file_name}");
        assert!(!source.contains('\r'), "{file_name}");
    }
    let py_files = python::emit_version(&model).unwrap();
    assert!(!py_files.is_empty());
    for (file_name, source) in &py_files {
        assert_ne!(source, "", "{file_name}");
        assert!(!source.contains('\r'), "{file_name}");
    }

    // Confirm `Stability::Deprecated` also round-trips through model, emitters, and inspect IR.
    let deprecated_contract = witforge::load_sources(vec![witforge::SourceFile::new(
        "contract.wit".to_string(),
        "package example:dep@0.2.0;\n\
         @since(version = 0.1.0)\n\
         interface api {\n\
             @since(version = 0.1.0)\n\
             @deprecated(version = 0.2.0)\n\
             record old-item { id: u32 }\n\
         }\n"
        .to_string(),
    )])
    .unwrap();
    let deprecated_model = model::build(&deprecated_contract).unwrap();
    assert!(matches!(
        deprecated_model.interfaces[0].types[0].stability,
        model::Stability::Deprecated { .. }
    ));
    let dep_rust = rust::emit_version(&deprecated_model).unwrap();
    assert!(dep_rust.contains("#[deprecated("));
    let dep_ir = witforge::inspect::emit_version_json(&deprecated_model, None).unwrap();
    assert!(dep_ir.contains("\"kind\":\"deprecated\""));
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
        "fixtures/emit/json/all.schema.json",
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
        assert!(
            !typescript::plan(&root).unwrap().files().is_empty(),
            "{name} ts"
        );
        assert!(
            !typescript_glue::plan(&root, "../plain", "../proto")
                .unwrap()
                .files()
                .is_empty(),
            "{name} ts-glue"
        );
        assert!(
            !python::plan(&root).unwrap().files().is_empty(),
            "{name} py"
        );
        assert!(
            !python_glue::plan(&root, "plain", "proto")
                .unwrap()
                .files()
                .is_empty(),
            "{name} py-glue"
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

    // The JSON Schema conformance fixture exercises all four WIT result forms.
    let json_fixture = model::build(&load(&fixture("emit/rust/all")).unwrap()).unwrap();
    let mut saw_both = false;
    let mut saw_ok_only = false;
    let mut saw_err_only = false;
    let mut saw_bare = false;
    for interface in &json_fixture.interfaces {
        for def in &interface.types {
            if let TypeDefKind::Record(fields) = &def.kind {
                for field in fields {
                    if let TypeExpr::Result(result) = &field.ty {
                        match (result.ok.is_some(), result.err.is_some()) {
                            (true, true) => saw_both = true,
                            (true, false) => saw_ok_only = true,
                            (false, true) => saw_err_only = true,
                            (false, false) => saw_bare = true,
                        }
                    }
                }
            }
        }
    }
    assert!(
        saw_both && saw_ok_only && saw_err_only && saw_bare,
        "JSON Schema conformance fixture must hold all four result forms"
    );
}
