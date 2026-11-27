//! `W10-067` contract validation invariant tests:
//!
//! 1. Every single-version generator refusal is reported by `check` (`WF1003`/`WF1030`).
//! 2. Unreachable unsupported dependency items—and every item transitively referencing them,
//!    including across two dependency packages—are pruned cleanly without exit 2 or dangling
//!    references.
//! 3. Package segments accepted by `protoc`/`protox` (including proto3 keywords such as
//!    `service`, `message`, `option`, `enum`, `package`, `import`) are accepted by `check` and
//!    every generator and compile under `protox` + `prost-build`.
//! 4. Property test: `subset::check_diagnostics` passes ⇒ `rust`, `proto`, `schema`, and `glue`
//!    all succeed (never exit 1, never exit 2).

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;
use witforge::emit::{glue, json_schema, proto, rust, typescript};
use witforge::load;
use witforge::lock::{LockMode, WireLock};
use witforge::model;
use witforge::subset;

#[path = "support/propgen.rs"]
mod propgen;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn scratch_dir(label: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "witforge-check-gen-{label}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn collect_refs<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(reference)) = map.get("$ref") {
                out.push(reference.as_str());
            }
            for child in map.values() {
                collect_refs(child, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_refs(item, out);
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_lines)]
#[test]
fn unreachable_unsupported_dependency_chains_and_cross_dep_refs_are_pruned_cleanly() {
    let base = scratch_dir("prune-deps");
    let wit_root = base.join("wit");
    let v01 = wit_root.join("v0.1");
    fs::create_dir_all(v01.join("deps/dep-a")).unwrap();
    fs::create_dir_all(v01.join("deps/dep-b")).unwrap();

    // `dep-a` has:
    // - supported `good` (referenced by root and dep-b)
    // - unreachable unsupported `hidden` (resource)
    // - unreachable `wrap` referencing `hidden`
    // - unreachable `outer` referencing `wrap` (transitive within one dep!)
    // - unreachable `alias-outer` and function `take-outer` referencing `outer`
    // - unreachable `record bad-char { c: char }` -> `record bad-holder { b: bad-char }`
    fs::write(
        v01.join("deps/dep-a/types.wit"),
        "package ex:dep-a@0.1.0;\n\n\
         interface types {\n\
           record good { id: u32 }\n\
           resource hidden;\n\
           record wrap { h: hidden }\n\
           record outer { w: wrap }\n\
           type alias-outer = outer;\n\
           take-outer: func(o: alias-outer);\n\
           record bad-char { c: tuple<u8, u8> }\n\
           record bad-holder { b: bad-char }\n\
         }\n",
    )
    .unwrap();

    // `dep-b` references BOTH `dep-a/types/good` (supported) AND `dep-a/types/outer` and
    // `dep-a/types/bad-holder` (unreachable transitive unsupported items across two deps!).
    // Previously `rust` and `schema` exited 0 with a dangling Rust alias and a dangling `$ref`.
    fs::write(
        v01.join("deps/dep-b/types.wit"),
        "package ex:dep-b@0.1.0;\n\n\
         interface types {\n\
           use ex:dep-a/types@0.1.0.{good, outer, bad-holder};\n\
           record b-good { item: good }\n\
           record b-bad { o: outer, b: bad-holder }\n\
           take-bad: func(x: b-bad);\n\
         }\n",
    )
    .unwrap();

    fs::write(
        v01.join("contract.wit"),
        "package ex:root@0.1.0;\n\n\
         interface api {\n\
           use ex:dep-b/types@0.1.0.{b-good};\n\
           get: func() -> b-good;\n\
         }\n",
    )
    .unwrap();

    let contract = load(&v01).unwrap();
    let diags = subset::check_diagnostics(&contract);
    assert!(diags.is_empty(), "expected clean check, got:\n{diags:#?}");

    let built = model::build(&contract).unwrap();
    let mut lock = WireLock::default();
    lock.reconcile_with(&built.members().unwrap(), LockMode::First)
        .unwrap();
    fs::write(v01.join("wire.lock"), lock.render()).unwrap();

    // All five generators succeed with zero dangling references.
    let rust_files = rust::plan(&wit_root).unwrap();
    let proto_files = proto::plan(&wit_root).unwrap();
    let schema_files = json_schema::plan(&wit_root).unwrap();
    let glue_files = glue::plan(&wit_root, "crate::plain", "crate::proto").unwrap();
    let ts_files = typescript::plan(&wit_root).unwrap();
    for forbidden in [
        "Hidden",
        "Wrap",
        "Outer",
        "AliasOuter",
        "BadChar",
        "BadHolder",
        "BBad",
        "takeOuter",
        "takeBad",
    ] {
        for (path, content) in ts_files.files() {
            assert!(
                !content.contains(forbidden),
                "pruned dependency symbol `{forbidden}` leaked into {path}:\n{content}"
            );
        }
    }

    let deps_rs = rust_files.files().get("witforge_deps.rs").unwrap();
    for forbidden in [
        "Hidden",
        "Wrap",
        "Outer",
        "AliasOuter",
        "BadChar",
        "BadHolder",
        "BBad",
        "take_outer",
        "take_bad",
    ] {
        assert!(
            !deps_rs.contains(forbidden),
            "pruned dependency symbol `{forbidden}` leaked into witforge_deps.rs:\n{deps_rs}"
        );
    }

    // Rust compiles cleanly (no dangling alias).
    let rust_out = base.join("rust");
    rust_files.write(&rust_out).unwrap();
    let rustc = Command::new("rustc")
        .args([
            "--edition=2024",
            "--crate-type=lib",
            "--emit=metadata",
            "-o",
        ])
        .arg(base.join("plain.rmeta"))
        .arg(rust_out.join("mod.rs"))
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "plain Rust failed to compile:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );

    // Proto compiles cleanly with protox + prost-build.
    let proto_out = base.join("proto");
    proto_files.write(&proto_out).unwrap();
    let proto_paths: Vec<PathBuf> = fs::read_dir(&proto_out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "proto"))
        .collect();
    let fds = protox::compile(proto_paths, [&proto_out]).unwrap();
    let prost_out = base.join("prost");
    fs::create_dir_all(&prost_out).unwrap();
    prost_build::Config::new()
        .out_dir(&prost_out)
        .compile_fds(fds)
        .unwrap();

    // JSON Schema has no dangling `$ref` across any emitted schema file.
    let mut parsed_schemas = std::collections::BTreeMap::new();
    for (file_name, content) in schema_files.files() {
        let parsed: serde_json::Value = serde_json::from_str(content).unwrap();
        parsed_schemas.insert(file_name.as_str(), parsed);
    }
    let second_dep_schema = schema_files.files().get("ex.dep_b.v0.schema.json").unwrap();
    for forbidden in ["outer", "bad-holder", "b-bad", "hidden", "wrap"] {
        assert!(
            !second_dep_schema.contains(forbidden),
            "pruned symbol `{forbidden}` leaked into dep_b schema:\n{second_dep_schema}"
        );
    }
    let first_defs = parsed_schemas["ex.dep_a.v0.schema.json"]["definitions"]
        .as_object()
        .unwrap();
    assert!(first_defs.contains_key("types/good"));
    assert!(!first_defs.contains_key("types/outer"));
    assert!(!first_defs.contains_key("types/bad-holder"));
    assert!(
        second_dep_schema.contains("ex.dep_a.v0.schema.json#/definitions/types~1good"),
        "{second_dep_schema}"
    );
    // Walk every `$ref` in every emitted schema and verify the target file & JSON pointer exist.
    for (file_name, doc) in &parsed_schemas {
        let mut refs = Vec::new();
        collect_refs(doc, &mut refs);
        for reference in refs {
            let (target_file, pointer) = if let Some(rest) = reference.strip_prefix('#') {
                (*file_name, rest)
            } else {
                reference
                    .split_once('#')
                    .unwrap_or_else(|| panic!("unexpected $ref `{reference}` in `{file_name}`"))
            };
            let target_doc = parsed_schemas
                .get(target_file)
                .unwrap_or_else(|| panic!("dangling $ref `{reference}` in `{file_name}`"));
            assert!(
                target_doc.pointer(pointer).is_some(),
                "dangling $ref pointer `{reference}` in `{file_name}`"
            );
        }
    }
    jsonschema::validator_for(&parsed_schemas["ex.dep_a.v0.schema.json"]).unwrap();
    assert!(glue_files.files().contains_key("witforge_deps.rs"));

    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn proto_keyword_package_segments_pass_check_and_compile_in_protox_and_prost() {
    // Every proto3 keyword accepted as a package segment by protoc / protox.
    let pairs = [
        ("example", "service"),
        ("message", "option"),
        ("enum", "package"),
        ("import", "syntax"),
        ("repeated", "optional"),
        ("oneof", "map"),
        ("reserved", "rpc"),
        ("returns", "stream"),
        ("weak", "public"),
        ("extensions", "to"),
        ("max", "required"),
        ("bool", "bytes"),
        ("string", "double"),
        ("float", "int32"),
        ("int64", "uint32"),
        ("uint64", "sint32"),
        ("sint64", "fixed32"),
        ("fixed64", "sfixed32"),
        ("sfixed64", "true"),
        ("false", "service"),
    ];

    let base = scratch_dir("proto-kw-pkgs");
    for (idx, (ns, pkg)) in pairs.iter().enumerate() {
        let root = base.join(format!("case_{idx}"));
        let v01 = root.join("v0.1");
        fs::create_dir_all(&v01).unwrap();
        let wit = format!(
            "package {}:{}@0.1.0;\n\ninterface types {{\n  record item {{ id: u32 }}\n  ping: func(x: item) -> item;\n}}\n",
            propgen::wit_escape(ns),
            propgen::wit_escape(pkg),
        );
        fs::write(v01.join("contract.wit"), wit).unwrap();

        let contract = load(&v01).unwrap();
        let diags = subset::check_diagnostics(&contract);
        assert!(diags.is_empty(), "{ns}:{pkg} refused by check: {diags:#?}");
        let built = model::build(&contract).unwrap();
        let mut lock = WireLock::default();
        lock.reconcile_with(&built.members().unwrap(), LockMode::First)
            .unwrap();
        fs::write(v01.join("wire.lock"), lock.render()).unwrap();

        rust::plan(&root).unwrap();
        let proto_files = proto::plan(&root).unwrap();
        json_schema::plan(&root).unwrap();
        glue::plan(&root, "crate::plain", "crate::proto").unwrap();
        typescript::plan(&root).unwrap();

        let proto_dir = base.join(format!("proto_{idx}"));
        proto_files.write(&proto_dir).unwrap();
        let descriptors = protox::compile([proto_dir.join("v0.proto")], [&proto_dir])
            .unwrap_or_else(|e| panic!("protox rejected package `{ns}.{pkg}.v0`: {e}"));
        let prost_dir = base.join(format!("prost_{idx}"));
        fs::create_dir_all(&prost_dir).unwrap();
        prost_build::Config::new()
            .out_dir(&prost_dir)
            .compile_fds(descriptors)
            .unwrap_or_else(|e| panic!("prost-build rejected package `{ns}.{pkg}.v0`: {e}"));
    }
    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn every_known_generator_refusal_is_reported_by_check_with_a_code() {
    let base = scratch_dir("known-refusals");
    let cases: &[(&str, &str, &str, &str)] = &[
        (
            "request-clash",
            "package ex:t@0.1.0;\ninterface types { record get-request { id: u32 } get: func() -> get-request; }\n",
            "WF1003",
            "TypesGetRequest",
        ),
        (
            "response-clash",
            "package ex:t@0.1.0;\ninterface types { record get-response { id: u32 } get: func(); }\n",
            "WF1003",
            "TypesGetResponse",
        ),
        (
            "reserved-functions",
            "package ex:t@0.1.0;\ninterface types { record functions { id: u32 } }\n",
            "WF1003",
            "reserved glue module `functions`",
        ),
        (
            "type-vs-trait",
            "package ex:t@0.1.0;\ninterface api { record api { id: u32 } ping: func(); }\n",
            "WF1003",
            "Api",
        ),
        (
            "variant-witforge-value",
            "package ex:t@0.1.0;\ninterface types { variant v { witforge-value, ok } }\n",
            "WF1003",
            "witforge_value",
        ),
        (
            "enum-unspecified",
            "package ex:t@0.1.0;\ninterface types { enum e { unspecified, ok } }\n",
            "WF1003",
            "UNSPECIFIED",
        ),
        (
            "fn-unspecified",
            "package ex:t@0.1.0;\ninterface api { unspecified: func(); }\n",
            "WF1003",
            "UNSPECIFIED",
        ),
        (
            "prost-accessor-collision",
            "package ex:t@0.1.0;\ninterface types { enum kind { a } record r { mode: kind, set-mode: bool } }\n",
            "WF1003",
            "set_mode",
        ),
        (
            "wrapper-collision",
            "package ex:t@0.1.0;\ninterface ab { record cd { a: u8 } }\ninterface list-of-ab { record cd { b: string } }\ninterface api { use ab.{cd as c1}; use list-of-ab.{cd as c2}; record r { a: list<list<list<c1>>>, b: list<list<c2>> } }\n",
            "WF1003",
            "WitforgeListOfListOfAbCd",
        ),
        (
            "wrapper-vs-user",
            "package ex:t@0.1.0;\ninterface witforge { record list-of-string { v: string } }\ninterface api { type m = list<list<string>>; }\n",
            "WF1003",
            "WitforgeListOfString",
        ),
    ];

    for (label, wit, expected_code, expected_substr) in cases {
        let dir = base.join(label);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("contract.wit"), wit).unwrap();
        let diags = subset::check_diagnostics(&load(&dir).unwrap());
        assert!(
            diags
                .iter()
                .any(|d| d.code == *expected_code && d.message.contains(expected_substr)),
            "{label}: expected {expected_code} containing `{expected_substr}`, got:\n{diags:#?}"
        );
    }

    // Two dependency packages mapping to one generated Rust/glue module (`my_ns_dep`).
    let dep_mod = base.join("dep-mod-clash");
    fs::create_dir_all(dep_mod.join("deps/a")).unwrap();
    fs::create_dir_all(dep_mod.join("deps/b")).unwrap();
    fs::write(
        dep_mod.join("deps/a/types.wit"),
        "package my-ns:dep@0.1.0;\ninterface types { record item { id: u32 } }\n",
    )
    .unwrap();
    fs::write(
        dep_mod.join("deps/b/types.wit"),
        "package my:ns-dep@0.1.0;\ninterface types { record item { id: u32 } }\n",
    )
    .unwrap();
    fs::write(
        dep_mod.join("contract.wit"),
        "package ex:app@0.1.0;\ninterface api { record ok { id: u32 } }\n",
    )
    .unwrap();
    let diags = subset::check_diagnostics(&load(&dep_mod).unwrap());
    assert!(
        diags
            .iter()
            .any(|d| d.code == "WF1003" && d.message.contains("my_ns_dep")),
        "{diags:#?}"
    );

    // Type-expansion limits (`W10-058`) are also reported directly by `subset::check_diagnostics`
    // with `WF1030`.
    let nested = format!("{}u8{}", "list<".repeat(40), ">".repeat(40));
    let limit_dir = base.join("nesting-limit");
    fs::create_dir_all(&limit_dir).unwrap();
    fs::write(
        limit_dir.join("contract.wit"),
        format!("package ex:t@0.1.0;\ninterface types {{ record r {{ value: {nested} }} }}\n"),
    )
    .unwrap();
    let diags = subset::check_diagnostics(&load(&limit_dir).unwrap());
    assert!(
        diags
            .iter()
            .any(|d| d.code == "WF1030" && d.message.contains("nesting")),
        "{diags:#?}"
    );

    // Upper-case WIT interface/type names (`interface API`) are handled consistently:
    // either `check` refuses them with a code, or all four generators succeed.
    let upper_dir = base.join("upper-api");
    let upper_v01 = upper_dir.join("v0.1");
    fs::create_dir_all(&upper_v01).unwrap();
    fs::write(
        upper_v01.join("contract.wit"),
        "package ex:t@0.1.0;\ninterface API { record item { id: u32 } ping: func(x: item) -> item; }\n",
    )
    .unwrap();
    let upper_loaded = load(&upper_v01).unwrap();
    let upper_diags = subset::check_diagnostics(&upper_loaded);
    if upper_diags.is_empty() {
        let built = model::build(&upper_loaded).unwrap();
        let mut lock = WireLock::default();
        lock.reconcile_with(&built.members().unwrap(), LockMode::First)
            .unwrap();
        fs::write(upper_v01.join("wire.lock"), lock.render()).unwrap();
        rust::plan(&upper_dir).unwrap();
        proto::plan(&upper_dir).unwrap();
        json_schema::plan(&upper_dir).unwrap();
        glue::plan(&upper_dir, "crate::plain", "crate::proto").unwrap();
        typescript::plan(&upper_dir).unwrap();
    } else {
        assert!(
            upper_diags.iter().all(|d| !d.code.is_empty()),
            "{upper_diags:#?}"
        );
    }

    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn deterministic_property_sample_covers_passing_contracts_and_implies_generate() {
    let base = scratch_dir("prop-sample");
    let mut loaded_count = 0usize;
    let mut passing_count = 0usize;

    for (idx, contract) in propgen::sample_contracts([42; 32], 64)
        .into_iter()
        .enumerate()
    {
        let wit_root = base.join(format!("c{idx}"));
        let v01 = wit_root.join("v0.1");
        contract.write_to(&v01);

        let Ok(loaded) = load(&v01) else {
            continue;
        };
        loaded_count += 1;

        let diags = subset::check_diagnostics(&loaded);
        if diags.is_empty() {
            passing_count += 1;
            let built = model::build(&loaded).unwrap_or_else(|e| {
                panic!(
                    "sample {idx}: check passed but model::build failed: {e:#}\n{}",
                    contract.root_wit
                )
            });
            let mut lock = WireLock::default();
            lock.reconcile_with(&built.members().unwrap(), LockMode::First)
                .unwrap();
            fs::write(v01.join("wire.lock"), lock.render()).unwrap();

            rust::plan(&wit_root).unwrap_or_else(|e| {
                panic!(
                    "sample {idx}: check passed but rust failed: {e:#}\n{}",
                    contract.root_wit
                )
            });
            proto::plan(&wit_root).unwrap_or_else(|e| {
                panic!(
                    "sample {idx}: check passed but proto failed: {e:#}\n{}",
                    contract.root_wit
                )
            });
            json_schema::plan(&wit_root).unwrap_or_else(|e| {
                panic!(
                    "sample {idx}: check passed but schema failed: {e:#}\n{}",
                    contract.root_wit
                )
            });
            glue::plan(&wit_root, "crate::plain", "crate::proto").unwrap_or_else(|e| {
                panic!(
                    "sample {idx}: check passed but glue failed: {e:#}\n{}",
                    contract.root_wit
                )
            });
            typescript::plan(&wit_root).unwrap_or_else(|e| {
                panic!(
                    "sample {idx}: check passed but ts failed: {e:#}\n{}",
                    contract.root_wit
                )
            });
        }
    }

    assert!(
        loaded_count >= 50,
        "expected >= 50 syntactically valid generated contracts, got {loaded_count}"
    );
    assert!(
        passing_count >= 20,
        "expected >= 20 passing contracts in 64 samples (non-vacuous property), got {passing_count}"
    );
    fs::remove_dir_all(&base).unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        max_shrink_iters: 64,
        failure_persistence: None,
        .. ProptestConfig::default()
    })]

    #[test]
    fn check_implies_all_four_generators_succeed_property(contract in propgen::arb_contract()) {
        let base = scratch_dir("prop");
        let wit_root = base.join("wit");
        let v01 = wit_root.join("v0.1");
        contract.write_to(&v01);

        let Ok(loaded) = load(&v01) else {
            let _ = fs::remove_dir_all(&base);
            return Ok(());
        };

        let diags = subset::check_diagnostics(&loaded);
        if diags.is_empty() {
            let built = model::build(&loaded).map_err(|err| {
                TestCaseError::fail(format!(
                    "check passed but model::build failed: {err:#}\nroot:\n{}\ndeps:\n{:#?}",
                    contract.root_wit, contract.deps
                ))
            })?;
            let members = built.members().map_err(|err| {
                TestCaseError::fail(format!(
                    "check passed but model.members failed: {err:#}\nroot:\n{}",
                    contract.root_wit
                ))
            })?;
            let mut lock = WireLock::default();
            lock.reconcile_with(&members, LockMode::First).map_err(|err| {
                TestCaseError::fail(format!(
                    "check passed but lock.reconcile failed: {err:#}\nroot:\n{}",
                    contract.root_wit
                ))
            })?;
            fs::write(v01.join("wire.lock"), lock.render()).unwrap();

            for (gen_name, res) in [
                ("rust", rust::plan(&wit_root)),
                ("proto", proto::plan(&wit_root)),
                ("schema", json_schema::plan(&wit_root)),
                ("glue", glue::plan(&wit_root, "crate::plain", "crate::proto")),
                ("ts", typescript::plan(&wit_root)),
            ] {
                if let Err(err) = res {
                    let kind = if witforge::is_contract_error(&err) {
                        "exit 1 (ContractError)"
                    } else {
                        "exit 2 (internal error)"
                    };
                    let _ = fs::remove_dir_all(&base);
                    return Err(TestCaseError::fail(format!(
                        "check passed but `{gen_name}` failed with {kind}: {err:#}\n\
                         root:\n{}\ndeps:\n{:#?}",
                        contract.root_wit, contract.deps
                    )));
                }
            }
        }
        let _ = fs::remove_dir_all(&base);
    }
}
