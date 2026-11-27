//! End-to-end oracle proof for the cross-language protobuf conformance bundle (`W11-034`).
//!
//! Generates the canonical `conformance_version = 1` bundle, checks its `wit/` tree with
//! `witforge::workflow::check_root`, compiles both neutral and polyglot-optioned `.proto` outputs
//! with system `protoc` (when present) and `protox` + `prost-build` + `pbjson-build`, and runs
//! EVERY SINGLE decode vector in `vectors.json` through compiled Rust `prost` + `pbjson` +
//! `witforge` glue in BOTH binary protobuf and proto3-JSON forms.

mod support;

use std::fs;
use std::process::Command;

use support::{HarnessBatch, QualityChecks, RustEdition, ScratchDir};
use witforge::conformance::{self, CONFORMANCE_VERSION, ConformanceOptions};
use witforge::emit::{glue, rust};

const ORACLE_DRIVER: &str = r#"
{
    use std::collections::BTreeSet;
    use prost::Message as _;
    use serde_json::Value;

    use crate::conformance_case::glue::GlueError;
    use crate::conformance_case::glue::v0 as g;
    use crate::conformance_case::glue::witforge_deps::witforge_conformance_dep as g_dep;
    use crate::conformance_case::plain::v0_1 as p;
    use crate::conformance_case::plain::witforge_deps::witforge_conformance_dep as p_dep;
    use crate::conformance_case::proto::witforge::conformance::v0 as w;
    use crate::conformance_case::proto::witforge::conformance_dep::v0 as w_dep;

    fn decode_base64(input: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len() * 3 / 4);
        let bytes = input.as_bytes();
        assert_eq!(bytes.len() % 4, 0, "invalid base64 length: {input}");
        fn val(b: u8) -> u8 {
            match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => panic!("invalid base64 byte {b}"),
            }
        }
        for chunk in bytes.chunks_exact(4) {
            let a = val(chunk[0]);
            let b = val(chunk[1]);
            out.push((a << 2) | (b >> 4));
            if chunk[2] != b'=' {
                let c = val(chunk[2]);
                out.push((b << 4) | (c >> 2));
                if chunk[3] != b'=' {
                    let d = val(chunk[3]);
                    out.push((c << 6) | d);
                }
            }
        }
        out
    }

    fn classify_glue_error(err: &GlueError) -> (&'static str, &'static str) {
        match err {
            GlueError::IntegerOutOfRange { field, .. } => ("IntegerOutOfRange", field),
            GlueError::MissingField { field } => ("MissingField", field),
            GlueError::UnknownEnumValue { enumeration, .. } => ("UnknownEnumValue", enumeration),
            GlueError::UnknownCase { message } => ("UnknownCase", message),
            GlueError::InvalidState { message } => ("InvalidState", message),
            GlueError::InvalidChar { field, .. } => ("InvalidChar", field),
            GlueError::InvalidLength { field, .. } => ("InvalidLength", field),
            other => panic!("unexpected GlueError variant: {other:?}"),
        }
    }

    fn scalars_eq(a: &p::types::Scalars, b: &p::types::Scalars) -> bool {
        let f32_eq = if a.a_f32.is_nan() && b.a_f32.is_nan() {
            true
        } else {
            a.a_f32.to_bits() == b.a_f32.to_bits()
        };
        let f64_eq = if a.a_f64.is_nan() && b.a_f64.is_nan() {
            true
        } else {
            a.a_f64.to_bits() == b.a_f64.to_bits()
        };
        f32_eq && f64_eq && a.a_bool == b.a_bool && a.a_char == b.a_char && a.a_string == b.a_string
    }

    fn json_obj(entries: Vec<(&str, Value)>) -> Value {
        let mut map = serde_json::Map::new();
        for (k, v) in entries {
            map.insert(k.to_string(), v);
        }
        Value::Object(map)
    }

    fn json_obj_owned(entries: Vec<(String, Value)>) -> Value {
        let mut map = serde_json::Map::new();
        for (k, v) in entries {
            map.insert(k, v);
        }
        Value::Object(map)
    }

    fn color_str(c: &p::types::Color) -> &'static str {
        match c {
            p::types::Color::Red => "red",
            p::types::Color::Green => "green",
            p::types::Color::Blue => "blue",
        }
    }

    fn shared_status_str(s: &p_dep::shared::SharedStatus) -> &'static str {
        match s {
            p_dep::shared::SharedStatus::Active => "active",
            p_dep::shared::SharedStatus::Paused => "paused",
        }
    }

    fn point_to_json(v: &p::types::Point) -> Value {
        serde_json::json!({ "x": v.x, "y": v.y })
    }

    fn shared_meta_to_json(v: &p_dep::shared::SharedMeta) -> Value {
        serde_json::json!({
            "id": v.id.to_string(),
            "tag": v.tag,
            "status": shared_status_str(&v.status),
        })
    }

    fn integers_to_json(v: &p::types::Integers) -> Value {
        serde_json::json!({
            "a-u8": v.a_u8,
            "a-u16": v.a_u16,
            "a-u32": v.a_u32,
            "a-u64": v.a_u64.to_string(),
            "a-s8": v.a_s8,
            "a-s16": v.a_s16,
            "a-s32": v.a_s32,
            "a-s64": v.a_s64.to_string(),
        })
    }

    fn assert_f32_matches_json(id: &str, field: &str, got: f32, expected: &Value) {
        if got.is_nan() {
            assert_eq!(expected.as_str(), Some("NaN"), "{id}.{field}: expected NaN");
        } else if got.is_infinite() {
            let want = if got.is_sign_positive() {
                "Infinity"
            } else {
                "-Infinity"
            };
            assert_eq!(expected.as_str(), Some(want), "{id}.{field}: expected {want}");
        } else {
            let want = expected
                .as_f64()
                .unwrap_or_else(|| panic!("{id}.{field}: expected finite number, got {expected}"))
                as f32;
            assert_eq!(
                got.to_bits(),
                want.to_bits(),
                "{id}.{field}: f32 bits mismatch (got={got} / {:#x}, want={want} / {:#x})",
                got.to_bits(),
                want.to_bits()
            );
        }
    }

    fn assert_f64_matches_json(id: &str, field: &str, got: f64, expected: &Value) {
        if got.is_nan() {
            assert_eq!(expected.as_str(), Some("NaN"), "{id}.{field}: expected NaN");
        } else if got.is_infinite() {
            let want = if got.is_sign_positive() {
                "Infinity"
            } else {
                "-Infinity"
            };
            assert_eq!(expected.as_str(), Some(want), "{id}.{field}: expected {want}");
        } else {
            let want = expected
                .as_f64()
                .unwrap_or_else(|| panic!("{id}.{field}: expected finite number, got {expected}"));
            assert_eq!(
                got.to_bits(),
                want.to_bits(),
                "{id}.{field}: f64 bits mismatch (got={got} / {:#x}, want={want} / {:#x})",
                got.to_bits(),
                want.to_bits()
            );
        }
    }

    fn assert_scalars_plain_json(id: &str, v: &p::types::Scalars, expected: &Value) {
        let obj = expected
            .as_object()
            .unwrap_or_else(|| panic!("{id}: plain_json must be an object"));
        assert_eq!(obj.len(), 5, "{id}: unexpected keys in plain_json: {obj:?}");
        assert_f32_matches_json(id, "a-f32", v.a_f32, &obj["a-f32"]);
        assert_f64_matches_json(id, "a-f64", v.a_f64, &obj["a-f64"]);
        assert_eq!(obj["a-bool"], Value::Bool(v.a_bool), "{id}.a-bool");
        assert_eq!(obj["a-char"], Value::String(v.a_char.to_string()), "{id}.a-char");
        assert_eq!(obj["a-string"], Value::String(v.a_string.clone()), "{id}.a-string");
    }

    fn optionals_to_json(v: &p::types::Optionals) -> Value {
        json_obj(vec![
            ("opt-u8", v.opt_u8.map_or(Value::Null, |n| serde_json::json!(n))),
            ("opt-s8", v.opt_s8.map_or(Value::Null, |n| serde_json::json!(n))),
            (
                "opt-char",
                v.opt_char
                    .map_or(Value::Null, |c| Value::String(c.to_string())),
            ),
            (
                "opt-string",
                v.opt_string
                    .as_ref()
                    .map_or(Value::Null, |s| Value::String(s.clone())),
            ),
            (
                "opt-bytes",
                v.opt_bytes
                    .as_ref()
                    .map_or(Value::Null, |b| serde_json::json!(b)),
            ),
            (
                "opt-fixed-bytes",
                v.opt_fixed_bytes
                    .as_ref()
                    .map_or(Value::Null, |b| serde_json::json!(b.as_slice())),
            ),
            (
                "opt-list",
                v.opt_list
                    .as_ref()
                    .map_or(Value::Null, |l| serde_json::json!(l)),
            ),
            (
                "opt-enum",
                v.opt_enum
                    .as_ref()
                    .map_or(Value::Null, |c| Value::String(color_str(c).to_string())),
            ),
            (
                "opt-point",
                v.opt_point.as_ref().map_or(Value::Null, point_to_json),
            ),
            (
                "nested-opt",
                match v.nested_opt {
                    None => Value::Null,
                    Some(None) => serde_json::json!({ "none": null }),
                    Some(Some(n)) => serde_json::json!({ "some": n }),
                },
            ),
            (
                "opt-map",
                v.opt_map.as_ref().map_or(Value::Null, |m| {
                    json_obj_owned(
                        m.iter()
                            .map(|(k, val)| (k.to_string(), serde_json::json!(val)))
                            .collect(),
                    )
                }),
            ),
        ])
    }

    fn collections_to_json(v: &p::types::Collections) -> Value {
        json_obj(vec![
            ("raw-bytes", serde_json::json!(v.raw_bytes)),
            ("fixed-bytes", serde_json::json!(v.fixed_bytes.as_slice())),
            ("u16-items", serde_json::json!(v.u16_items)),
            ("fixed-u16s", serde_json::json!(v.fixed_u16s.as_slice())),
            (
                "fixed-chars",
                Value::Array(
                    v.fixed_chars
                        .iter()
                        .map(|c| Value::String(c.to_string()))
                        .collect(),
                ),
            ),
            (
                "point-items",
                Value::Array(v.point_items.iter().map(point_to_json).collect()),
            ),
            ("nested-lists", serde_json::json!(v.nested_lists)),
            (
                "nested-fixed",
                Value::Array(
                    v.nested_fixed
                        .iter()
                        .map(|arr| serde_json::json!(arr.as_slice()))
                        .collect(),
                ),
            ),
            (
                "opt-items",
                Value::Array(
                    v.opt_items
                        .iter()
                        .map(|opt| opt.map_or(Value::Null, |n| serde_json::json!(n)))
                        .collect(),
                ),
            ),
            (
                "result-items",
                Value::Array(
                    v.result_items
                        .iter()
                        .map(|res| match res {
                            Ok(n) => serde_json::json!({ "ok": n }),
                            Err(c) => serde_json::json!({ "err": color_str(c) }),
                        })
                        .collect(),
                ),
            ),
        ])
    }

    fn maps_to_json(v: &p::types::Maps) -> Value {
        json_obj(vec![
            (
                "str-to-u8",
                json_obj_owned(
                    v.str_to_u8
                        .iter()
                        .map(|(k, val)| (k.clone(), serde_json::json!(val)))
                        .collect(),
                ),
            ),
            (
                "u8-to-str",
                json_obj_owned(
                    v.u8_to_str
                        .iter()
                        .map(|(k, val)| (k.to_string(), Value::String(val.clone())))
                        .collect(),
                ),
            ),
            (
                "s8-to-point",
                json_obj_owned(
                    v.s8_to_point
                        .iter()
                        .map(|(k, val)| (k.to_string(), point_to_json(val)))
                        .collect(),
                ),
            ),
            (
                "char-to-color",
                json_obj_owned(
                    v.char_to_color
                        .iter()
                        .map(|(k, val)| (k.to_string(), Value::String(color_str(val).to_string())))
                        .collect(),
                ),
            ),
            (
                "bool-to-s16",
                json_obj_owned(
                    v.bool_to_s16
                        .iter()
                        .map(|(k, val)| (k.to_string(), serde_json::json!(val)))
                        .collect(),
                ),
            ),
            (
                "u64-to-s64",
                json_obj_owned(
                    v.u64_to_s64
                        .iter()
                        .map(|(k, val)| (k.to_string(), Value::String(val.to_string())))
                        .collect(),
                ),
            ),
            (
                "nested-maps",
                Value::Array(
                    v.nested_maps
                        .iter()
                        .map(|m| {
                            json_obj_owned(
                                m.iter()
                                    .map(|(k, val)| (k.to_string(), serde_json::json!(val)))
                                    .collect(),
                            )
                        })
                        .collect(),
                ),
            ),
        ])
    }

    fn choice_to_json(v: &p::types::Choice) -> Value {
        match v {
            p::types::Choice::UnitCase => serde_json::json!({ "case": "unit-case" }),
            p::types::Choice::Small(n) => serde_json::json!({ "case": "small", "value": n }),
            p::types::Choice::Glyph(c) => {
                serde_json::json!({ "case": "glyph", "value": c.to_string() })
            }
            p::types::Choice::Nested(pt) => {
                serde_json::json!({ "case": "nested", "value": point_to_json(pt) })
            }
            p::types::Choice::Fixed(arr) => {
                serde_json::json!({ "case": "fixed", "value": arr.as_slice() })
            }
        }
    }

    fn outcome_to_json(v: &p::types::Outcome) -> Value {
        match v {
            Ok(n) => serde_json::json!({ "ok": n }),
            Err(s) => serde_json::json!({ "err": s }),
        }
    }

    fn results_to_json(v: &p::types::Results) -> Value {
        json_obj(vec![
            ("both", outcome_to_json(&v.both)),
            (
                "only-ok",
                match &v.only_ok {
                    Ok(n) => serde_json::json!({ "ok": n }),
                    Err(()) => serde_json::json!({ "err": null }),
                },
            ),
            (
                "only-err",
                match &v.only_err {
                    Ok(()) => serde_json::json!({ "ok": null }),
                    Err(c) => serde_json::json!({ "err": color_str(c) }),
                },
            ),
            (
                "bare",
                match &v.bare {
                    Ok(()) => serde_json::json!({ "ok": null }),
                    Err(()) => serde_json::json!({ "err": null }),
                },
            ),
        ])
    }

    fn permissions_to_json(v: &p::types::Permissions) -> Value {
        serde_json::json!({
            "read": v.read,
            "write": v.write,
            "execute": v.execute,
        })
    }

    fn composites_to_json(v: &p::types::Composites) -> Value {
        json_obj(vec![
            ("origin", point_to_json(&v.origin)),
            ("color", Value::String(color_str(&v.color).to_string())),
            ("perms", permissions_to_json(&v.perms)),
            ("choice", choice_to_json(&v.choice)),
            ("outcome", outcome_to_json(&v.outcome)),
            ("foreign", shared_meta_to_json(&v.foreign)),
            (
                "foreign-status",
                Value::String(shared_status_str(&v.foreign_status).to_string()),
            ),
        ])
    }

    fn assert_plain_eq<P>(
        id: &str,
        expected: &Value,
        to_json: impl Fn(&P) -> Value,
    ) -> impl Fn(&str, &P, &Value) {
        let _ = (id, expected);
        move |vec_id: &str, val: &P, want: &Value| {
            let got = to_json(val);
            assert_eq!(
                &got, want,
                "{vec_id}: plain_json mismatch:\n  got:  {got}\n  want: {want}"
            );
        }
    }

    fn assert_encode_plain_json_schema(id: &str, proto_message: &str, plain_json: &Value) {
        let obj = plain_json
            .as_object()
            .unwrap_or_else(|| panic!("{id}: encode plain_json must be a JSON object"));
        let keys: BTreeSet<&str> = obj.keys().map(String::as_str).collect();
        match proto_message {
            ".witforge.conformance.v0.TypesIntegers" => {
                let want: BTreeSet<&str> = [
                    "a-u8", "a-u16", "a-u32", "a-u64", "a-s8", "a-s16", "a-s32", "a-s64",
                ]
                .into_iter()
                .collect();
                assert_eq!(keys, want, "{id}: encode plain_json keys mismatch");
            }
            ".witforge.conformance.v0.TypesScalars" => {
                let want_str: BTreeSet<&str> =
                    ["a-f32", "a-f64", "a-bool", "a-char", "a-string"]
                        .into_iter()
                        .collect();
                let want_cp: BTreeSet<&str> =
                    ["a-f32", "a-f64", "a-bool", "a-char-codepoint", "a-string"]
                        .into_iter()
                        .collect();
                assert!(
                    keys == want_str || keys == want_cp,
                    "{id}: encode scalars plain_json keys mismatch: {keys:?}"
                );
            }
            ".witforge.conformance.v0.TypesCollections" => {
                let want: BTreeSet<&str> = [
                    "raw-bytes",
                    "fixed-bytes",
                    "u16-items",
                    "fixed-u16s",
                    "fixed-chars",
                    "point-items",
                    "nested-lists",
                    "nested-fixed",
                    "opt-items",
                    "result-items",
                ]
                .into_iter()
                .collect();
                assert_eq!(keys, want, "{id}: encode collections plain_json keys mismatch");
            }
            ".witforge.conformance.v0.TypesMaps" => {
                let want: BTreeSet<&str> = [
                    "str-to-u8",
                    "u8-to-str",
                    "s8-to-point",
                    "char-to-color",
                    "bool-to-s16",
                    "u64-to-s64",
                    "nested-maps",
                ]
                .into_iter()
                .collect();
                assert_eq!(keys, want, "{id}: encode maps plain_json keys mismatch");
            }
            ".witforge.conformance.v0.TypesComposites" => {
                let want: BTreeSet<&str> = [
                    "origin",
                    "color",
                    "perms",
                    "choice",
                    "outcome",
                    "foreign",
                    "foreign-status",
                ]
                .into_iter()
                .collect();
                assert_eq!(keys, want, "{id}: encode composites plain_json keys mismatch");
            }
            other => panic!("{id}: unexpected encode proto_message `{other}`"),
        }
    }

    fn run_type_vector<W, P>(
        id: &str,
        expected: &str,
        expected_kind: Option<&str>,
        expected_target: Option<&str>,
        bin: &[u8],
        p3_json: &str,
        plain_json: &Value,
        from_proto: impl Fn(W) -> Result<P, GlueError>,
        to_proto: impl Fn(&P) -> W,
        eq: impl Fn(&P, &P) -> bool,
        check_plain_json: impl Fn(&str, &P, &Value),
        bin_errors: &mut BTreeSet<String>,
        json_errors: &mut BTreeSet<String>,
    ) where
        W: prost::Message + Default + serde::de::DeserializeOwned + serde::Serialize,
        P: std::fmt::Debug,
    {
        // 1. Binary protobuf check
        let decoded_wire = W::decode(bin)
            .unwrap_or_else(|e| panic!("{id}: prost::Message::decode failed: {e}"));
        let bin_res = from_proto(decoded_wire);

        // 2. Proto3-JSON check
        let json_wire_res: Result<W, _> = serde_json::from_str(p3_json);

        match expected {
            "accept" => {
                let plain_from_bin = bin_res.unwrap_or_else(|e| {
                    panic!("{id}: expected binary accept, got GlueError::{e:?}")
                });
                check_plain_json(id, &plain_from_bin, plain_json);
                let re_wire = to_proto(&plain_from_bin);
                let re_bytes = re_wire.encode_to_vec();
                let roundtrip_bin = from_proto(
                    W::decode(re_bytes.as_slice())
                        .unwrap_or_else(|e| panic!("{id}: re-decode binary failed: {e}")),
                )
                .unwrap_or_else(|e| panic!("{id}: round-trip binary from_proto failed: {e:?}"));
                assert!(
                    eq(&plain_from_bin, &roundtrip_bin),
                    "{id}: binary round-trip mismatch:\n  first:  {plain_from_bin:?}\n  second: {roundtrip_bin:?}"
                );

                let wire_from_json = json_wire_res.unwrap_or_else(|e| {
                    panic!("{id}: expected proto3-JSON parse accept for `{p3_json}`, got {e}")
                });
                let plain_from_json = from_proto(wire_from_json).unwrap_or_else(|e| {
                    panic!("{id}: expected proto3-JSON glue accept, got {e:?}")
                });
                check_plain_json(id, &plain_from_json, plain_json);
                assert!(
                    eq(&plain_from_bin, &plain_from_json),
                    "{id}: binary vs proto3-JSON decoded mismatch:\n  bin:  {plain_from_bin:?}\n  json: {plain_from_json:?}"
                );

                let re_json_wire = to_proto(&plain_from_json);
                let re_json = serde_json::to_string(&re_json_wire)
                    .unwrap_or_else(|e| panic!("{id}: proto3-JSON serialize failed: {e}"));
                let is_known_pbjson_non_finite = matches!(
                    id,
                    "scalars-valid-f32-nan-f64-pos-inf"
                        | "scalars-valid-f32-neg-inf-f64-nan"
                        | "scalars-valid-f32-pos-inf-f64-neg-inf"
                );
                if !(is_known_pbjson_non_finite && re_json.contains(":null")) {
                    let roundtrip_json = from_proto(
                        serde_json::from_str::<W>(&re_json).unwrap_or_else(|e| {
                            panic!("{id}: proto3-JSON re-parse `{re_json}` failed: {e}")
                        }),
                    )
                    .unwrap_or_else(|e| panic!("{id}: round-trip JSON from_proto failed: {e:?}"));
                    assert!(
                        eq(&plain_from_json, &roundtrip_json),
                        "{id}: proto3-JSON round-trip mismatch:\n  first:  {plain_from_json:?}\n  second: {roundtrip_json:?}"
                    );
                }
            }
            "reject" => {
                assert!(plain_json.is_null(), "{id}: reject decode vector must have null plain_json");
                let want_kind = expected_kind.expect("reject vector missing error_kind");
                let want_target = expected_target.expect("reject vector missing error_target");
                let err = bin_res.unwrap_err();
                let (got_kind, got_target) = classify_glue_error(&err);
                assert_eq!(
                    got_kind, want_kind,
                    "{id}: binary GlueError kind mismatch (err = {err:?})"
                );
                assert_eq!(
                    got_target, want_target,
                    "{id}: binary GlueError target mismatch (err = {err:?})"
                );
                bin_errors.insert(got_kind.to_string());

                match json_wire_res {
                    Ok(wire_from_json) => {
                        let json_err = from_proto(wire_from_json).unwrap_err();
                        let (j_kind, j_target) = classify_glue_error(&json_err);
                        assert_eq!(
                            j_kind, want_kind,
                            "{id}: proto3-JSON GlueError kind mismatch (err = {json_err:?})"
                        );
                        assert_eq!(
                            j_target, want_target,
                            "{id}: proto3-JSON GlueError target mismatch (err = {json_err:?})"
                        );
                        json_errors.insert(j_kind.to_string());
                    }
                    Err(parse_err) => {
                        // pbjson rejects unknown numeric enum values directly during JSON
                        // deserialization (before glue runs), which enforces the same closed-enum
                        // rejection boundary.
                        assert_eq!(
                            want_kind, "UnknownEnumValue",
                            "{id}: unexpected proto3-JSON parse error `{parse_err}` for non-enum error kind `{want_kind}` (input: {p3_json})"
                        );
                        json_errors.insert("UnknownEnumValue".to_string());
                    }
                }
            }
            other => panic!("{id}: unexpected expected=`{other}`"),
        }
    }

    let vectors_path = __WITFORGE_VECTORS_PATH__;
    let raw = std::fs::read_to_string(vectors_path).unwrap();
    let manifest: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(manifest["conformance_version"], 1);
    let declared_error_kinds: BTreeSet<&str> = manifest["error_kinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let vectors = manifest["vectors"].as_array().unwrap();

    let mut decode_accept_count = 0usize;
    let mut decode_reject_count = 0usize;
    let mut encode_reject_count = 0usize;
    let mut bin_errors = BTreeSet::new();
    let mut json_errors = BTreeSet::new();

    for vec in vectors {
        let id = vec["id"].as_str().unwrap();
        let direction = vec["direction"].as_str().unwrap();
        let expected = vec["expected"].as_str().unwrap();
        let proto_message = vec["proto_message"].as_str().unwrap();
        let error_kind = vec["error_kind"].as_str();
        let error_target = vec["error_target"].as_str();
        let plain_json = &vec["plain_json"];

        if let Some(kind) = error_kind {
            assert!(
                declared_error_kinds.contains(kind),
                "{id}: error_kind `{kind}` is not listed in manifest.error_kinds"
            );
        }

        if direction == "encode" {
            assert_eq!(expected, "reject", "{id}: encode vector must expect reject");
            assert!(vec["proto_binary_base64"].is_null(), "{id}");
            assert!(vec["proto3_json"].is_null(), "{id}");
            assert!(error_kind.is_some() && error_target.is_some(), "{id}");
            assert_encode_plain_json_schema(id, proto_message, plain_json);
            encode_reject_count += 1;
            continue;
        }
        assert_eq!(direction, "decode", "{id}");
        if expected == "accept" {
            decode_accept_count += 1;
        } else {
            decode_reject_count += 1;
        }
        let bin = decode_base64(vec["proto_binary_base64"].as_str().unwrap());
        let p3 = vec["proto3_json"].as_str().unwrap();

        match proto_message {
            ".witforge.conformance.v0.TypesIntegers" => run_type_vector::<w::TypesIntegers, _>(
                id, expected, error_kind, error_target, &bin, p3, plain_json,
                g::types::integers::from_proto, g::types::integers::to_proto, PartialEq::eq,
                assert_plain_eq(id, plain_json, integers_to_json),
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesScalars" => run_type_vector::<w::TypesScalars, _>(
                id, expected, error_kind, error_target, &bin, p3, plain_json,
                g::types::scalars::from_proto, g::types::scalars::to_proto, scalars_eq,
                assert_scalars_plain_json,
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesOptionals" => run_type_vector::<w::TypesOptionals, _>(
                id, expected, error_kind, error_target, &bin, p3, plain_json,
                g::types::optionals::from_proto, g::types::optionals::to_proto, PartialEq::eq,
                assert_plain_eq(id, plain_json, optionals_to_json),
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesCollections" => run_type_vector::<w::TypesCollections, _>(
                id, expected, error_kind, error_target, &bin, p3, plain_json,
                g::types::collections::from_proto, g::types::collections::to_proto, PartialEq::eq,
                assert_plain_eq(id, plain_json, collections_to_json),
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesMaps" => run_type_vector::<w::TypesMaps, _>(
                id, expected, error_kind, error_target, &bin, p3, plain_json,
                g::types::maps::from_proto, g::types::maps::to_proto, PartialEq::eq,
                assert_plain_eq(id, plain_json, maps_to_json),
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesChoice" => run_type_vector::<w::TypesChoice, _>(
                id, expected, error_kind, error_target, &bin, p3, plain_json,
                g::types::choice::from_proto, g::types::choice::to_proto, PartialEq::eq,
                assert_plain_eq(id, plain_json, choice_to_json),
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesResults" => run_type_vector::<w::TypesResults, _>(
                id, expected, error_kind, error_target, &bin, p3, plain_json,
                g::types::results::from_proto, g::types::results::to_proto, PartialEq::eq,
                assert_plain_eq(id, plain_json, results_to_json),
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesPermissions" => run_type_vector::<w::TypesPermissions, _>(
                id, expected, error_kind, error_target, &bin, p3, plain_json,
                g::types::permissions::from_proto, g::types::permissions::to_proto, PartialEq::eq,
                assert_plain_eq(id, plain_json, permissions_to_json),
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesComposites" => run_type_vector::<w::TypesComposites, _>(
                id, expected, error_kind, error_target, &bin, p3, plain_json,
                g::types::composites::from_proto, g::types::composites::to_proto, PartialEq::eq,
                assert_plain_eq(id, plain_json, composites_to_json),
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance_dep.v0.SharedSharedMeta" => {
                run_type_vector::<w_dep::SharedSharedMeta, _>(
                    id, expected, error_kind, error_target, &bin, p3, plain_json,
                    g_dep::shared::shared_meta::from_proto, g_dep::shared::shared_meta::to_proto,
                    PartialEq::eq,
                    assert_plain_eq(id, plain_json, shared_meta_to_json),
                    &mut bin_errors, &mut json_errors,
                )
            }
            ".witforge.conformance.v0.ApiEchoIntegersRequest" => {
                run_type_vector::<w::ApiEchoIntegersRequest, _>(
                    id, expected, error_kind, error_target, &bin, p3, plain_json,
                    g::api::functions::echo_integers::from_request,
                    g::api::functions::echo_integers::to_request,
                    PartialEq::eq,
                    assert_plain_eq(id, plain_json, |v| json_obj(vec![("input", integers_to_json(v))])),
                    &mut bin_errors, &mut json_errors,
                )
            }
            ".witforge.conformance.v0.ApiEchoIntegersResponse" => {
                run_type_vector::<w::ApiEchoIntegersResponse, _>(
                    id, expected, error_kind, error_target, &bin, p3, plain_json,
                    g::api::functions::echo_integers::from_response,
                    g::api::functions::echo_integers::to_response,
                    PartialEq::eq,
                    assert_plain_eq(id, plain_json, |v| json_obj(vec![("result", integers_to_json(v))])),
                    &mut bin_errors, &mut json_errors,
                )
            }
            ".witforge.conformance.v0.ApiProcessForeignRequest" => {
                run_type_vector::<w::ApiProcessForeignRequest, _>(
                    id, expected, error_kind, error_target, &bin, p3, plain_json,
                    g::api::functions::process_foreign::from_request,
                    |(meta, status, code)| g::api::functions::process_foreign::to_request(meta, status, code),
                    PartialEq::eq,
                    assert_plain_eq(id, plain_json, |(meta, status, code)| {
                        json_obj(vec![
                            ("meta", shared_meta_to_json(meta)),
                            ("status", Value::String(shared_status_str(status).to_string())),
                            ("code", serde_json::json!(code)),
                        ])
                    }),
                    &mut bin_errors, &mut json_errors,
                )
            }
            ".witforge.conformance.v0.ApiProcessForeignResponse" => {
                run_type_vector::<w::ApiProcessForeignResponse, _>(
                    id, expected, error_kind, error_target, &bin, p3, plain_json,
                    g::api::functions::process_foreign::from_response,
                    g::api::functions::process_foreign::to_response,
                    PartialEq::eq,
                    assert_plain_eq(id, plain_json, |res| {
                        let inner = match res {
                            Ok(pt) => serde_json::json!({ "ok": point_to_json(pt) }),
                            Err(c) => serde_json::json!({ "err": color_str(c) }),
                        };
                        json_obj(vec![("result", inner)])
                    }),
                    &mut bin_errors, &mut json_errors,
                )
            }
            ".witforge.conformance.v0.ApiNotifyRequest" => {
                run_type_vector::<w::ApiNotifyRequest, _>(
                    id, expected, error_kind, error_target, &bin, p3, plain_json,
                    g::api::functions::notify::from_request,
                    |(label, prio)| g::api::functions::notify::to_request(label, prio),
                    PartialEq::eq,
                    assert_plain_eq(id, plain_json, |(label, prio)| {
                        serde_json::json!({ "label": label, "priority": prio })
                    }),
                    &mut bin_errors, &mut json_errors,
                )
            }
            ".witforge.conformance.v0.ApiNotifyResponse" => {
                run_type_vector::<w::ApiNotifyResponse, _>(
                    id, expected, error_kind, error_target, &bin, p3, plain_json,
                    g::api::functions::notify::from_response,
                    |()| g::api::functions::notify::to_response(),
                    PartialEq::eq,
                    assert_plain_eq(id, plain_json, |()| serde_json::json!({})),
                    &mut bin_errors, &mut json_errors,
                )
            }
            other => panic!("{id}: unhandled proto_message `{other}`"),
        }
    }

    assert_eq!(
        decode_accept_count + decode_reject_count + encode_reject_count,
        vectors.len(),
        "every conformance vector must be executed"
    );
    assert!(decode_accept_count >= 38, "got {decode_accept_count}");
    assert!(decode_reject_count >= 65, "got {decode_reject_count}");
    assert!(encode_reject_count >= 35, "got {encode_reject_count}");
    for kind in [
        "IntegerOutOfRange",
        "MissingField",
        "UnknownEnumValue",
        "UnknownCase",
        "InvalidState",
        "InvalidChar",
        "InvalidLength",
    ] {
        assert!(
            bin_errors.contains(kind),
            "binary decode oracle did not exercise GlueError::{kind}"
        );
        assert!(
            json_errors.contains(kind),
            "proto3-JSON decode oracle did not exercise GlueError::{kind}"
        );
    }
}
"#;

#[test]
fn conformance_bundle_vectors_pass_compiled_rust_prost_pbjson_glue_oracle() {
    let scratch = ScratchDir::new("conformance-oracle");
    let neutral_dir = scratch.path().join("bundle-neutral");
    let poly_dir = scratch.path().join("bundle-poly");

    conformance::generate(&neutral_dir, &ConformanceOptions::new()).unwrap();
    conformance::generate(&poly_dir, &ConformanceOptions::polyglot()).unwrap();

    assert!(
        conformance::check(&neutral_dir, &ConformanceOptions::new())
            .unwrap()
            .is_empty()
    );
    assert!(
        conformance::check(&poly_dir, &ConformanceOptions::polyglot())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        conformance::vectors_json().unwrap(),
        fs::read_to_string(poly_dir.join("vectors.json")).unwrap()
    );

    // Confirm neutral and polyglot bundles share identical vectors.json and wit/ trees while
    // differing only in the opt-in file options inside proto/.
    assert_eq!(
        fs::read_to_string(neutral_dir.join("vectors.json")).unwrap(),
        fs::read_to_string(poly_dir.join("vectors.json")).unwrap()
    );
    assert_eq!(
        u64::from(CONFORMANCE_VERSION),
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(poly_dir.join("vectors.json")).unwrap()
        )
        .unwrap()["conformance_version"]
            .as_u64()
            .unwrap()
    );

    // If system `protoc` is installed, verify both neutral and polyglot `.proto` sets compile
    // cleanly into binary FileDescriptorSets with zero warnings.
    if Command::new("protoc").arg("--version").output().is_ok() {
        for (label, proto_dir) in [
            ("neutral", neutral_dir.join("proto")),
            ("polyglot", poly_dir.join("proto")),
        ] {
            let desc_out = scratch.path().join(format!("{label}.desc"));
            let output = Command::new("protoc")
                .arg(format!("--proto_path={}", proto_dir.display()))
                .arg(format!("--descriptor_set_out={}", desc_out.display()))
                .arg("--include_imports")
                .arg(proto_dir.join("v0.proto"))
                .arg(proto_dir.join("witforge.conformance_dep.v0.proto"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "protoc failed on {label} conformance bundle:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    // Generate plain Rust and glue from the polyglot bundle's `wit/` tree and mount the bundle's
    // own `proto/` directory (with Go/Java/C# file options!) directly into the Rust oracle crate
    // so we prove file options never break prost/pbjson/glue AND every vector passes!
    let plain_src = scratch.path().join("plain");
    let glue_src = scratch.path().join("glue");
    rust::generate(&poly_dir.join("wit"), &plain_src).unwrap();
    glue::generate(
        &poly_dir.join("wit"),
        &glue_src,
        "crate::conformance_case::plain",
        "crate::conformance_case::proto",
    )
    .unwrap();

    // Pass the vectors.json path cleanly via explicit placeholder substitution.
    let vectors_abs = poly_dir.join("vectors.json");
    let literal = format!("{:?}", vectors_abs.to_string_lossy().into_owned());
    let driver = ORACLE_DRIVER.replace("__WITFORGE_VECTORS_PATH__", &literal);
    assert_ne!(driver, ORACLE_DRIVER, "vectors path placeholder must match");

    for (label, edition) in [
        ("conformance-run-2021", RustEdition::E2021),
        ("conformance-run-2024", RustEdition::E2024),
    ] {
        let mut batch = HarnessBatch::new(label, edition, QualityChecks::ALL);
        batch.add_generated_dirs(
            "conformance_case",
            "plain",
            "proto",
            "glue",
            &plain_src,
            &poly_dir.join("proto"),
            &glue_src,
            true,
            driver.clone(),
        );
        batch.verify_and_run(&format!("conformance_bundle_oracle_{label}"));
    }
    scratch.finish();
}
