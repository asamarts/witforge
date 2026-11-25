//! End-to-end tests for the W11-040 WIT subset expansion: `char`, fixed-length `list<T, N>`,
//! and `map<K, V>` across model, lockfile, subset checker, inspect IR, diff, JSON Schema
//! (Draft 7 + Draft 2020-12), Rust (+ `--serde` including `N > 32` arrays), `convert.rs`,
//! protobuf, `pbjson`, glue (including `InvalidChar` and `InvalidLength`), and `#![no_std]` +
//! `alloc`.

#[path = "support/mod.rs"]
mod support;

use std::fs;
use std::path::Path;
use std::process::Command;

use support::{HarnessBatch, QualityChecks, RustEdition, ScratchDir, ensure_lock, workspace_root};
use witforge::diagnostic::codes;
use witforge::emit::{glue, json_schema, proto, rust};
use witforge::lock::WireLock;
use witforge::{diff, inspect, load, model, subset};

const DEP_WIT: &str = "\
/// Shared dependency package with char, fixed lists, and maps.
package ex:shared@0.1.0;

/// Shared value types.
interface types {
    /// A small shared leaf record.
    record leaf {
        /// Initial letter.
        initial: char,
        /// Fixed 4-byte tag.
        tag: list<u8, 4>,
    }

    /// Named fixed-length byte array over 32 bytes.
    type sha512 = list<u8, 64>;

    /// Named fixed-length list of non-u8 elements.
    type quad = list<u32, 4>;

    /// Named map type.
    type leaf-map = map<string, leaf>;

    /// Named optional map.
    type maybe-leaf-map = option<leaf-map>;

    /// Named optional fixed list.
    type maybe-quad = option<quad>;
}
";

const V01_WIT: &str = "\
/// Contract v0.1 exercising char, fixed-length lists, and maps.
package ex:expand@0.1.0;

/// Expanded value types.
interface types {
    use ex:shared/types@0.1.0.{leaf, sha512, quad, leaf-map, maybe-leaf-map, maybe-quad};

    /// Closed enum used as map value.
    enum status {
        /// Active.
        active,
        /// Inactive.
        inactive,
    }

    /// Small local record.
    record point {
        /// X coordinate.
        x: u32,
        /// Label character.
        label: char,
    }

    /// Named byte alias to verify alias-resolved `list<byte, N>` -> `[u8; N]` / `bytes`.
    type byte = u8;

    /// Named fixed-length byte array using byte alias.
    type digest32 = list<byte, 32>;

    /// Named fixed-length byte array over 32 bytes.
    type digest64 = list<u8, 64>;

    /// Named fixed-length record list.
    type pair = list<point, 2>;

    /// Named map with narrow integer key.
    type byte-labels = map<u8, string>;

    /// Named map with char key.
    type char-counts = map<char, u32>;

    /// Named optional map.
    type maybe-counts = option<map<string, u32>>;

    /// Named optional fixed list.
    type maybe-pair = option<pair>;

    /// Record covering every new construct and nested large-array Serde combinators.
    record payload {
        /// Single Unicode scalar value.
        ch: char,
        /// Optional character.
        opt-ch: option<char>,
        /// Fixed 4-byte array (small).
        small-bytes: list<u8, 4>,
        /// Fixed 32-byte array (Serde built-in boundary).
        mid-bytes: digest32,
        /// Fixed 64-byte array (Serde helper > 32).
        big-bytes: digest64,
        /// Optional 64-byte array.
        opt-big: option<list<u8, 64>>,
        /// List of 40-byte arrays.
        vec-big: list<list<u8, 40>>,
        /// Map to 48-byte arrays.
        map-big: map<string, list<u8, 48>>,
        /// Result carrying >32 arrays on both sides.
        res-big: result<list<u8, 33>, list<u16, 34>>,
        /// Nested fixed-length list of large arrays.
        nested-big: list<list<u8, 40>, 2>,
        /// Fixed list of records.
        points: pair,
        /// Fixed list of u32.
        nums: list<u32, 4>,
        /// Optional fixed list of u32.
        opt-nums: option<list<u32, 4>>,
        /// Optional named fixed list.
        opt-pair: maybe-pair,
        /// Maps covering all 11 primitive key types.
        m-bool: map<bool, u32>,
        /// Unsigned 8-bit key map.
        m-u8: byte-labels,
        /// Unsigned 16-bit key map.
        m-u16: map<u16, status>,
        /// Unsigned 32-bit key map.
        m-u32: map<u32, list<u8>>,
        /// Unsigned 64-bit key map.
        m-u64: map<u64, option<string>>,
        /// Signed 8-bit key map.
        m-s8: map<s8, string>,
        /// Signed 16-bit key map.
        m-s16: map<s16, bool>,
        /// Signed 32-bit key map.
        m-s32: map<s32, list<u32>>,
        /// Signed 64-bit key map.
        m-s64: map<s64, map<string, u32>>,
        /// Char key map.
        m-char: char-counts,
        /// String key map with dependency record values.
        m-dep: map<string, leaf>,
        /// Optional map (anonymous).
        opt-map: option<map<string, u32>>,
        /// Optional map (named).
        named-opt-map: maybe-counts,
        /// Dependency types.
        dep-hash: sha512,
        /// Dependency fixed list.
        dep-quad: quad,
        /// Dependency map.
        dep-map: leaf-map,
        /// Dependency optional map.
        dep-opt-map: maybe-leaf-map,
        /// Dependency optional fixed list.
        dep-opt-quad: maybe-quad,
    }

    /// Variant exercising singular projections of char, fixed lists, and maps.
    variant choice {
        /// Character case.
        letter(char),
        /// Small fixed bytes case.
        small(list<u8, 4>),
        /// Large fixed bytes case (> 32 for Serde).
        large(list<u8, 64>),
        /// Fixed non-u8 list case.
        quad(list<u32, 4>),
        /// Map case.
        table(map<string, u32>),
        /// Foreign map case.
        dep-table(leaf-map),
    }

    /// Echo function exercising direct params and result.
    echo: func(
        ch: char,
        fixed: list<u8, 4>,
        attrs: map<string, u32>,
        data: payload,
    ) -> result<choice, string>;
}
";

const V02_WIT: &str = "\
/// Contract v0.2 adding a defaulted map field.
package ex:expand@0.2.0;

/// Expanded value types.
interface types {
    use ex:shared/types@0.1.0.{leaf, sha512, quad, leaf-map, maybe-leaf-map, maybe-quad};

    /// Closed enum used as map value.
    enum status {
        /// Active.
        active,
        /// Inactive.
        inactive,
    }

    /// Small local record.
    record point {
        /// X coordinate.
        x: u32,
        /// Label character.
        label: char,
    }

    /// Named byte alias to verify alias-resolved `list<byte, N>` -> `[u8; N]` / `bytes`.
    type byte = u8;

    /// Named fixed-length byte array using byte alias.
    type digest32 = list<byte, 32>;

    /// Named fixed-length byte array over 32 bytes.
    type digest64 = list<u8, 64>;

    /// Named fixed-length record list.
    type pair = list<point, 2>;

    /// Named map with narrow integer key.
    type byte-labels = map<u8, string>;

    /// Named map with char key.
    type char-counts = map<char, u32>;

    /// Named optional map.
    type maybe-counts = option<map<string, u32>>;

    /// Named optional fixed list.
    type maybe-pair = option<pair>;

    /// Record covering every new construct plus a defaulted map addition in v0.2.
    record payload {
        /// Single Unicode scalar value.
        ch: char,
        /// Optional character.
        opt-ch: option<char>,
        /// Fixed 4-byte array (small).
        small-bytes: list<u8, 4>,
        /// Fixed 32-byte array (Serde built-in boundary).
        mid-bytes: digest32,
        /// Fixed 64-byte array (Serde helper > 32).
        big-bytes: digest64,
        /// Optional 64-byte array.
        opt-big: option<list<u8, 64>>,
        /// List of 40-byte arrays.
        vec-big: list<list<u8, 40>>,
        /// Map to 48-byte arrays.
        map-big: map<string, list<u8, 48>>,
        /// Result carrying >32 arrays on both sides.
        res-big: result<list<u8, 33>, list<u16, 34>>,
        /// Nested fixed-length list of large arrays.
        nested-big: list<list<u8, 40>, 2>,
        /// Fixed list of records.
        points: pair,
        /// Fixed list of u32.
        nums: list<u32, 4>,
        /// Optional fixed list of u32.
        opt-nums: option<list<u32, 4>>,
        /// Optional named fixed list.
        opt-pair: maybe-pair,
        /// Maps covering all 11 primitive key types.
        m-bool: map<bool, u32>,
        /// Unsigned 8-bit key map.
        m-u8: byte-labels,
        /// Unsigned 16-bit key map.
        m-u16: map<u16, status>,
        /// Unsigned 32-bit key map.
        m-u32: map<u32, list<u8>>,
        /// Unsigned 64-bit key map.
        m-u64: map<u64, option<string>>,
        /// Signed 8-bit key map.
        m-s8: map<s8, string>,
        /// Signed 16-bit key map.
        m-s16: map<s16, bool>,
        /// Signed 32-bit key map.
        m-s32: map<s32, list<u32>>,
        /// Signed 64-bit key map.
        m-s64: map<s64, map<string, u32>>,
        /// Char key map.
        m-char: char-counts,
        /// String key map with dependency record values.
        m-dep: map<string, leaf>,
        /// Optional map (anonymous).
        opt-map: option<map<string, u32>>,
        /// Optional map (named).
        named-opt-map: maybe-counts,
        /// Dependency types.
        dep-hash: sha512,
        /// Dependency fixed list.
        dep-quad: quad,
        /// Dependency map.
        dep-map: leaf-map,
        /// Dependency optional map.
        dep-opt-map: maybe-leaf-map,
        /// Dependency optional fixed list.
        dep-opt-quad: maybe-quad,
        /// Newly added map field defaulted to empty BTreeMap in v0.1 -> v0.2 conversion.
        extra-attrs: map<string, u32>,
    }

    /// Variant exercising singular projections of char, fixed lists, and maps.
    variant choice {
        /// Character case.
        letter(char),
        /// Small fixed bytes case.
        small(list<u8, 4>),
        /// Large fixed bytes case (> 32 for Serde).
        large(list<u8, 64>),
        /// Fixed non-u8 list case.
        quad(list<u32, 4>),
        /// Map case.
        table(map<string, u32>),
        /// Foreign map case.
        dep-table(leaf-map),
    }

    /// Echo function exercising direct params and result.
    echo: func(
        ch: char,
        fixed: list<u8, 4>,
        attrs: map<string, u32>,
        data: payload,
    ) -> result<choice, string>;
}
";

fn write_two_version_root(wit_root: &Path) {
    let v01 = wit_root.join("v0.1");
    let v02 = wit_root.join("v0.2");
    fs::create_dir_all(v01.join("deps/shared")).unwrap();
    fs::create_dir_all(v02.join("deps/shared")).unwrap();
    fs::write(v01.join("deps/shared/contract.wit"), DEP_WIT).unwrap();
    fs::write(v02.join("deps/shared/contract.wit"), DEP_WIT).unwrap();
    fs::write(v01.join("contract.wit"), V01_WIT).unwrap();
    fs::write(v02.join("contract.wit"), V02_WIT).unwrap();

    ensure_lock(&v01);
    let mut v02_lock =
        WireLock::parse(&fs::read_to_string(v01.join("wire.lock")).unwrap()).unwrap();
    let v02_model = model::build(&load(&v02).unwrap()).unwrap();
    v02_lock.reconcile(&v02_model.members().unwrap()).unwrap();
    v02_lock.write_atomic(&v02.join("wire.lock")).unwrap();
}

#[test]
#[allow(clippy::too_many_lines)]
fn subset_expansion_model_lockfile_inspect_diff_and_refusals() {
    let stage = ScratchDir::new("w11-040-core");
    let wit_root = stage.path().join("wit");
    write_two_version_root(&wit_root);

    let v01 = wit_root.join("v0.1");
    let v02 = wit_root.join("v0.2");

    // 1. Subset check passes cleanly on both versions.
    assert!(subset::check(&load(&v01).unwrap()).is_empty());
    assert!(subset::check(&load(&v02).unwrap()).is_empty());

    // 2. Canonical lockfile tokens include `char`, `list<T,N>`, and `map<K,V>` and round-trip.
    let lock_text = fs::read_to_string(v01.join("wire.lock")).unwrap();
    assert!(lock_text.contains("= 1 field char"), "{lock_text}");
    assert!(lock_text.contains("field list<u8,4>"), "{lock_text}");
    assert!(lock_text.contains("field list<u8,32>"), "{lock_text}");
    assert!(lock_text.contains("field list<u8,64>"), "{lock_text}");
    assert!(
        lock_text.contains("field map<string,list<u8,48>>"),
        "{lock_text}"
    );
    assert!(
        lock_text.contains("field map<s64,map<string,u32>>"),
        "{lock_text}"
    );
    assert!(
        lock_text.contains("field map<string,ex:shared/types/leaf>"),
        "{lock_text}"
    );
    let parsed_lock = WireLock::parse(&lock_text).unwrap();
    assert_eq!(parsed_lock.render(), lock_text);

    // 3. `inspect` JSON IR keeps version 1 and emits additive `char`, `fixed-length-list`, `map`.
    let v01_model = model::build(&load(&v01).unwrap()).unwrap();
    let ir_str = inspect::emit_version_json(&v01_model, Some(&parsed_lock)).unwrap();
    let ir: serde_json::Value = serde_json::from_str(&ir_str).unwrap();
    assert_eq!(ir["version"], 1);
    assert!(ir_str.contains("\"kind\":\"char\""), "{ir_str}");
    assert!(
        ir_str.contains("\"kind\": \"fixed-length-list\""),
        "{ir_str}"
    );
    assert!(ir_str.contains("\"length\": 64"), "{ir_str}");
    assert!(ir_str.contains("\"kind\": \"map\""), "{ir_str}");

    // 4. `diff`: adding a map field is `added-defaulted`, wire-compatible, and decode-compatible.
    let full = diff::compare_full(&v01, &v02).unwrap();
    assert_eq!(full.analysis.report.changes.len(), 1, "{full:#?}");
    let change = &full.analysis.report.changes[0];
    let verdict = full.decode_verdicts[0];
    assert_eq!(change.path, "types/payload.extra-attrs");
    assert_eq!(change.kind, diff::ChangeKind::Added { defaulted: true });
    assert!(!change.wire_breaking);
    assert!(!verdict.is_breaking());
    assert!(verdict.new_reads_old);
    assert!(verdict.old_reads_new);

    // 5. `diff`: adding a required `list<u8, 4>`, `list<u32, 4>`, or `char` field is NOT
    //    defaulted, IS decode-breaking (`new_reads_old: false`), and blocks `convert.rs`.
    for (label, added_field) in [
        ("fixed-bytes", "added: list<u8, 4>,"),
        ("fixed-u32", "added: list<u32, 4>,"),
        ("char", "added: char,"),
    ] {
        let dir = stage.path().join(format!("diff-{label}"));
        let a = dir.join("v0.1");
        let b = dir.join("v0.2");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(
            a.join("contract.wit"),
            "package ex:req@0.1.0;\ninterface types { record r { id: u32 } }\n",
        )
        .unwrap();
        fs::write(
            b.join("contract.wit"),
            format!("package ex:req@0.2.0;\ninterface types {{ record r {{ id: u32, {added_field} }} }}\n"),
        )
        .unwrap();
        ensure_lock(&a);
        let mut b_lock =
            WireLock::parse(&fs::read_to_string(a.join("wire.lock")).unwrap()).unwrap();
        b_lock
            .reconcile(&model::build(&load(&b).unwrap()).unwrap().members().unwrap())
            .unwrap();
        b_lock.write_atomic(&b.join("wire.lock")).unwrap();

        let rep = diff::compare_full(&a, &b).unwrap();
        assert_eq!(rep.analysis.report.changes.len(), 1, "{label}: {rep:#?}");
        assert_eq!(
            rep.analysis.report.changes[0].kind,
            diff::ChangeKind::Added { defaulted: false },
            "{label}"
        );
        assert!(rep.decode_verdicts[0].is_breaking(), "{label}");
        assert!(!rep.decode_verdicts[0].new_reads_old, "{label}");
        assert!(rep.decode_verdicts[0].old_reads_new, "{label}");
        assert!(rust::plan(&dir).is_err(), "{label}: convert must refuse");
    }

    // 6. Subset refusals: fixed-length bounds (`WF1021`), raw-ident map-to-enum (`WF1003`),
    //    and prost `get_<field>` / `insert_<field>` accessor collisions (`WF1003`).
    let bad_cases = [
        (
            "zero-len",
            "package ex:b@0.1.0;\ninterface t { record r { v: list<u8, 0> } }\n",
            codes::SUBSET_FIXED_LENGTH_LIST,
        ),
        (
            "over-max-len",
            "package ex:b@0.1.0;\ninterface t { type big = list<u8, 65537>; }\n",
            codes::SUBSET_FIXED_LENGTH_LIST,
        ),
        (
            "map-enum-raw-ident-field",
            "package ex:b@0.1.0;\ninterface t { enum e { a } record r { %match: map<string, e> } }\n",
            codes::IDENTIFIER_COLLISION,
        ),
        (
            "map-enum-raw-ident-param",
            "package ex:b@0.1.0;\ninterface t { enum e { a } f: func(%type: map<string, e>); }\n",
            codes::IDENTIFIER_COLLISION,
        ),
        (
            "map-enum-get-collision",
            "package ex:b@0.1.0;\ninterface t { enum e { a } record r { items: map<string, e>, get-items: bool } }\n",
            codes::IDENTIFIER_COLLISION,
        ),
        (
            "map-enum-insert-collision",
            "package ex:b@0.1.0;\ninterface t { enum e { a } record r { items: map<string, e>, insert-items: bool } }\n",
            codes::IDENTIFIER_COLLISION,
        ),
    ];
    for (name, wit, expected_code) in bad_cases {
        let dir = stage.path().join(format!("refuse-{name}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("contract.wit"), wit).unwrap();
        let diags = subset::check_diagnostics(&load(&dir).unwrap());
        assert!(
            diags.iter().any(|d| d.code == expected_code),
            "{name}: expected {expected_code}, got {diags:#?}"
        );
    }

    // 7. Protobuf emitter includes dependency import even when dependency appears only as map value.
    let map_dep_only = stage.path().join("map-dep-only");
    let md_v01 = map_dep_only.join("v0.1");
    fs::create_dir_all(md_v01.join("deps/shared")).unwrap();
    fs::write(
        md_v01.join("deps/shared/contract.wit"),
        "package ex:shared@0.1.0;\ninterface types { record item { id: u32 } }\n",
    )
    .unwrap();
    fs::write(
        md_v01.join("contract.wit"),
        "package ex:app@0.1.0;\ninterface api { use ex:shared/types@0.1.0.{item}; record box { m: map<string, item> } }\n",
    )
    .unwrap();
    ensure_lock(&md_v01);
    let proto_files = proto::plan(&map_dep_only).unwrap();
    let v0_proto = proto_files.files().get("v0.proto").unwrap();
    assert!(
        v0_proto.contains("import \"ex.shared.v0.proto\";"),
        "proto must import dependency referenced inside map<K, V>:\n{v0_proto}"
    );
    let proto_out = stage.path().join("map-dep-proto");
    proto_files.write(&proto_out).unwrap();
    protox::compile([proto_out.join("v0.proto")], [&proto_out]).unwrap();

    stage.finish();
}

#[test]
fn subset_expansion_json_schema_draft7_and_2020_12_validate_instances() {
    let stage = ScratchDir::new("w11-040-schema");
    let wit_root = stage.path().join("wit");
    let v01 = wit_root.join("v0.1");
    fs::create_dir_all(&v01).unwrap();
    fs::write(
        v01.join("contract.wit"),
        "\
package ex:sch@0.1.0;

interface types {
    record doc {
        ch: char,
        fixed-bytes: list<u8, 4>,
        fixed-strs: list<string, 2>,
        m-bool: map<bool, u32>,
        m-u8: map<u8, string>,
        m-s16: map<s16, bool>,
        m-char: map<char, u32>,
        m-str: map<string, list<u32, 2>>,
    }
}
",
    )
    .unwrap();
    ensure_lock(&v01);

    let model = model::build(&load(&v01).unwrap()).unwrap();
    for (draft, def_ptr) in [
        (
            json_schema::JsonSchemaDraft::Draft7,
            "#/definitions/types~1doc",
        ),
        (
            json_schema::JsonSchemaDraft::Draft202012,
            "#/$defs/types~1doc",
        ),
    ] {
        let schema_str = json_schema::emit_version_with_options(
            &model,
            &json_schema::SchemaOptions::new().with_draft(draft),
        )
        .unwrap();
        let mut schema: serde_json::Value = serde_json::from_str(&schema_str).unwrap();
        schema.as_object_mut().unwrap().insert(
            "$ref".to_string(),
            serde_json::Value::String(def_ptr.to_string()),
        );
        let validator = jsonschema::validator_for(&schema).unwrap();

        let valid = serde_json::json!({
            "ch": "😀",
            "fixed-bytes": [1, 2, 3, 255],
            "fixed-strs": ["a", "b"],
            "m-bool": { "true": 1, "false": 2 },
            "m-u8": { "0": "zero", "255": "max" },
            "m-s16": { "-12": true, "42": false },
            "m-char": { "x": 10, "😀": 20 },
            "m-str": { "k": [7, 8] }
        });
        assert!(
            validator.is_valid(&valid),
            "{draft:?}: expected valid instance to pass"
        );

        let invalid_cases = [
            ("empty-char", serde_json::json!({ "ch": "" })),
            ("two-char", serde_json::json!({ "ch": "ab" })),
            (
                "short-fixed",
                serde_json::json!({ "fixed-bytes": [1, 2, 3] }),
            ),
            (
                "long-fixed",
                serde_json::json!({ "fixed-bytes": [1, 2, 3, 4, 5] }),
            ),
            (
                "bad-bool-key",
                serde_json::json!({ "m-bool": { "yes": 1 } }),
            ),
            ("neg-u8-key", serde_json::json!({ "m-u8": { "-1": "bad" } })),
            (
                "bad-s16-key",
                serde_json::json!({ "m-s16": { "abc": true } }),
            ),
            ("bad-char-key", serde_json::json!({ "m-char": { "ab": 1 } })),
            (
                "bad-map-val-len",
                serde_json::json!({ "m-str": { "k": [7] } }),
            ),
        ];
        for (label, patch) in invalid_cases {
            let mut bad = valid.clone();
            for (k, v) in patch.as_object().unwrap() {
                bad.as_object_mut().unwrap().insert(k.clone(), v.clone());
            }
            assert!(
                !validator.is_valid(&bad),
                "{draft:?} {label}: expected invalid instance to be rejected"
            );
        }
    }

    stage.finish();
}

#[test]
#[allow(clippy::too_many_lines)]
fn subset_expansion_compiles_and_roundtrips_in_editions_2021_and_2024() {
    let stage = ScratchDir::new("w11-040-rt");
    let wit_root = stage.path().join("wit");
    write_two_version_root(&wit_root);

    // Confirm generated Rust emits `mod __witforge_serde` when `--serde` is on, and omits it when off.
    let plain_no_serde = rust::plan(&wit_root).unwrap();
    let v01_no_serde = plain_no_serde.files().get("v0_1.rs").unwrap();
    assert!(!v01_no_serde.contains("__witforge_serde"));
    let plain_serde = rust::plan_with_serde(&wit_root, true).unwrap();
    let v01_serde = plain_serde.files().get("v0_1.rs").unwrap();
    assert!(v01_serde.contains("mod __witforge_serde"));
    assert!(
        !v01_serde.contains("pub mid_bytes: Digest32,\n        #[serde(with"),
        "32-byte array must not need a serde helper"
    );

    let roundtrip_body = r#"
    {
        use prost::Message as _;
        use std::collections::BTreeMap;
        use crate::expand::plain::{convert, v0_1, v0_2, witforge_deps::ex_shared};
        use crate::expand::glue;

        let leaf = ex_shared::types::Leaf {
            initial: 'λ',
            tag: [1, 2, 3, 4],
        };
        let mut map_big = BTreeMap::new();
        map_big.insert("k".to_string(), [0xAB; 48]);

        let mut m_bool = BTreeMap::new();
        m_bool.insert(false, 10);
        m_bool.insert(true, 20);
        let mut m_u8 = BTreeMap::new();
        m_u8.insert(255u8, "max".to_string());
        let mut m_u16 = BTreeMap::new();
        m_u16.insert(1024u16, v0_2::types::Status::Active);
        let mut m_u32 = BTreeMap::new();
        m_u32.insert(7u32, vec![9u8, 8, 7]);
        let mut m_u64 = BTreeMap::new();
        m_u64.insert(u64::MAX, Some("hi".to_string()));
        m_u64.insert(0, None);
        let mut m_s8 = BTreeMap::new();
        m_s8.insert(-128i8, "min".to_string());
        let mut m_s16 = BTreeMap::new();
        m_s16.insert(-32000i16, true);
        let mut m_s32 = BTreeMap::new();
        m_s32.insert(-5i32, vec![1, 2, 3]);
        let mut inner_map = BTreeMap::new();
        inner_map.insert("nested".to_string(), 99u32);
        let mut m_s64 = BTreeMap::new();
        m_s64.insert(i64::MIN, inner_map.clone());
        let mut m_char = BTreeMap::new();
        m_char.insert('😀', 42u32);
        let mut m_dep = BTreeMap::new();
        m_dep.insert("leaf".to_string(), leaf.clone());

        let v2_val = v0_2::types::Payload {
            ch: '😀',
            opt_ch: Some('z'),
            small_bytes: [10, 20, 30, 40],
            mid_bytes: [7u8; 32],
            big_bytes: [0x5A; 64],
            opt_big: Some([0x3C; 64]),
            vec_big: vec![[1u8; 40], [2u8; 40]],
            map_big: map_big.clone(),
            res_big: Ok([9u8; 33]),
            nested_big: [[3u8; 40], [4u8; 40]],
            points: [
                v0_2::types::Point { x: 1, label: 'a' },
                v0_2::types::Point { x: 2, label: 'b' },
            ],
            nums: [11, 22, 33, 44],
            opt_nums: Some([5, 6, 7, 8]),
            opt_pair: Some([
                v0_2::types::Point { x: 3, label: 'c' },
                v0_2::types::Point { x: 4, label: 'd' },
            ]),
            m_bool,
            m_u8,
            m_u16,
            m_u32,
            m_u64,
            m_s8,
            m_s16,
            m_s32,
            m_s64,
            m_char,
            m_dep: m_dep.clone(),
            opt_map: Some(BTreeMap::new()),
            named_opt_map: Some(inner_map.clone()),
            dep_hash: [0xEE; 64],
            dep_quad: [1, 2, 3, 4],
            dep_map: m_dep.clone(),
            dep_opt_map: Some(BTreeMap::new()),
            dep_opt_quad: Some([9, 8, 7, 6]),
            extra_attrs: inner_map.clone(),
        };

        // 1. Plain Rust `--serde` JSON round-trip (including >32 arrays and Err side of Result).
        let json = serde_json::to_string(&v2_val).unwrap();
        let back: v0_2::types::Payload = serde_json::from_str(&json).unwrap();
        assert_eq!(back, v2_val);

        let mut v2_err_res = v2_val.clone();
        v2_err_res.res_big = Err([512u16; 34]);
        v2_err_res.opt_big = None;
        let json2 = serde_json::to_string(&v2_err_res).unwrap();
        assert_eq!(serde_json::from_str::<v0_2::types::Payload>(&json2).unwrap(), v2_err_res);

        for choice in [
            v0_2::types::Choice::Letter('π'),
            v0_2::types::Choice::Small([1, 2, 3, 4]),
            v0_2::types::Choice::Large([0x42; 64]),
            v0_2::types::Choice::Quad([10, 20, 30, 40]),
            v0_2::types::Choice::Table(inner_map.clone()),
            v0_2::types::Choice::DepTable(m_dep.clone()),
        ] {
            let s = serde_json::to_string(&choice).unwrap();
            assert_eq!(serde_json::from_str::<v0_2::types::Choice>(&s).unwrap(), choice);
            let p = glue::v0::types::choice::to_proto(&choice);
            let bytes = p.encode_to_vec();
            let decoded_p = crate::expand::proto::ex::expand::v0::TypesChoice::decode(bytes.as_slice()).unwrap();
            assert_eq!(glue::v0::types::choice::from_proto(decoded_p).unwrap(), choice);
            let pj = serde_json::to_string(&p).unwrap();
            let decoded_pj: crate::expand::proto::ex::expand::v0::TypesChoice = serde_json::from_str(&pj).unwrap();
            assert_eq!(glue::v0::types::choice::from_proto(decoded_pj).unwrap(), choice);
        }

        // 2. Prost binary + pbjson proto3-JSON round-trip through glue.
        let p = glue::v0::types::payload::to_proto(&v2_val);
        let bytes = p.encode_to_vec();
        let decoded_p = crate::expand::proto::ex::expand::v0::TypesPayload::decode(bytes.as_slice()).unwrap();
        assert_eq!(glue::v0::types::payload::from_proto(decoded_p).unwrap(), v2_val);

        let pj = serde_json::to_string(&p).unwrap();
        let decoded_pj: crate::expand::proto::ex::expand::v0::TypesPayload = serde_json::from_str(&pj).unwrap();
        assert_eq!(glue::v0::types::payload::from_proto(decoded_pj).unwrap(), v2_val);

        // Distinguish None vs Some(empty map) on both anonymous and named option<map>.
        let mut v2_none = v2_val.clone();
        v2_none.opt_map = None;
        v2_none.named_opt_map = None;
        v2_none.dep_opt_map = None;
        v2_none.opt_nums = None;
        let p_none = glue::v0::types::payload::to_proto(&v2_none);
        assert_eq!(glue::v0::types::payload::from_proto(p_none).unwrap(), v2_none);

        // 3. Function request/response round-trip through glue.
        let req = glue::v0::types::functions::echo::to_request(&'✓', &[1, 2, 3, 4], &inner_map, &v2_val);
        let (r_ch, r_fixed, r_attrs, r_data) = glue::v0::types::functions::echo::from_request(req).unwrap();
        assert_eq!((r_ch, r_fixed, r_attrs, r_data), ('✓', [1, 2, 3, 4], inner_map.clone(), v2_val.clone()));
        let resp_val = Ok(v0_2::types::Choice::Large([0x77; 64]));
        let resp = glue::v0::types::functions::echo::to_response(&resp_val);
        assert_eq!(glue::v0::types::functions::echo::from_response(resp).unwrap(), resp_val);

        // 4. Glue decode error tests: InvalidChar, InvalidLength, IntegerOutOfRange, MissingField.
        let mut bad_char = glue::v0::types::payload::to_proto(&v2_val);
        bad_char.ch = Some(0xD800);
        assert_eq!(
            glue::v0::types::payload::from_proto(bad_char).unwrap_err(),
            glue::GlueError::InvalidChar { field: "types/payload.ch", value: 0xD800 }
        );

        let mut bad_map_char_key = glue::v0::types::payload::to_proto(&v2_val);
        bad_map_char_key.m_char.insert(0x110000, 1);
        assert_eq!(
            glue::v0::types::payload::from_proto(bad_map_char_key).unwrap_err(),
            glue::GlueError::InvalidChar { field: "types/char-counts", value: 0x110000 }
        );

        let mut bad_bytes_len = glue::v0::types::payload::to_proto(&v2_val);
        bad_bytes_len.small_bytes = Some(vec![1, 2, 3]);
        assert_eq!(
            glue::v0::types::payload::from_proto(bad_bytes_len).unwrap_err(),
            glue::GlueError::InvalidLength { field: "types/payload.small-bytes", expected: 4, actual: 3 }
        );

        let mut bad_repeated_len = glue::v0::types::payload::to_proto(&v2_val);
        bad_repeated_len.nums = vec![1, 2, 3, 4, 5];
        assert_eq!(
            glue::v0::types::payload::from_proto(bad_repeated_len).unwrap_err(),
            glue::GlueError::InvalidLength { field: "types/payload.nums", expected: 4, actual: 5 }
        );

        let mut bad_record_list_len = glue::v0::types::payload::to_proto(&v2_val);
        bad_record_list_len.points.pop();
        assert_eq!(
            glue::v0::types::payload::from_proto(bad_record_list_len).unwrap_err(),
            glue::GlueError::InvalidLength { field: "types/pair", expected: 2, actual: 1 }
        );

        let mut bad_opt_fixed = glue::v0::types::payload::to_proto(&v2_val);
        bad_opt_fixed.opt_nums.as_mut().unwrap().values.clear();
        assert_eq!(
            glue::v0::types::payload::from_proto(bad_opt_fixed).unwrap_err(),
            glue::GlueError::InvalidLength { field: "types/payload.opt-nums", expected: 4, actual: 0 }
        );

        let mut bad_map_u8_key = glue::v0::types::payload::to_proto(&v2_val);
        bad_map_u8_key.m_u8.insert(256, "overflow".to_string());
        assert_eq!(
            glue::v0::types::payload::from_proto(bad_map_u8_key).unwrap_err(),
            glue::GlueError::IntegerOutOfRange { field: "types/byte-labels", value: 256 }
        );

        let mut bad_map_s8_key = glue::v0::types::payload::to_proto(&v2_val);
        bad_map_s8_key.m_s8.insert(200, "overflow".to_string());
        assert_eq!(
            glue::v0::types::payload::from_proto(bad_map_s8_key).unwrap_err(),
            glue::GlueError::IntegerOutOfRange { field: "types/payload.m-s8", value: 200 }
        );

        let mut missing_char = glue::v0::types::payload::to_proto(&v2_val);
        missing_char.ch = None;
        assert_eq!(
            glue::v0::types::payload::from_proto(missing_char).unwrap_err(),
            glue::GlueError::MissingField { field: "types/payload.ch" }
        );

        let mut missing_fixed_bytes = glue::v0::types::payload::to_proto(&v2_val);
        missing_fixed_bytes.small_bytes = None;
        assert_eq!(
            glue::v0::types::payload::from_proto(missing_fixed_bytes).unwrap_err(),
            glue::GlueError::MissingField { field: "types/payload.small-bytes" }
        );

        // 5. Version conversion (`convert.rs`): v0.2 -> v0.1 -> v0.2 defaults `extra_attrs` to empty map.
        let v1_val: v0_1::types::Payload = convert::v0_2_to_v0_1::types::payload(&v2_val).unwrap();
        let v2_back: v0_2::types::Payload = convert::v0_1_to_v0_2::types::payload(&v1_val).unwrap();
        let mut expected_back = v2_val.clone();
        expected_back.extra_attrs = BTreeMap::new();
        assert_eq!(v2_back, expected_back);
    }
    "#;

    for edition in [RustEdition::E2021, RustEdition::E2024] {
        let mut batch = HarnessBatch::new("w11-040-ed", edition, QualityChecks::ALL);
        batch.add_wit_root("expand", &wit_root, true, true, roundtrip_body);
        batch.verify_and_run(&format!("subset_expansion ({})", edition.as_str()));
    }

    stage.finish();
}

#[test]
#[allow(clippy::too_many_lines)]
fn subset_expansion_no_std_with_serde_and_btree_map_glue_compiles_and_roundtrips() {
    let stage = ScratchDir::new("w11-040-nostd");
    let wit_root = stage.path().join("wit");
    write_two_version_root(&wit_root);

    let rust_files = rust::plan_with_options(
        &wit_root,
        None,
        &rust::RustOptions::new().with_no_std(true).with_serde(true),
    )
    .unwrap();
    let proto_files = proto::plan(&wit_root).unwrap();
    let glue_files = glue::plan_with_options(
        &wit_root,
        "crate::plain",
        "crate::proto",
        None,
        &glue::GlueOptions::new().with_no_std(true),
    )
    .unwrap();

    for (name, content) in rust_files.files().iter().chain(glue_files.files().iter()) {
        assert!(
            !content.contains("::std::"),
            "no_std output `{name}` unexpectedly contained `::std::`:\n{content}"
        );
    }

    let krate = stage.path().join("nostd-expand");
    fs::create_dir_all(krate.join("src")).unwrap();
    rust_files.write(&krate.join("src/plain")).unwrap();
    proto_files.write(&krate.join("proto")).unwrap();
    glue_files.write(&krate.join("src/glue")).unwrap();

    for edition in ["2021", "2024"] {
        for dir in [krate.join("src/plain"), krate.join("src/glue")] {
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().and_then(std::ffi::OsStr::to_str) != Some("rs") {
                    continue;
                }
                let fmt = Command::new("rustfmt")
                    .args(["--edition", edition, "--check"])
                    .arg(&path)
                    .output()
                    .unwrap();
                assert!(
                    fmt.status.success(),
                    "rustfmt --edition {edition} failed on {}:\n{}",
                    path.display(),
                    String::from_utf8_lossy(&fmt.stderr)
                );
            }
        }
    }

    fs::copy(
        workspace_root().join("Cargo.lock"),
        krate.join("Cargo.lock"),
    )
    .unwrap();
    fs::write(
        krate.join("Cargo.toml"),
        "[package]\n\
         name = \"witforge-w11-040-nostd\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\
         \n\
         [dependencies]\n\
         prost = { version = \"0.14\", default-features = false, features = [\"derive\"] }\n\
         serde = { version = \"1\", default-features = false, features = [\"derive\", \"alloc\"] }\n\
         \n\
         [dev-dependencies]\n\
         serde_json = \"1\"\n\
         \n\
         [build-dependencies]\n\
         prost-build = \"0.14\"\n\
         protox = \"0.10\"\n\
         \n\
         [workspace]\n",
    )
    .unwrap();
    fs::write(
        krate.join("build.rs"),
        "use std::fs;\n\
         use std::path::PathBuf;\n\
         fn main() -> Result<(), Box<dyn std::error::Error>> {\n\
             let proto_dir = PathBuf::from(\"proto\");\n\
             let mut protos: Vec<PathBuf> = fs::read_dir(&proto_dir)?\n\
                 .filter_map(|e| Some(e.ok()?.path()))\n\
                 .filter(|p| p.extension().and_then(|s| s.to_str()) == Some(\"proto\"))\n\
                 .collect();\n\
             protos.sort();\n\
             let fds = protox::compile(protos, [&proto_dir])?;\n\
             let mut cfg = prost_build::Config::new();\n\
             cfg.btree_map([\".\"]);\n\
             cfg.include_file(\"prost_include.rs\");\n\
             cfg.compile_fds(fds)?;\n\
             Ok(())\n\
         }\n",
    )
    .unwrap();
    fs::write(
        krate.join("src/lib.rs"),
        "#![no_std]\n\
         #![deny(missing_docs)]\n\
         \n\
         //! `#![no_std]` + `alloc` + `--serde` + `btree_map` verification crate.\n\
         \n\
         extern crate alloc;\n\
         \n\
         /// Generated plain Rust types.\n\
         #[path = \"plain/mod.rs\"]\n\
         pub mod plain;\n\
         \n\
         /// Prost-generated protobuf types with BTreeMap.\n\
         #[allow(clippy::all, missing_docs)]\n\
         pub mod proto {\n\
             include!(concat!(env!(\"OUT_DIR\"), \"/prost_include.rs\"));\n\
         }\n\
         \n\
         /// Generated glue.\n\
         #[path = \"glue/mod.rs\"]\n\
         pub mod glue;\n\
         \n\
         #[cfg(test)]\n\
         mod tests {\n\
             use alloc::collections::BTreeMap;\n\
             use alloc::string::ToString as _;\n\
             use alloc::vec;\n\
             use prost::Message as _;\n\
             use super::{glue, plain, proto};\n\
             \n\
             #[test]\n\
             fn nostd_glue_convert_and_serde_roundtrip() {\n\
                 let mut m = BTreeMap::new();\n\
                 m.insert(\"k\".to_string(), 7u32);\n\
                 let c = plain::v0_2::types::Choice::Large([0xAA; 64]);\n\
                 let p = glue::v0::types::choice::to_proto(&c);\n\
                 let bytes = p.encode_to_vec();\n\
                 let back = proto::ex::expand::v0::TypesChoice::decode(bytes.as_slice()).unwrap();\n\
                 assert_eq!(glue::v0::types::choice::from_proto(back).unwrap(), c);\n\
                 let t = plain::v0_2::types::Choice::Table(m);\n\
                 let tp = glue::v0::types::choice::to_proto(&t);\n\
                 assert_eq!(glue::v0::types::choice::from_proto(tp).unwrap(), t);\n\
                 let j = serde_json::to_string(&c).unwrap();\n\
                 assert_eq!(serde_json::from_str::<plain::v0_2::types::Choice>(&j).unwrap(), c);\n\
             }\n\
         }\n",
    )
    .unwrap();

    let target_dir = support::shared_target_dir();
    let clippy = Command::new("cargo")
        .current_dir(&krate)
        .env("CARGO_TARGET_DIR", &target_dir)
        .args(["clippy", "--lib", "--offline", "--", "-D", "warnings"])
        .output()
        .unwrap();
    assert!(
        clippy.status.success(),
        "no_std clippy failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&clippy.stdout),
        String::from_utf8_lossy(&clippy.stderr)
    );

    let test = Command::new("cargo")
        .current_dir(&krate)
        .env("CARGO_TARGET_DIR", &target_dir)
        .args(["test", "--lib", "--offline"])
        .output()
        .unwrap();
    assert!(
        test.status.success(),
        "no_std test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );

    stage.finish();
}
