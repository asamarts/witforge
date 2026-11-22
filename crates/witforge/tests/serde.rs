//! Opt-in serde derives must compile, round-trip supported values, and pin documented JSON names
//! and `serde_json` limitations (`W10-071`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::emit::rust;
use witforge::{load, model};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/serde")
}

#[allow(clippy::too_many_lines)]
#[test]
fn serde_is_opt_in_and_roundtrips() {
    let contract = load(&fixture()).unwrap();
    let model = model::build(&contract).unwrap();
    assert_eq!(
        rust::emit_version(&model).unwrap(),
        rust::emit_version_with_serde(&model, false).unwrap()
    );
    let source = rust::emit_version_with_serde(&model, true).unwrap();
    assert!(
        source.contains(
            "#[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]"
        ),
        "{source}"
    );

    // Build two adjacent versions to pin cross-version Serde reading vs `convert.rs`.
    let output = std::env::temp_dir().join(format!("witforge-serde-{}", std::process::id()));
    let _ = fs::remove_dir_all(&output);
    let wit_root = output.join("wit");
    let v01 = wit_root.join("v0.1");
    let v02 = wit_root.join("v0.2");
    fs::create_dir_all(&v01).unwrap();
    fs::create_dir_all(&v02).unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package example:evolve@0.1.0;\n\
         interface types {\n\
             record doc { id: u32 }\n\
         }\n",
    )
    .unwrap();
    fs::write(
        v02.join("contract.wit"),
        "package example:evolve@0.2.0;\n\
         interface types {\n\
             record doc { id: u32, note: option<string> }\n\
             record doc-with-list { id: u32, tags: list<string> }\n\
         }\n",
    )
    .unwrap();
    let v01_model = model::build(&load(&v01).unwrap()).unwrap();
    let v02_model = model::build(&load(&v02).unwrap()).unwrap();
    let v01_src = rust::emit_version_with_serde(&v01_model, true).unwrap();
    let v02_src = rust::emit_version_with_serde(&v02_model, true).unwrap();

    fs::create_dir_all(output.join("src")).unwrap();
    fs::write(
        output.join("Cargo.toml"),
        r#"[package]
name = "witforge-serde-check"
version = "0.0.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .unwrap();

    let harness = r#"
mod evolve_v0_1 {
__V01__
}

mod evolve_v0_2 {
__V02__
}

fn base_item() -> types::Item {
    types::Item {
        id: 9_007_199_254_740_993, // 2^53 + 1: exact in serde_json
        signed: -9_007_199_254_740_993,
        name: "row".to_string(),
        self_: "self-val".to_string(),
        r#type: "type-val".to_string(),
        url: "https://example.test".to_string(),
        score: 1.5,
        ratio: 0.25,
        note: Some("n".to_string()),
        nested: Some(Some(42)),
        deep: Some(Some(Some("inner".to_string()))),
        aliased_nested: Some(Some(7)),
        nested_list: vec![Some(Some(1)), None],
        tags: vec!["a".to_string()],
        raw: vec![0, 127, 255],
        scores: vec![Some(2.5), None],
        plain_floats: vec![-0.0, 3.25],
        res_both: Ok(7),
        res_ok: Ok(9),
        res_err: Err("bad".to_string()),
        res_bare: Ok(()),
    }
}

fn main() {
    // 1. Full round-trip of all valid values, including 64-bit ints > 2^53, negative zero,
    //    nested options (None and Some(Some(..))), byte lists, and all four result forms.
    let item = base_item();
    let json_text = serde_json::to_string(&item).unwrap();
    let back: types::Item = serde_json::from_str(&json_text).unwrap();
    assert_eq!(item, back);
    assert!(back.plain_floats[0].is_sign_negative(), "-0.0 keeps sign bit");

    // 2. Pin exact JSON keys and representations (Rust spelling, not JSON Schema spelling).
    let val: serde_json::Value = serde_json::from_str(&json_text).unwrap();
    assert_eq!(val["self_"], "self-val");
    assert!(val.get("self").is_none());
    assert_eq!(val["type"], "type-val");
    assert!(val.get("r#type").is_none());
    assert_eq!(val["url"], "https://example.test");
    assert!(val.get("URL").is_none());
    assert_eq!(val["raw"], serde_json::json!([0, 127, 255]));
    assert_eq!(val["res_both"], serde_json::json!({"Ok": 7}));
    assert_eq!(val["res_ok"], serde_json::json!({"Ok": 9}));
    assert_eq!(val["res_err"], serde_json::json!({"Err": "bad"}));
    assert_eq!(val["res_bare"], serde_json::json!({"Ok": null}));
    assert_eq!(
        serde_json::to_value(types::Item {
            res_both: Err("boom".to_string()),
            res_ok: Err(()),
            res_err: Ok(()),
            res_bare: Err(()),
            ..base_item()
        })
        .unwrap()["res_ok"],
        serde_json::json!({"Err": null})
    );
    assert_eq!(
        serde_json::to_value(types::Item {
            res_err: Ok(()),
            ..base_item()
        })
        .unwrap()["res_err"],
        serde_json::json!({"Ok": null})
    );
    assert_eq!(
        serde_json::to_value(types::Item {
            res_bare: Err(()),
            ..base_item()
        })
        .unwrap()["res_bare"],
        serde_json::json!({"Err": null})
    );

    let state = types::State::DoneNow;
    let state_json = serde_json::to_string(&state).unwrap();
    assert_eq!(state_json, "\"DoneNow\"");
    assert_eq!(serde_json::from_str::<types::State>(&state_json).unwrap(), state);

    let unit_event = types::Event::Started;
    let unit_json = serde_json::to_string(&unit_event).unwrap();
    assert_eq!(unit_json, "\"Started\"");
    assert_eq!(serde_json::from_str::<types::Event>(&unit_json).unwrap(), unit_event);

    let payload_event = types::Event::Item(item.clone());
    let payload_val = serde_json::to_value(&payload_event).unwrap();
    assert!(payload_val.get("Item").is_some());
    assert_eq!(
        serde_json::from_value::<types::Event>(payload_val).unwrap(),
        payload_event
    );

    let marks = types::Marks { trace: true, read_only: false };
    let marks_val = serde_json::to_value(&marks).unwrap();
    assert_eq!(marks_val, serde_json::json!({"trace": true, "read_only": false}));
    assert_eq!(serde_json::from_value::<types::Marks>(marks_val).unwrap(), marks);

    let alias: types::AliasItem = item.clone();
    assert_eq!(
        serde_json::from_str::<types::AliasItem>(&serde_json::to_string(&alias).unwrap()).unwrap(),
        alias
    );

    // 3. Pin documented `serde_json` losses on nested options:
    //    `Some(None)` (2-level, 3-level, in list, through alias) serializes as `null` and
    //    deserializes back as `None`.
    let nested_loss = types::Item {
        nested: Some(None),
        deep: Some(Some(None)),
        aliased_nested: Some(None),
        nested_list: vec![Some(None), None, Some(Some(5))],
        ..base_item()
    };
    let nested_val = serde_json::to_value(&nested_loss).unwrap();
    assert_eq!(nested_val["nested"], serde_json::Value::Null);
    assert_eq!(nested_val["deep"], serde_json::Value::Null);
    assert_eq!(nested_val["aliased_nested"], serde_json::Value::Null);
    assert_eq!(nested_val["nested_list"], serde_json::json!([null, null, 5]));
    let nested_back: types::Item = serde_json::from_value(nested_val).unwrap();
    assert_eq!(nested_back.nested, None);
    assert_eq!(nested_back.deep, None);
    assert_eq!(nested_back.aliased_nested, None);
    assert_eq!(nested_back.nested_list, vec![None, None, Some(Some(5))]);

    // 4. Pin documented `serde_json` losses on non-finite floats (NaN, +Inf, -Inf):
    //    field / list element serializes as `null` and fails to deserialize;
    //    option payload `Some(NaN)` serializes as `null` and deserializes as `None`.
    for non_finite in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let field_item = types::Item { score: non_finite, ..base_item() };
        let field_json = serde_json::to_string(&field_item).unwrap();
        let field_val: serde_json::Value = serde_json::from_str(&field_json).unwrap();
        assert_eq!(field_val["score"], serde_json::Value::Null);
        assert!(serde_json::from_str::<types::Item>(&field_json).is_err());

        let list_item = types::Item { plain_floats: vec![non_finite], ..base_item() };
        let list_json = serde_json::to_string(&list_item).unwrap();
        assert!(serde_json::from_str::<types::Item>(&list_json).is_err());

        let opt_item = types::Item { scores: vec![Some(non_finite)], ..base_item() };
        let opt_json = serde_json::to_string(&opt_item).unwrap();
        let opt_back: types::Item = serde_json::from_str(&opt_json).unwrap();
        assert_eq!(opt_back.scores, vec![None]);
    }
    let f32_nan = types::Item { ratio: f32::NAN, ..base_item() };
    let f32_json = serde_json::to_string(&f32_nan).unwrap();
    assert!(serde_json::from_str::<types::Item>(&f32_json).is_err());

    // 5. Pin reading rules vs JSON Schema:
    //    - absent `option` field deserializes as `None`;
    //    - unknown fields are silently ignored.
    let mut partial = serde_json::to_value(&item).unwrap();
    let obj = partial.as_object_mut().unwrap();
    obj.remove("note");
    obj.insert("unknown_extra_field".to_string(), serde_json::json!(123));
    let partial_back: types::Item = serde_json::from_value(partial).unwrap();
    assert_eq!(partial_back.note, None);

    // 6. Pin cross-version reading (v0.1 JSON read as v0.2):
    //    added `option<T>` becomes `None`; added `list<T>` fails with a missing-field error
    //    (unlike `convert.rs`, which defaults both).
    let old_doc = evolve_v0_1::types::Doc { id: 11 };
    let old_json = serde_json::to_string(&old_doc).unwrap();
    let new_doc: evolve_v0_2::types::Doc = serde_json::from_str(&old_json).unwrap();
    assert_eq!(new_doc, evolve_v0_2::types::Doc { id: 11, note: None });
    assert!(serde_json::from_str::<evolve_v0_2::types::DocWithList>(&old_json).is_err());
}
"#;

    let combined = format!(
        "{source}\n{}",
        harness
            .replace("__V01__", &v01_src)
            .replace("__V02__", &v02_src)
    );
    fs::write(output.join("src/main.rs"), combined).unwrap();
    let result = Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--manifest-path"])
        .arg(output.join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "serde-generated Rust did not compile or pass assertions:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(&output).unwrap();
}
