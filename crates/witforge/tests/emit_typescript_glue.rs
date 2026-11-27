//! Offline Tier 1 integration tests for the TypeScript checked protobuf glue emitter (`W13-003`).
//!
//! Runs with zero external toolchains (`protoc`, `node`, `tsc`) required so it passes on Linux,
//! macOS, Windows, and MSRV `1.88` inside `cargo test --workspace --locked`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use witforge::emit::output::{GeneratedFileStatus, HeaderStyle};
use witforge::emit::typescript_glue::{self, TypeScriptGlueOptions};
use witforge::lock::{LockMode, WireLock};
use witforge::{is_contract_error, load, model, subset};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn scratch_dir(label: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "witforge-ts-glue-{label}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

fn lock_version_first(dir: &Path) {
    let loaded = load(dir).unwrap();
    let diags = subset::check_diagnostics(&loaded);
    assert!(diags.is_empty(), "subset check failed: {diags:#?}");
    let built = model::build(&loaded).unwrap();
    let mut lock = WireLock::default();
    lock.reconcile_with(&built.members().unwrap(), LockMode::First)
        .unwrap();
    fs::write(dir.join("wire.lock"), lock.render()).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn conformance_contract_emits_deterministic_lf_only_glue_covering_all_decode_and_encode_rules() {
    let base = scratch_dir("conformance-offline");
    let bundle_dir = base.join("bundle");
    witforge::conformance::generate(
        &bundle_dir,
        &witforge::conformance::ConformanceOptions::new(),
    )
    .unwrap();
    let wit_root = bundle_dir.join("wit");

    let first = typescript_glue::plan(&wit_root, "../plain", "../proto").unwrap();
    let second = typescript_glue::plan_with_options(
        &wit_root,
        "../plain",
        "../proto",
        None,
        &TypeScriptGlueOptions::new(),
    )
    .unwrap();
    assert_eq!(first, second, "ts-glue output must be deterministic");

    let expected_header =
        HeaderStyle::LineComment.format_header("ts-glue", "witforge", "conformance");
    for (rel, content) in first.files() {
        assert!(!content.contains('\r'), "{rel} contains CR");
        assert!(
            content.starts_with(&expected_header),
            "{rel} missing ts-glue header:\n{content}"
        );
    }

    // Core nested layout: root barrel, error & helpers modules, v0 major dir, and shared dep tree.
    for required in [
        "index.ts",
        "error.ts",
        "helpers.ts",
        "v0/index.ts",
        "v0/types.ts",
        "v0/api.ts",
        "witforge_deps/index.ts",
        "witforge_deps/witforge_conformance_dep/index.ts",
        "witforge_deps/witforge_conformance_dep/shared.ts",
    ] {
        assert!(
            first.files().contains_key(required),
            "missing expected ts-glue file `{required}`; got: {:?}",
            first.files().keys().collect::<Vec<_>>()
        );
    }

    let error_ts = &first.files()["error.ts"];
    for kind in [
        "\"IntegerOutOfRange\"",
        "\"MissingField\"",
        "\"UnknownEnumValue\"",
        "\"UnknownCase\"",
        "\"InvalidState\"",
        "\"InvalidChar\"",
        "\"InvalidLength\"",
        "export class WitforgeGlueError extends Error",
    ] {
        assert!(error_ts.contains(kind), "error.ts missing `{kind}`");
    }

    let helpers_ts = &first.files()["helpers.ts"];
    for helper in [
        "export function requirePresent",
        "export function checkU8",
        "export function checkU16",
        "export function checkU32",
        "export function checkU64",
        "export function checkS8",
        "export function checkS16",
        "export function checkS32",
        "export function checkS64",
        "export function decodeChar",
        "export function encodeChar",
        "export function checkLength",
        "export function parseMapIntKey",
        "export function parseMapCharKey",
        "export function parseMapBigIntKey",
        "export function parseMapBoolKey",
        "export function setMapEntry",
    ] {
        assert!(helpers_ts.contains(helper), "helpers.ts missing `{helper}`");
    }

    let types_ts = &first.files()["v0/types.ts"];
    for needle in [
        "import { create as _wf_create } from \"@bufbuild/protobuf\";",
        "import type * as _wf_plain from \"../../plain/v0_1/types.js\";",
        "import * as _wf_proto from \"../../proto/v0_pb.js\";",
        "schema: _wf_proto.TypesIntegersSchema",
        "_wf_h.checkU8(",
        "_wf_h.checkU16(",
        "_wf_h.checkU32(",
        "_wf_h.checkU64(",
        "_wf_h.checkS8(",
        "_wf_h.checkS16(",
        "_wf_h.checkS32(",
        "_wf_h.checkS64(",
        "_wf_h.encodeChar(",
        "_wf_h.decodeChar(",
        "_wf_h.checkLength(",
        "_wf_h.requirePresent(",
        "\"UnknownEnumValue\"",
        "\"UnknownCase\"",
        "\"InvalidState\"",
        "\"types/integers.a-u8\"",
        "\"types/scalars.a-char\"",
        "\"types/collections.fixed-bytes\"",
        "\"types/maps.char-to-color\"",
        "\"types/choice\"",
        "\"types/color\"",
    ] {
        assert!(types_ts.contains(needle), "v0/types.ts missing `{needle}`");
    }

    let api_ts = &first.files()["v0/api.ts"];
    for needle in [
        "export const functions = {",
        "METHOD: _wf_proto.ApiMethod.ECHO_INTEGERS",
        "requestSchema: _wf_proto.ApiEchoIntegersRequestSchema",
        "responseSchema: _wf_proto.ApiEchoIntegersResponseSchema",
        "METHOD: _wf_proto.ApiMethod.PROCESS_FOREIGN",
        "METHOD: _wf_proto.ApiMethod.NOTIFY",
        "\"api#echo-integers.input\"",
        "\"api#process-foreign.code\"",
        "\"api#notify.priority\"",
    ] {
        assert!(api_ts.contains(needle), "v0/api.ts missing `{needle}`");
    }

    let dep_shared_ts = &first.files()["witforge_deps/witforge_conformance_dep/shared.ts"];
    assert!(
        dep_shared_ts.contains(
            "import * as _wf_proto from \"../../../proto/witforge.conformance_dep.v0_pb.js\";"
        ),
        "dep shared.ts proto import mismatch:\n{dep_shared_ts}"
    );
    assert!(
        dep_shared_ts.contains(
            "import type * as _wf_plain from \"../../../plain/witforge_deps/witforge_conformance_dep/shared.js\";"
        ),
        "dep shared.ts plain import mismatch:\n{dep_shared_ts}"
    );

    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn multi_major_disambiguated_deps_canonical_owner_reexports_and_npm_import_roots() {
    let base = scratch_dir("multi-major-deps");
    let wit = base.join("wit");

    // v0.1 & v1.0 share an identical dep (`acme:shared@0.1.0`); v2.0 upgrades `acme:shared@0.1.0`
    // in place (same dep major, different definition -> per-major scoped + disambiguated proto!).
    for (ver, pkg_ver, dep_extra) in [
        ("v0.1", "0.1.0", ""),
        ("v1.0", "1.0.0", ""),
        ("v2.0", "2.0.0", "\n    extra: bool,"),
    ] {
        let dir = wit.join(ver);
        fs::create_dir_all(dir.join("deps/shared")).unwrap();
        fs::write(
            dir.join("deps/shared/dep.wit"),
            format!(
                "package acme:shared@0.1.0;\n\
                 interface core {{\n\
                   record tag {{\n\
                     id: u8,{dep_extra}\n\
                   }}\n\
                 }}\n"
            ),
        )
        .unwrap();
        fs::write(
            dir.join("contract.wit"),
            format!(
                "package acme:app@{pkg_ver};\n\
                 interface api {{\n\
                   use acme:shared/core@0.1.0.{{tag}};\n\
                   record env {{ tag: tag }}\n\
                 }}\n"
            ),
        )
        .unwrap();
        lock_version_first(&dir);
    }

    // Use bare npm package specifiers (`@acme/plain`, `@acme/proto`) to verify package-root resolution.
    let files = typescript_glue::plan(&wit, "@acme/plain", "@acme/proto").unwrap();

    // v0 owns the first definition of acme:shared; v1 has an identical copy so v1 re-exports from v0!
    assert!(
        files
            .files()
            .contains_key("witforge_deps/v0/acme_shared/core.ts")
    );
    assert!(
        files
            .files()
            .contains_key("witforge_deps/v1/acme_shared/core.ts")
    );
    assert!(
        files
            .files()
            .contains_key("witforge_deps/v2/acme_shared/core.ts")
    );

    let v0_dep = &files.files()["witforge_deps/v0/acme_shared/core.ts"];
    assert!(
        v0_dep.contains("from \"@acme/proto/v0.acme.shared.v0_pb.js\""),
        "v0 disambiguated proto import mismatch:\n{v0_dep}"
    );
    assert!(
        v0_dep.contains("from \"@acme/plain/witforge_deps/v0/acme_shared/core.js\""),
        "v0 scoped plain import mismatch:\n{v0_dep}"
    );

    let v1_dep = &files.files()["witforge_deps/v1/acme_shared/core.ts"];
    assert!(
        v1_dep.contains("export * from \"../../v0/acme_shared/core.js\";"),
        "v1 must re-export from canonical owner v0:\n{v1_dep}"
    );

    let v2_dep = &files.files()["witforge_deps/v2/acme_shared/core.ts"];
    assert!(
        v2_dep.contains("from \"@acme/proto/v2.acme.shared.v0_pb.js\""),
        "v2 disambiguated proto import mismatch:\n{v2_dep}"
    );
    assert!(
        v2_dep.contains("extra"),
        "v2 dep glue must include v2-specific field:\n{v2_dep}"
    );

    // Retiring v0.1 via `--released` promotes v1 to canonical owner and drops `v0/` from output.
    let released = wit.join("released");
    fs::write(&released, "v0.1 retired\nv1.0\n").unwrap();
    let live_files =
        typescript_glue::plan_with_released(&wit, "../plain", "../proto", Some(&released)).unwrap();
    assert!(!live_files.files().contains_key("v0/index.ts"));
    assert!(live_files.files().contains_key("v1/index.ts"));
    assert!(live_files.files().contains_key("v2/index.ts"));
    let v1_promoted = &live_files.files()["witforge_deps/v1/acme_shared/core.ts"];
    assert!(
        v1_promoted.contains("from \"../../../../proto/v1.acme.shared.v0_pb.js\""),
        "promoted v1 dep should import depth-adjusted disambiguated proto:\n{v1_promoted}"
    );

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn fixtures_generate_check_diff_prune_and_enforce_ownership_and_collision_guards() {
    for rel in [
        "versions/wit",
        "deps-full/wit",
        "deps-world/wit",
        "deps-convert/wit",
    ] {
        let planned = typescript_glue::plan(&fixture(rel), "../plain", "../proto").unwrap();
        assert!(!planned.files().is_empty(), "{rel}");
        assert!(planned.files().contains_key("index.ts"), "{rel}");
        assert!(planned.files().contains_key("error.ts"), "{rel}");
        assert!(planned.files().contains_key("helpers.ts"), "{rel}");
    }

    let base = scratch_dir("guards");
    let wit = fixture("versions/wit");
    let out = base.join("ts-glue-out");

    // 1. generate + check + unified_diff + stale owned subdir pruning.
    typescript_glue::generate(&wit, &out, "../plain", "../proto").unwrap();
    assert!(
        typescript_glue::check(&wit, &out, "../plain", "../proto")
            .unwrap()
            .is_empty()
    );
    let header = HeaderStyle::LineComment.format_header("ts-glue", "example", "evolve");
    let stale_dir = out.join("v99");
    fs::create_dir_all(&stale_dir).unwrap();
    fs::write(
        stale_dir.join("stale.ts"),
        format!("{header}\nexport {{}};\n"),
    )
    .unwrap();
    let drift = typescript_glue::check(&wit, &out, "../plain", "../proto").unwrap();
    assert_eq!(drift.len(), 1);
    assert_eq!(drift[0].status, GeneratedFileStatus::Extra);
    typescript_glue::generate(&wit, &out, "../plain", "../proto").unwrap();
    assert!(!stale_dir.exists(), "stale owned subdir must be pruned");

    // 2. Foreign header (`ts` in `ts-glue` output root) is refused.
    let foreign_header = HeaderStyle::LineComment.format_header("ts", "example", "evolve");
    fs::write(
        out.join("foreign.ts"),
        format!("{foreign_header}\nexport {{}};\n"),
    )
    .unwrap();
    let foreign_err = typescript_glue::generate(&wit, &out, "../plain", "../proto").unwrap_err();
    assert!(
        format!("{foreign_err:#}").contains("generator `ts`"),
        "{foreign_err:#}"
    );
    fs::remove_file(out.join("foreign.ts")).unwrap();

    // 3. Unowned file colliding with expected path is refused and never overwritten.
    fs::write(out.join("error.ts"), "// hand-written\n").unwrap();
    let unowned_check = typescript_glue::check(&wit, &out, "../plain", "../proto").unwrap();
    assert!(
        unowned_check
            .iter()
            .any(|f| f.status == GeneratedFileStatus::Unowned),
        "{unowned_check:?}"
    );
    assert!(typescript_glue::generate(&wit, &out, "../plain", "../proto").is_err());
    assert_eq!(
        fs::read_to_string(out.join("error.ts")).unwrap(),
        "// hand-written\n"
    );

    // 4. Invalid import roots are refused with operational (non-contract) error.
    for bad in [
        "",
        "/abs/path",
        "../trailing/",
        "@scope-only",
        "../bad;semi",
    ] {
        let err = typescript_glue::plan(&wit, bad, "../proto").unwrap_err();
        assert!(
            !is_contract_error(&err),
            "`{bad}` should not be ContractError: {err:#}"
        );
        let err2 = typescript_glue::plan(&wit, "../plain", bad).unwrap_err();
        assert!(
            !is_contract_error(&err2),
            "`{bad}` should not be ContractError: {err2:#}"
        );
    }

    // 5. Reserved `functions` type export collision is refused as a ContractError.
    let bad_wit = base.join("reserved-wit");
    let v01 = bad_wit.join("v0.1");
    fs::create_dir_all(&v01).unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package acme:collide@0.1.0;\ninterface api { record functions { id: u32 } }\n",
    )
    .unwrap();
    let clash_err = typescript_glue::plan(&bad_wit, "../plain", "../proto").unwrap_err();
    assert!(
        is_contract_error(&clash_err),
        "reserved `functions` clash must be ContractError: {clash_err:#}"
    );

    // 6. Windows-reserved (`con`) and barrel-reserved (`index`) interface names emit safe file
    //    stems (`con_.ts`, `index_.ts`) without colliding with `v0/index.ts`.
    let stems_wit = base.join("safe-stems");
    let stems_v01 = stems_wit.join("v0.1");
    fs::create_dir_all(&stems_v01).unwrap();
    fs::write(
        stems_v01.join("contract.wit"),
        "package acme:stems@0.1.0;\n\
         interface con { record a { id: u8 } }\n\
         interface index { use con.{a}; record b { item: a } }\n",
    )
    .unwrap();
    lock_version_first(&stems_v01);
    let stems_files = typescript_glue::plan(&stems_wit, "../plain", "../proto").unwrap();
    assert!(stems_files.files().contains_key("v0/con_.ts"));
    assert!(stems_files.files().contains_key("v0/index_.ts"));
    assert!(stems_files.files().contains_key("v0/index.ts"));
    assert!(
        stems_files.files()["v0/index_.ts"].contains("from \"./con_.js\""),
        "index_.ts should import sibling glue via safe stem:\n{}",
        stems_files.files()["v0/index_.ts"]
    );

    // 7. Same-interface types named `value` and functions with parameters matching type names
    //    reference non-shadowable `_wf_t_*` internal bindings.
    let shadow_wit = base.join("shadow-safe");
    let shadow_v01 = shadow_wit.join("v0.1");
    fs::create_dir_all(&shadow_v01).unwrap();
    fs::write(
        shadow_v01.join("contract.wit"),
        "package acme:shadow@0.1.0;\n\
         interface api {\n\
           record value { n: u8 }\n\
           record request { v: value }\n\
           send: func(request: request) -> request;\n\
         }\n",
    )
    .unwrap();
    lock_version_first(&shadow_v01);
    let shadow_files = typescript_glue::plan(&shadow_wit, "../plain", "../proto").unwrap();
    let shadow_api = &shadow_files.files()["v0/api.ts"];
    assert!(shadow_api.contains("const _wf_t_value = {"), "{shadow_api}");
    assert!(
        shadow_api.contains("export const value = _wf_t_value;"),
        "{shadow_api}"
    );
    assert!(
        shadow_api.contains("_wf_t_value.toProto(value.v)"),
        "{shadow_api}"
    );
    assert!(
        shadow_api.contains("_wf_t_request.toProto(request)"),
        "{shadow_api}"
    );

    fs::remove_dir_all(&base).unwrap();
}
