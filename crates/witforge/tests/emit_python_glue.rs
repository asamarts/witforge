//! Offline Tier 1 integration tests for the Python checked protobuf glue emitter (`W13-005`).
//!
//! Runs with zero external toolchains (`protoc`, `python3`, `google.protobuf`) required so it
//! passes on Linux, macOS, Windows, and MSRV `1.88` inside `cargo test --workspace --locked`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use witforge::emit::output::{GeneratedFileStatus, HeaderStyle};
use witforge::emit::python_glue::{self, PythonGlueOptions};
use witforge::lock::{LockMode, WireLock};
use witforge::{is_contract_error, load, model, subset};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn scratch_dir(label: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "witforge-py-glue-{label}-{}-{n}",
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
fn python_glue_conformance_contract_emits_deterministic_lf_only_glue() {
    let base = scratch_dir("conformance-offline");
    let bundle_dir = base.join("bundle");
    witforge::conformance::generate(
        &bundle_dir,
        &witforge::conformance::ConformanceOptions::new(),
    )
    .unwrap();
    let wit_root = bundle_dir.join("wit");

    let first = python_glue::plan(&wit_root, "plain", "proto").unwrap();
    let second = python_glue::plan_with_options(
        &wit_root,
        "plain",
        "proto",
        None,
        &PythonGlueOptions::new(),
    )
    .unwrap();
    assert_eq!(first, second, "py-glue output must be deterministic");
    assert!(first.markers().contains("py.typed"));

    let expected_header =
        HeaderStyle::HashComment.format_header("py-glue", "witforge", "conformance");
    for (rel, content) in first.files() {
        assert!(!content.contains('\r'), "{rel} contains CR");
        if rel == "py.typed" {
            assert!(content.is_empty(), "py.typed marker must be empty");
        } else {
            assert!(
                content.starts_with(&expected_header),
                "{rel} missing py-glue header:\n{content}"
            );
        }
    }

    for required in [
        "__init__.py",
        "error.py",
        "helpers.py",
        "v0/__init__.py",
        "v0/types.py",
        "v0/api.py",
        "witforge_deps/__init__.py",
        "witforge_deps/witforge_conformance_dep/__init__.py",
        "witforge_deps/witforge_conformance_dep/shared.py",
    ] {
        assert!(
            first.files().contains_key(required),
            "missing expected py-glue file `{required}`; got: {:?}",
            first.files().keys().collect::<Vec<_>>()
        );
    }

    let error_py = &first.files()["error.py"];
    for kind in [
        "\"IntegerOutOfRange\"",
        "\"MissingField\"",
        "\"UnknownEnumValue\"",
        "\"UnknownCase\"",
        "\"InvalidState\"",
        "\"InvalidChar\"",
        "\"InvalidLength\"",
        "class WitforgeGlueError(ValueError):",
    ] {
        assert!(error_py.contains(kind), "error.py missing `{kind}`");
    }

    let helpers_py = &first.files()["helpers.py"];
    for helper in [
        "def require_present(",
        "isinstance(value, bool)",
        "def check_u8(",
        "def check_u16(",
        "def check_u32(",
        "def check_u64(",
        "def check_s8(",
        "def check_s16(",
        "def check_s32(",
        "def check_s64(",
        "def check_bool(",
        "def check_bytes(",
        "def decode_char(",
        "def encode_char(",
        "def check_length(",
    ] {
        assert!(helpers_py.contains(helper), "helpers.py missing `{helper}`");
    }

    let types_py = &first.files()["v0/types.py"];
    for needle in [
        "from __future__ import annotations",
        "from plain.v0_1 import types as _wf_plain",
        "from proto import v0_pb2 as _wf_proto",
        "proto_type: _wf_Final[type[_wf_proto.TypesIntegers]] = _wf_proto.TypesIntegers",
        "_wf_h.check_u8(",
        "_wf_h.check_u16(",
        "_wf_h.check_u32(",
        "_wf_h.check_u64(",
        "_wf_h.check_s8(",
        "_wf_h.check_s16(",
        "_wf_h.check_s32(",
        "_wf_h.check_s64(",
        "_wf_h.encode_char(",
        "_wf_h.decode_char(",
        "_wf_h.check_length(",
        "_wf_h.require_present(",
        ".HasField(",
        ".WhichOneof(",
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
        assert!(types_py.contains(needle), "v0/types.py missing `{needle}`");
    }

    let api_py = &first.files()["v0/api.py"];
    for needle in [
        "class functions:",
        "METHOD: _wf_Final[int] = _wf_proto.API_METHOD_ECHO_INTEGERS",
        "request_type: _wf_Final[type[_wf_proto.ApiEchoIntegersRequest]] = _wf_proto.ApiEchoIntegersRequest",
        "response_type: _wf_Final[type[_wf_proto.ApiEchoIntegersResponse]] = _wf_proto.ApiEchoIntegersResponse",
        "METHOD: _wf_Final[int] = _wf_proto.API_METHOD_PROCESS_FOREIGN",
        "METHOD: _wf_Final[int] = _wf_proto.API_METHOD_NOTIFY",
        "\"api#echo-integers.input\"",
        "\"api#process-foreign.code\"",
        "\"api#notify.priority\"",
    ] {
        assert!(api_py.contains(needle), "v0/api.py missing `{needle}`");
    }

    let dep_shared_py = &first.files()["witforge_deps/witforge_conformance_dep/shared.py"];
    assert!(
        dep_shared_py.contains("from proto.witforge.conformance_dep import v0_pb2 as _wf_proto"),
        "dep shared.py proto import mismatch:\n{dep_shared_py}"
    );
    assert!(
        dep_shared_py.contains(
            "from plain.witforge_deps.witforge_conformance_dep import shared as _wf_plain"
        ),
        "dep shared.py plain import mismatch:\n{dep_shared_py}"
    );

    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn python_glue_multi_major_disambiguated_deps_and_relative_roots() {
    let base = scratch_dir("multi-major-deps");
    let wit = base.join("wit");

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

    let files = python_glue::plan(&wit, "my_app.plain", "my_app.proto").unwrap();

    assert!(
        files
            .files()
            .contains_key("witforge_deps/v0/acme_shared/core.py")
    );
    assert!(
        files
            .files()
            .contains_key("witforge_deps/v1/acme_shared/core.py")
    );
    assert!(
        files
            .files()
            .contains_key("witforge_deps/v2/acme_shared/core.py")
    );

    let v0_dep = &files.files()["witforge_deps/v0/acme_shared/core.py"];
    assert!(
        v0_dep.contains("from my_app.proto.v0.acme.shared import v0_pb2 as _wf_proto"),
        "v0 disambiguated proto import mismatch:\n{v0_dep}"
    );
    assert!(
        v0_dep.contains("from my_app.plain.witforge_deps.v0.acme_shared import core as _wf_plain"),
        "v0 scoped plain import mismatch:\n{v0_dep}"
    );

    let v1_dep = &files.files()["witforge_deps/v1/acme_shared/core.py"];
    assert!(
        v1_dep.contains("from ...v0.acme_shared.core import ("),
        "v1 must re-export from canonical owner v0:\n{v1_dep}"
    );
    assert!(
        v1_dep.contains("tag as tag,"),
        "v1 re-export missing `tag`:\n{v1_dep}"
    );

    let v2_dep = &files.files()["witforge_deps/v2/acme_shared/core.py"];
    assert!(
        v2_dep.contains("from my_app.proto.v2.acme.shared import v0_pb2 as _wf_proto"),
        "v2 disambiguated proto import mismatch:\n{v2_dep}"
    );
    assert!(
        v2_dep.contains("extra"),
        "v2 dep glue must include v2-specific field:\n{v2_dep}"
    );

    // Retiring v0.1 via `--released` promotes v1 to canonical owner with relative root depth adjustment.
    let released = wit.join("released");
    fs::write(&released, "v0.1 retired\nv1.0\n").unwrap();
    let live_files =
        python_glue::plan_with_released(&wit, "..plain", "..proto", Some(&released)).unwrap();
    assert!(!live_files.files().contains_key("v0/__init__.py"));
    assert!(live_files.files().contains_key("v1/__init__.py"));
    assert!(live_files.files().contains_key("v2/__init__.py"));
    let v1_promoted = &live_files.files()["witforge_deps/v1/acme_shared/core.py"];
    assert!(
        v1_promoted.contains("from .....proto.v1.acme.shared import v0_pb2 as _wf_proto"),
        "promoted v1 dep should import depth-adjusted relative proto:\n{v1_promoted}"
    );

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn python_glue_fixtures_generate_check_markers_prune_and_guards() {
    for rel in [
        "versions/wit",
        "deps-full/wit",
        "deps-world/wit",
        "deps-convert/wit",
    ] {
        let planned = python_glue::plan(&fixture(rel), "plain", "proto").unwrap();
        assert!(!planned.files().is_empty(), "{rel}");
        assert!(planned.files().contains_key("__init__.py"), "{rel}");
        assert!(planned.files().contains_key("error.py"), "{rel}");
        assert!(planned.files().contains_key("helpers.py"), "{rel}");
        assert!(planned.markers().contains("py.typed"), "{rel}");
    }

    let base = scratch_dir("guards");
    let wit = fixture("versions/wit");
    let out = base.join("py-glue-out");

    // 1. generate + check + empty py.typed marker + stale owned subdir pruning.
    python_glue::generate(&wit, &out, "plain", "proto").unwrap();
    assert!(out.join("py.typed").is_file());
    assert_eq!(fs::read_to_string(out.join("py.typed")).unwrap(), "");
    assert!(
        python_glue::check(&wit, &out, "plain", "proto")
            .unwrap()
            .is_empty()
    );

    let header = HeaderStyle::HashComment.format_header("py-glue", "example", "evolve");
    let stale_dir = out.join("v99");
    fs::create_dir_all(&stale_dir).unwrap();
    fs::write(stale_dir.join("stale.py"), format!("{header}\n")).unwrap();
    let drift = python_glue::check(&wit, &out, "plain", "proto").unwrap();
    assert_eq!(drift.len(), 1);
    assert_eq!(drift[0].status, GeneratedFileStatus::Extra);
    python_glue::generate(&wit, &out, "plain", "proto").unwrap();
    assert!(!stale_dir.exists(), "stale owned subdir must be pruned");

    // 2. Foreign header (`py` in `py-glue` output root) is refused.
    let foreign_header = HeaderStyle::HashComment.format_header("py", "example", "evolve");
    fs::write(out.join("foreign.py"), format!("{foreign_header}\n")).unwrap();
    let foreign_err = python_glue::generate(&wit, &out, "plain", "proto").unwrap_err();
    assert!(
        format!("{foreign_err:#}").contains("generator `py`"),
        "{foreign_err:#}"
    );
    fs::remove_file(out.join("foreign.py")).unwrap();

    // 3. Unowned file colliding with expected path is refused and never overwritten.
    fs::write(out.join("error.py"), "# hand-written\n").unwrap();
    let unowned_check = python_glue::check(&wit, &out, "plain", "proto").unwrap();
    assert!(
        unowned_check
            .iter()
            .any(|f| f.status == GeneratedFileStatus::Unowned),
        "{unowned_check:?}"
    );
    assert!(python_glue::generate(&wit, &out, "plain", "proto").is_err());
    assert_eq!(
        fs::read_to_string(out.join("error.py")).unwrap(),
        "# hand-written\n"
    );

    // 4. Invalid Python module roots are refused with operational (non-contract) error.
    for bad in [
        "",
        "../plain",
        "/abs/path",
        "plain.",
        "a..b",
        "from",
        "my-pkg",
    ] {
        let err = python_glue::plan(&wit, bad, "proto").unwrap_err();
        assert!(
            !is_contract_error(&err),
            "`{bad}` should not be ContractError: {err:#}"
        );
        let err2 = python_glue::plan(&wit, "plain", bad).unwrap_err();
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
    let clash_err = python_glue::plan(&bad_wit, "plain", "proto").unwrap_err();
    assert!(
        is_contract_error(&clash_err),
        "reserved `functions` clash must be ContractError: {clash_err:#}"
    );

    // 6. Windows-reserved (`con`), Python-keyword (`class`), and shadowing-safe internal classes
    //    plus keyword dependency proto `importlib.import_module` fallback.
    let stems_wit = base.join("safe-stems");
    let stems_v01 = stems_wit.join("v0.1");
    fs::create_dir_all(stems_v01.join("deps/kw")).unwrap();
    fs::write(
        stems_v01.join("deps/kw/dep.wit"),
        "package %from:cls@0.1.0;\ninterface items { record entry { id: u8 } }\n",
    )
    .unwrap();
    fs::write(
        stems_v01.join("contract.wit"),
        "package acme:stems@0.1.0;\n\
         interface con { record a { id: u8 } }\n\
         interface %class {\n\
           use con.{a};\n\
           use %from:cls/items@0.1.0.{entry};\n\
           record value { n: u8 }\n\
           record request { item: a, e: entry, v: value, mro: u8, %from: u16, %import: option<u16> }\n\
           send: func(request: request, len: list<u8, 2>) -> request;\n\
         }\n",
    )
    .unwrap();
    lock_version_first(&stems_v01);
    let stems_files = python_glue::plan(&stems_wit, "plain", "proto").unwrap();
    assert!(stems_files.files().contains_key("v0/con_.py"));
    assert!(stems_files.files().contains_key("v0/class_.py"));
    let class_py = &stems_files.files()["v0/class_.py"];
    assert!(
        class_py.contains("from . import con_ as _wf_g_con_"),
        "{class_py}"
    );
    assert!(class_py.contains("class _WfT_value:"), "{class_py}");
    assert!(class_py.contains("value = _WfT_value"), "{class_py}");
    assert!(
        class_py.contains("_WfT_value.to_proto(value.v)"),
        "{class_py}"
    );
    assert!(
        class_py.contains("_WfT_request.to_proto(request)"),
        "{class_py}"
    );
    assert!(class_py.contains("_wf_builtins.len("), "{class_py}");
    assert!(
        class_py.contains("setattr(_wf_out, \"from\","),
        "{class_py}"
    );
    assert!(
        class_py.contains("require_present(value, \"from\", \"class/request.from\")"),
        "{class_py}"
    );
    assert!(
        class_py.contains("getattr(value, \"import\")"),
        "{class_py}"
    );
    let kw_dep_py = &stems_files.files()["witforge_deps/from_cls/items.py"];
    assert!(
        kw_dep_py.contains("_wf_importlib.import_module(\"proto.from.cls.v0_pb2\")"),
        "keyword dep proto must use importlib fallback:\n{kw_dep_py}"
    );

    fs::remove_dir_all(&base).unwrap();
}
