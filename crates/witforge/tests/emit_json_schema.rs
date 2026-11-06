//! JSON Schema emitter tests: golden mapping, Draft 7 validation, layout, and drift.

use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::Validator;
use serde_json::{Value, json};
use witforge::emit::json_schema;
use witforge::load;
use witforge::model;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "witforge-json-schema-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn compile_definition(schema: &Value, definition: &str) -> Validator {
    let pointer = format!(
        "#/definitions/{}",
        definition.replace('~', "~0").replace('/', "~1")
    );
    let wrapper = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "definitions": schema["definitions"].clone(),
        "$ref": pointer,
    });
    jsonschema::validator_for(&wrapper).unwrap()
}

#[allow(clippy::needless_pass_by_value)]
fn validates(schema: &Value, definition: &str, instance: Value) -> bool {
    compile_definition(schema, definition)
        .validate(&instance)
        .is_ok()
}

#[test]
fn emits_the_golden_schema_for_every_mapping_row() {
    let model = model::build(&load(&fixture("emit/rust/all")).unwrap()).unwrap();
    let actual = json_schema::emit_version(&model).unwrap();
    let expected = fs::read_to_string(fixture("emit/json/all.schema.json")).unwrap();
    assert_eq!(actual, expected);
    assert!(actual.starts_with(json_schema::HEADER));
    assert!(actual.ends_with('\n'));
    assert!(!actual.contains('\r'));

    let schema: Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(schema["$schema"], "http://json-schema.org/draft-07/schema#");
    assert!(schema["definitions"].is_object());
    jsonschema::validator_for(&schema).unwrap();
}

#[test]
fn validates_nested_values_and_required_members() {
    let model = model::build(&load(&fixture("emit/rust/all")).unwrap()).unwrap();
    let schema: Value = serde_json::from_str(&json_schema::emit_version(&model).unwrap()).unwrap();

    let payload = json!({"name": "x", "bytes": [1, 2, 255]});
    assert!(validates(&schema, "values/payload", payload.clone()));
    assert!(!validates(&schema, "values/payload", json!({"name": "x"})));
    assert!(!validates(
        &schema,
        "values/payload",
        json!({"name": "x", "bytes": [256]})
    ));

    let record = json!({
        "bool-field": true,
        "u8-field": 255,
        "u16-field": 65535,
        "u32-field": 4_294_967_295_u64,
        "u64-field": 18_446_744_073_709_551_615_u64,
        "s8-field": -128,
        "s16-field": -32768,
        "s32-field": -2_147_483_648,
        "s64-field": -9_223_372_036_854_775_808_i64,
        "f32-field": 1.5,
        "f64-field": 2.5,
        "string-field": "text",
        "bytes-field": [0, 1],
        "list-field": ["a"],
        "nested-field": ["x", null],
        "option-field": null,
        "option-payload": {"name": "p", "bytes": []},
        "result-field": {"ok": 7},
        "result-ok": {"ok": 8},
        "result-err": {"err": "bad"},
        "result-bare": {},
        "variant-field": {"no-payload": null},
    });
    assert!(validates(&schema, "values/record-value", record.clone()));
    let mut missing = record.clone();
    missing.as_object_mut().unwrap().remove("string-field");
    assert!(!validates(&schema, "values/record-value", missing));
    let mut too_large = record.clone();
    too_large["u8-field"] = json!(256);
    assert!(!validates(&schema, "values/record-value", too_large));

    assert!(validates(
        &schema,
        "values/variant-value",
        json!({"with-payload": {"name": "v", "bytes": []}})
    ));
    assert!(validates(
        &schema,
        "values/variant-value",
        json!({"no-payload": null})
    ));
    assert!(!validates(&schema, "values/variant-value", json!({})));
    assert!(!validates(
        &schema,
        "values/variant-value",
        json!({"no-payload": "wrong"})
    ));

    assert!(validates(&schema, "values/enum-value", json!("first")));
    assert!(!validates(&schema, "values/enum-value", json!("third")));
    assert!(validates(
        &schema,
        "values/flag-value",
        json!({"one": true, "two": false})
    ));
    assert!(!validates(
        &schema,
        "values/flag-value",
        json!({"one": true})
    ));

    assert!(validates(
        &schema,
        "values/list-value",
        json!(["one", "two"])
    ));
    assert!(validates(&schema, "values/option-value", json!(null)));
    assert!(validates(&schema, "values/result-value", json!({"ok": 1})));
    assert!(validates(
        &schema,
        "values/result-value",
        json!({"err": "e"})
    ));
    assert!(!validates(
        &schema,
        "values/result-value",
        json!({"ok": 1, "err": "e"})
    ));
    assert!(!validates(&schema, "values/result-value", json!({})));
    assert!(validates(
        &schema,
        "values/alias-value",
        json!({"name": "a", "bytes": []})
    ));
    assert!(validates(
        &schema,
        "values/alias-chain",
        json!({"name": "a", "bytes": []})
    ));
}

#[test]
fn plan_generates_one_schema_per_version_and_checks_drift() {
    let root = fixture("versions/convert");
    let files = json_schema::plan(&root).unwrap();
    assert_eq!(
        files.files().keys().cloned().collect::<Vec<_>>(),
        vec![
            "v0_1.schema.json".to_string(),
            "v0_2.schema.json".to_string()
        ]
    );

    let out = temp("layout").join("out");
    json_schema::generate(&root, &out).unwrap();
    assert!(out.join("v0_1.schema.json").is_file());
    assert!(out.join("v0_2.schema.json").is_file());
    assert_eq!(
        json_schema::check(&root, &out).unwrap(),
        Vec::<String>::new()
    );

    fs::write(
        out.join("v0_1.schema.json"),
        format!("{}\nchanged\n", json_schema::HEADER),
    )
    .unwrap();
    let messages = json_schema::check(&root, &out).unwrap();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("v0_1.schema.json")),
        "{messages:?}"
    );
    fs::remove_dir_all(out.parent().unwrap()).unwrap();
}

#[test]
fn a_trailing_space_or_crlf_is_not_canonical() {
    let root = fixture("versions/convert");
    let out = temp("canonical").join("out");
    json_schema::generate(&root, &out).unwrap();
    let path = out.join("v0_1.schema.json");
    let canonical = fs::read_to_string(&path).unwrap();
    let changed = canonical.replace('\n', "\r\n");
    fs::write(&path, changed).unwrap();
    assert!(
        !json_schema::check(&root, &out).unwrap().is_empty(),
        "CRLF output should be drifted"
    );
    fs::remove_dir_all(out.parent().unwrap()).unwrap();
}
