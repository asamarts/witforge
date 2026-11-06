//! Property tests for lockfile parsing and emitter inputs.

use proptest::prelude::*;

use witforge::emit::{glue, json_schema, proto, rust};
use witforge::lock::WireLock;
use witforge::model::{
    Field, Function, Interface, Model, Package, Param, ResultExpr, Stability, TypeDef, TypeDefKind,
    TypeExpr, Version,
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

fn model(fields: &[u8], params: &[u8], result: u8) -> Model {
    let fields = fields
        .iter()
        .enumerate()
        .map(|(index, seed)| Field {
            name: format!("f{index}"),
            docs: String::new(),
            ty: type_expr(*seed, 0),
        })
        .collect();
    let params = params
        .iter()
        .enumerate()
        .map(|(index, seed)| Param {
            name: format!("p{index}"),
            ty: type_expr(*seed, 0),
        })
        .collect();
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
            types: vec![TypeDef {
                name: "record".to_string(),
                docs: String::new(),
                stability: Stability::Unknown,
                kind: TypeDefKind::Record(fields),
            }],
            functions: vec![Function {
                name: "call".to_string(),
                docs: String::new(),
                stability: Stability::Unknown,
                params,
                result: Some(type_expr(result, 0)),
            }],
        }],
        worlds: Vec::new(),
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
