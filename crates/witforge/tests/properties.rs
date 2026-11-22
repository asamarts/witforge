//! Property tests for lockfile parsing, model shapes, structured contracts, and emitters.

#[path = "../../../fuzz/fuzz_targets/structured_gen.rs"]
mod structured_gen;

use std::fs;
use std::path::Path;

use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

use witforge::diff;
use witforge::emit::{glue, json_schema, proto, rust};
use witforge::lock::WireLock;
use witforge::model::{
    Case, Field, Flag, Function, Interface, Model, Package, Param, QualifiedName, ResultExpr,
    Stability, TypeDef, TypeDefKind, TypeExpr, Version, World, WorldInterface,
};

fn primitive(seed: u8) -> TypeExpr {
    match seed % 12 {
        0 => TypeExpr::Bool,
        1 => TypeExpr::U8,
        2 => TypeExpr::U16,
        3 => TypeExpr::U32,
        4 => TypeExpr::U64,
        5 => TypeExpr::S8,
        6 => TypeExpr::S16,
        7 => TypeExpr::S32,
        8 => TypeExpr::S64,
        9 => TypeExpr::F32,
        10 => TypeExpr::F64,
        _ => TypeExpr::String,
    }
}

fn type_expr(seed: u8, depth: u8) -> TypeExpr {
    if depth >= 2 {
        return primitive(seed);
    }
    match seed % 15 {
        0..=11 => primitive(seed),
        12 => TypeExpr::Option(Box::new(type_expr(seed / 3, depth + 1))),
        13 => TypeExpr::List(Box::new(type_expr(seed / 5, depth + 1))),
        _ => TypeExpr::Result(ResultExpr {
            ok: Some(Box::new(type_expr(seed / 2, depth + 1))),
            err: Some(Box::new(type_expr(seed / 7, depth + 1))),
        }),
    }
}

fn named(interface: &str, name: &str) -> TypeExpr {
    TypeExpr::Named(QualifiedName {
        interface: interface.to_string(),
        name: name.to_string(),
    })
}

fn case(name: &str, payload: Option<TypeExpr>) -> Case {
    Case {
        name: name.to_string(),
        docs: String::new(),
        payload,
    }
}

fn field(name: &str, ty: TypeExpr) -> Field {
    Field {
        name: name.to_string(),
        docs: String::new(),
        ty,
    }
}

/// Build a model containing every supported type definition kind, every [`TypeExpr`], a world,
/// a function, and named aliases and structural types.
#[allow(clippy::too_many_lines)]
fn model(fields: &[u8], params: &[u8], result: u8) -> Model {
    let record0 = TypeDef {
        name: "record0".to_string(),
        docs: String::new(),
        stability: Stability::Unknown,
        kind: TypeDefKind::Record(
            fields
                .iter()
                .enumerate()
                .map(|(index, seed)| field(&format!("f{index}"), type_expr(*seed, 0)))
                .collect(),
        ),
    };
    let record1 = TypeDef {
        name: "record1".to_string(),
        docs: String::new(),
        stability: Stability::Unknown,
        kind: TypeDefKind::Record(vec![
            field("named", named("types", "record0")),
            field(
                "nested-list",
                TypeExpr::List(Box::new(named("types", "list0"))),
            ),
            field(
                "nested-option",
                TypeExpr::Option(Box::new(named("types", "option0"))),
            ),
        ]),
    };
    let variant = TypeDef {
        name: "variant0".to_string(),
        docs: String::new(),
        stability: Stability::Unknown,
        kind: TypeDefKind::Variant(vec![
            case("empty", None),
            case("payload", Some(type_expr(fields[0], 0))),
            case(
                "listed",
                Some(TypeExpr::List(Box::new(type_expr(result, 0)))),
            ),
        ]),
    };
    let enumeration = TypeDef {
        name: "enum0".to_string(),
        docs: String::new(),
        stability: Stability::Unknown,
        kind: TypeDefKind::Enum(vec![case("a", None), case("b", None), case("c", None)]),
    };
    let flags = TypeDef {
        name: "flags0".to_string(),
        docs: String::new(),
        stability: Stability::Unknown,
        kind: TypeDefKind::Flags(vec![
            Flag {
                name: "a".to_string(),
                docs: String::new(),
            },
            Flag {
                name: "b".to_string(),
                docs: String::new(),
            },
        ]),
    };
    let alias = TypeDef {
        name: "alias0".to_string(),
        docs: String::new(),
        stability: Stability::Unknown,
        kind: TypeDefKind::Alias(named("types", "record0")),
    };
    let list = TypeDef {
        name: "list0".to_string(),
        docs: String::new(),
        stability: Stability::Unknown,
        kind: TypeDefKind::List(type_expr(result, 0)),
    };
    let option = TypeDef {
        name: "option0".to_string(),
        docs: String::new(),
        stability: Stability::Unknown,
        kind: TypeDefKind::Option(type_expr(result, 1)),
    };
    let result_type = TypeDef {
        name: "result0".to_string(),
        docs: String::new(),
        stability: Stability::Unknown,
        kind: TypeDefKind::Result(ResultExpr {
            ok: Some(Box::new(type_expr(result, 0))),
            err: Some(Box::new(type_expr(result / 2, 0))),
        }),
    };
    let one_sided_result = TypeDef {
        name: "result1".to_string(),
        docs: String::new(),
        stability: Stability::Unknown,
        kind: TypeDefKind::Result(ResultExpr {
            ok: Some(Box::new(named("types", "record0"))),
            err: None,
        }),
    };
    let function = Function {
        name: "call".to_string(),
        docs: String::new(),
        stability: Stability::Unknown,
        params: params
            .iter()
            .enumerate()
            .map(|(index, seed)| Param {
                name: format!("p{index}"),
                ty: type_expr(*seed, 0),
            })
            .chain(std::iter::once(Param {
                name: "named".to_string(),
                ty: named("types", "record0"),
            }))
            .collect(),
        result: Some(type_expr(result, 0)),
    };
    Model {
        package: Package {
            namespace: "example".to_string(),
            name: "properties".to_string(),
            version: Version {
                major: 0,
                minor: 1,
                patch: 0,
            },
            docs: String::new(),
        },
        interfaces: vec![Interface {
            name: "types".to_string(),
            docs: String::new(),
            stability: Stability::Unknown,
            types: vec![
                record0,
                record1,
                variant,
                enumeration,
                flags,
                alias,
                list,
                option,
                result_type,
                one_sided_result,
            ],
            functions: vec![function],
        }],
        worlds: vec![World {
            name: "world0".to_string(),
            docs: String::new(),
            stability: Stability::Unknown,
            imports: vec![WorldInterface {
                interface: "types".to_string(),
                package: None,
                docs: String::new(),
                stability: Stability::Unknown,
            }],
            exports: vec![WorldInterface {
                interface: "types".to_string(),
                package: None,
                docs: String::new(),
                stability: Stability::Unknown,
            }],
        }],
        dependencies: Vec::new(),
    }
}

proptest! {
    #[test]
    fn arbitrary_lockfile_text_does_not_panic(text in ".{0,3000}") {
        if let Ok(lock) = WireLock::parse(&text) {
            prop_assert!(WireLock::parse(&lock.render()).is_ok());
        }
    }

    #[test]
    fn arbitrary_lockfile_text_with_line_endings_does_not_panic(
        chars in prop::collection::vec(any::<char>(), 0..2000),
    ) {
        let text: String = chars.into_iter().collect();
        if let Ok(lock) = WireLock::parse(&text) {
            prop_assert!(WireLock::parse(&lock.render()).is_ok());
        }
    }

    #[test]
    fn emitters_accept_generated_models_without_panicking(
        fields in prop::collection::vec(any::<u8>(), 1..8),
        params in prop::collection::vec(any::<u8>(), 0..4),
        result in any::<u8>(),
    ) {
        let model = model(&fields, &params, result);
        let members = model.members().unwrap();
        let mut lock = WireLock::default();
        lock.reconcile(&members).unwrap();

        let rust = rust::emit_version(&model).unwrap();
        prop_assert!(!rust.is_empty());
        let proto = proto::emit_major(&model, &lock).unwrap();
        prop_assert!(!proto.is_empty());
        let schema = json_schema::emit_version(&model).unwrap();
        prop_assert!(!schema.is_empty());
        let glue = glue::emit_version(&model, &lock, "crate::plain", "crate::proto").unwrap();
        prop_assert!(!glue.is_empty());
    }
}

#[test]
fn two_version_fixture_plans_conversions_and_diffs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/versions/wit");
    let files = rust::plan(&root).unwrap();
    assert!(
        files.files().keys().any(|name| name == "convert.rs"),
        "the two-version fixture must emit convert.rs"
    );
    let analysis = diff::compare_with_impacts(&root.join("v0.1"), &root.join("v0.2")).unwrap();
    assert!(
        analysis
            .report
            .changes
            .iter()
            .any(|change| change.path == "types/user.old-field"),
        "the fixture must expose its documented removal"
    );
}

#[test]
fn structured_contracts_satisfy_fuzz_invariants() {
    // 1. Every tracked seed in `fuzz/seeds/structured-contract/` and `fuzz/seeds/wit-package/`
    //    must satisfy all compiler invariants cleanly.
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let structured_seeds_dir = repo.join("fuzz/seeds/structured-contract");
    let mut seed_count = 0usize;
    let exercised = std::sync::atomic::AtomicUsize::new(0);
    for entry in fs::read_dir(&structured_seeds_dir).unwrap() {
        let bytes = fs::read(entry.unwrap().path()).unwrap();
        let generated = structured_gen::generate_from_bytes(&bytes);
        let scratch = structured_gen::ScratchDir::new("prop-seed");
        let v01 = scratch.path().join("v0.1");
        generated.write_to(&v01);
        if structured_gen::assert_invariants_for_version_dir(scratch.path(), &v01) {
            exercised.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        seed_count += 1;
    }
    assert!(seed_count >= 4, "expected tracked structured seeds");

    let wit_seeds_dir = repo.join("fuzz/seeds/wit-package");
    for entry in fs::read_dir(&wit_seeds_dir).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        let scratch = structured_gen::ScratchDir::new("prop-wit-seed");
        let v01 = scratch.path().join("v0.1");
        fs::create_dir_all(&v01).unwrap();
        fs::write(v01.join("contract.wit"), text).unwrap();
        let _ = structured_gen::assert_invariants_for_version_dir(scratch.path(), &v01);
    }

    let lock_seeds_dir = repo.join("fuzz/seeds/lockfile");
    for entry in fs::read_dir(&lock_seeds_dir).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        let lock = WireLock::parse(&text).expect("tracked lockfile seed must parse");
        let rendered = lock.render();
        let reparsed = WireLock::parse(&rendered).expect("re-rendered lockfile seed must parse");
        assert_eq!(rendered, reparsed.render());
        assert!(reparsed.is_canonical(&rendered));
    }

    // 2. Fixed-seed bounded proptest over arbitrary byte sequences feeding the structure-aware
    //    contract generator (including one-letter/upper-case names, multi-file roots, and deps).
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: 48,
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &[0x57; 32]),
    );
    let strategy = prop::collection::vec(any::<u8>(), 16..96);
    runner
        .run(&strategy, |bytes| {
            let generated = structured_gen::generate_from_bytes(&bytes);
            let scratch = structured_gen::ScratchDir::new("prop-struct");
            let v01 = scratch.path().join("v0.1");
            generated.write_to(&v01);
            if structured_gen::assert_invariants_for_version_dir(scratch.path(), &v01) {
                exercised.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            Ok(())
        })
        .unwrap();
    let total = exercised.load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        total >= 10,
        "expected at least 10 subset-clean structured contracts to exercise every invariant, got {total}"
    );
}
