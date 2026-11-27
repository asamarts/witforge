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

    fn run_type_vector<W, P>(
        id: &str,
        expected: &str,
        expected_kind: Option<&str>,
        expected_target: Option<&str>,
        bin: &[u8],
        p3_json: &str,
        from_proto: impl Fn(W) -> Result<P, GlueError>,
        to_proto: impl Fn(&P) -> W,
        eq: impl Fn(&P, &P) -> bool,
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

    let vectors_path = std::env::var("WITFORGE_CONFORMANCE_VECTORS")
        .expect("WITFORGE_CONFORMANCE_VECTORS must be set");
    let raw = std::fs::read_to_string(&vectors_path).unwrap();
    let manifest: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(manifest["conformance_version"], 1);
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

        if direction == "encode" {
            assert_eq!(expected, "reject", "{id}: encode vector must expect reject");
            assert!(vec["proto_binary_base64"].is_null(), "{id}");
            assert!(vec["proto3_json"].is_null(), "{id}");
            assert!(!vec["plain_json"].is_null(), "{id}");
            assert!(error_kind.is_some() && error_target.is_some(), "{id}");
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
                id, expected, error_kind, error_target, &bin, p3,
                g::types::integers::from_proto, g::types::integers::to_proto, PartialEq::eq,
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesScalars" => run_type_vector::<w::TypesScalars, _>(
                id, expected, error_kind, error_target, &bin, p3,
                g::types::scalars::from_proto, g::types::scalars::to_proto, scalars_eq,
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesOptionals" => run_type_vector::<w::TypesOptionals, _>(
                id, expected, error_kind, error_target, &bin, p3,
                g::types::optionals::from_proto, g::types::optionals::to_proto, PartialEq::eq,
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesCollections" => run_type_vector::<w::TypesCollections, _>(
                id, expected, error_kind, error_target, &bin, p3,
                g::types::collections::from_proto, g::types::collections::to_proto, PartialEq::eq,
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesMaps" => run_type_vector::<w::TypesMaps, _>(
                id, expected, error_kind, error_target, &bin, p3,
                g::types::maps::from_proto, g::types::maps::to_proto, PartialEq::eq,
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesChoice" => run_type_vector::<w::TypesChoice, _>(
                id, expected, error_kind, error_target, &bin, p3,
                g::types::choice::from_proto, g::types::choice::to_proto, PartialEq::eq,
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesResults" => run_type_vector::<w::TypesResults, _>(
                id, expected, error_kind, error_target, &bin, p3,
                g::types::results::from_proto, g::types::results::to_proto, PartialEq::eq,
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesPermissions" => run_type_vector::<w::TypesPermissions, _>(
                id, expected, error_kind, error_target, &bin, p3,
                g::types::permissions::from_proto, g::types::permissions::to_proto, PartialEq::eq,
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance.v0.TypesComposites" => run_type_vector::<w::TypesComposites, _>(
                id, expected, error_kind, error_target, &bin, p3,
                g::types::composites::from_proto, g::types::composites::to_proto, PartialEq::eq,
                &mut bin_errors, &mut json_errors,
            ),
            ".witforge.conformance_dep.v0.SharedSharedMeta" => {
                run_type_vector::<w_dep::SharedSharedMeta, _>(
                    id, expected, error_kind, error_target, &bin, p3,
                    g_dep::shared::shared_meta::from_proto, g_dep::shared::shared_meta::to_proto,
                    PartialEq::eq, &mut bin_errors, &mut json_errors,
                )
            }
            ".witforge.conformance.v0.ApiEchoIntegersRequest" => {
                run_type_vector::<w::ApiEchoIntegersRequest, _>(
                    id, expected, error_kind, error_target, &bin, p3,
                    g::api::functions::echo_integers::from_request,
                    g::api::functions::echo_integers::to_request,
                    PartialEq::eq, &mut bin_errors, &mut json_errors,
                )
            }
            ".witforge.conformance.v0.ApiEchoIntegersResponse" => {
                run_type_vector::<w::ApiEchoIntegersResponse, _>(
                    id, expected, error_kind, error_target, &bin, p3,
                    g::api::functions::echo_integers::from_response,
                    g::api::functions::echo_integers::to_response,
                    PartialEq::eq, &mut bin_errors, &mut json_errors,
                )
            }
            ".witforge.conformance.v0.ApiProcessForeignRequest" => {
                run_type_vector::<w::ApiProcessForeignRequest, _>(
                    id, expected, error_kind, error_target, &bin, p3,
                    g::api::functions::process_foreign::from_request,
                    |(meta, status, code)| g::api::functions::process_foreign::to_request(meta, status, code),
                    PartialEq::eq, &mut bin_errors, &mut json_errors,
                )
            }
            ".witforge.conformance.v0.ApiProcessForeignResponse" => {
                run_type_vector::<w::ApiProcessForeignResponse, _>(
                    id, expected, error_kind, error_target, &bin, p3,
                    g::api::functions::process_foreign::from_response,
                    g::api::functions::process_foreign::to_response,
                    PartialEq::eq, &mut bin_errors, &mut json_errors,
                )
            }
            ".witforge.conformance.v0.ApiNotifyRequest" => {
                run_type_vector::<w::ApiNotifyRequest, _>(
                    id, expected, error_kind, error_target, &bin, p3,
                    g::api::functions::notify::from_request,
                    |(label, prio)| g::api::functions::notify::to_request(label, prio),
                    PartialEq::eq, &mut bin_errors, &mut json_errors,
                )
            }
            ".witforge.conformance.v0.ApiNotifyResponse" => {
                run_type_vector::<w::ApiNotifyResponse, _>(
                    id, expected, error_kind, error_target, &bin, p3,
                    g::api::functions::notify::from_response,
                    |()| g::api::functions::notify::to_response(),
                    PartialEq::eq, &mut bin_errors, &mut json_errors,
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
    assert!(decode_accept_count >= 35, "got {decode_accept_count}");
    assert!(decode_reject_count >= 50, "got {decode_reject_count}");
    assert!(encode_reject_count >= 20, "got {encode_reject_count}");
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

    // Pass the vectors.json path cleanly by embedding it into the driver body.
    let vectors_abs = poly_dir.join("vectors.json");
    let driver = format!(
        "let vectors_path_override = {path:?};\n{body}",
        path = vectors_abs.to_string_lossy().into_owned(),
        body = ORACLE_DRIVER.replace(
            "std::env::var(\"WITFORGE_CONFORMANCE_VECTORS\")\n        .expect(\"WITFORGE_CONFORMANCE_VECTORS must be set\")",
            "vectors_path_override"
        )
    );

    let mut batch = HarnessBatch::new("conformance-run", RustEdition::E2024, QualityChecks::ALL);
    batch.add_generated_dirs(
        "conformance_case",
        "plain",
        "proto",
        "glue",
        &plain_src,
        &poly_dir.join("proto"),
        &glue_src,
        true,
        driver,
    );
    batch.verify_and_run("conformance_bundle_oracle");
    scratch.finish();
}
