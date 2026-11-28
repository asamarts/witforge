//! Offline Tier 1 integration tests for the Go checked protobuf glue emitter (`W13-007`).
//!
//! Runs with zero external toolchains (`protoc`, `go`, `gofmt`, `google.golang.org/protobuf`)
//! required so it passes on Linux, macOS, Windows, and MSRV `1.88` inside
//! `cargo test --workspace --locked`. When `gofmt` is available on `PATH`, it also verifies
//! zero-diff canonical `gofmt` formatting on every emitted `.go` file.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use witforge::emit::go_glue::{self, GoGlueOptions};
use witforge::emit::output::{GeneratedFileStatus, HeaderStyle};
use witforge::lock::{LockMode, WireLock};
use witforge::{is_contract_error, load, model, subset};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn scratch_dir(label: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "witforge-go-glue-{label}-{}-{n}",
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

fn assert_gofmt_if_available(dir: &Path) {
    let Ok(output) = Command::new("gofmt").arg("-d").arg(dir).output() else {
        return;
    };
    assert!(
        output.status.success(),
        "gofmt failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diff = String::from_utf8_lossy(&output.stdout);
    assert!(
        diff.is_empty(),
        "generated Go glue is not gofmt-clean:\n{diff}"
    );
}

#[allow(clippy::too_many_lines)]
#[test]
fn go_glue_conformance_contract_emits_deterministic_lf_only_glue() {
    let base = scratch_dir("conformance-offline");
    let bundle_dir = base.join("bundle");
    witforge::conformance::generate(
        &bundle_dir,
        &witforge::conformance::ConformanceOptions::new(),
    )
    .unwrap();
    let wit_root = bundle_dir.join("wit");
    let go_module = "example.com/witforge/conformance/glue";
    let plain_root = "example.com/witforge/conformance/plain";
    let proto_root = "example.com/witforge/conformance/proto";

    let first = go_glue::plan(&wit_root, go_module, plain_root, proto_root).unwrap();
    let second = go_glue::plan_with_options(
        &wit_root,
        go_module,
        plain_root,
        proto_root,
        None,
        &GoGlueOptions::new(),
    )
    .unwrap();
    assert_eq!(first, second, "go-glue output must be deterministic");

    let expected_header =
        HeaderStyle::GoGenerated.format_header("go-glue", "witforge", "conformance");
    for (rel, content) in first.files() {
        assert!(!content.contains('\r'), "{rel} contains CR");
        assert!(
            content.starts_with(&expected_header),
            "{rel} missing go-glue ownership header:\n{content}"
        );
        assert!(content.ends_with('\n'), "{rel} missing trailing newline");
    }

    for required in [
        "doc.go",
        "glueerr/error.go",
        "helpers/helpers.go",
        "v0/types/glue.go",
        "v0/api/glue.go",
        "witforge_deps/witforge_conformance_dep/shared/glue.go",
    ] {
        assert!(
            first.files().contains_key(required),
            "missing expected go-glue file `{required}`; got: {:?}",
            first.files().keys().collect::<Vec<_>>()
        );
    }

    assert!(
        first.files()["doc.go"].contains("package glue\n"),
        "doc.go:\n{}",
        first.files()["doc.go"]
    );

    let error_go = &first.files()["glueerr/error.go"];
    for needle in [
        "package glueerr",
        "KindIntegerOutOfRange ErrorKind = \"IntegerOutOfRange\"",
        "KindMissingField      ErrorKind = \"MissingField\"",
        "KindUnknownEnumValue  ErrorKind = \"UnknownEnumValue\"",
        "KindUnknownCase       ErrorKind = \"UnknownCase\"",
        "KindInvalidState      ErrorKind = \"InvalidState\"",
        "KindInvalidChar       ErrorKind = \"InvalidChar\"",
        "KindInvalidLength     ErrorKind = \"InvalidLength\"",
        "type GlueError struct {",
        "func (e *GlueError) Error() string {",
    ] {
        assert!(
            error_go.contains(needle),
            "glueerr/error.go missing `{needle}`"
        );
    }

    let helpers_go = &first.files()["helpers/helpers.go"];
    for needle in [
        "package helpers",
        "func DecodeU8(",
        "func DecodeU16(",
        "func DecodeS8(",
        "func DecodeS16(",
        "func DecodeChar(",
        "func EncodeChar(",
        "func CheckUTF8(",
        "func NonNilBytes(",
        "func CheckFixedBytes(",
        "func Ptr[T any](",
        "func SortedKeys[K cmp.Ordered, V any](",
    ] {
        assert!(
            helpers_go.contains(needle),
            "helpers/helpers.go missing `{needle}`"
        );
    }

    let types_go = &first.files()["v0/types/glue.go"];
    for needle in [
        "package types",
        "wf_plain \"example.com/witforge/conformance/plain/v0_1/types\"",
        "wf_pb \"example.com/witforge/conformance/proto/witforge/conformance/v0\"",
        "func IntegersToProto(val wf_plain.Integers) (*wf_pb.TypesIntegers, error)",
        "func IntegersFromProto(msg *wf_pb.TypesIntegers) (wf_plain.Integers, error)",
        "func ScalarsToProto(",
        "func ScalarsFromProto(",
        "func CollectionsToProto(",
        "func CollectionsFromProto(",
        "FixedU16S",
        "DecodeU8(",
        "DecodeU16(",
        "DecodeS8(",
        "DecodeS16(",
        "DecodeChar(",
        "EncodeChar(",
        "CheckUTF8(",
        "NonNilBytes(",
        "CheckFixedBytes(",
        "SortedKeys(",
        "\"types/integers.a-u8\"",
        "\"types/scalars.a-char\"",
        "\"types/collections.fixed-bytes\"",
        "\"types/maps.char-to-color\"",
        "\"types/choice\"",
        "\"types/color\"",
    ] {
        assert!(
            types_go.contains(needle),
            "v0/types/glue.go missing `{needle}`:\n{types_go}"
        );
    }

    let api_go = &first.files()["v0/api/glue.go"];
    for needle in [
        "package api",
        "type Method = wf_pb.ApiMethod",
        "wf_pb.ApiMethod_API_METHOD_ECHO_INTEGERS",
        "wf_pb.ApiMethod_API_METHOD_PROCESS_FOREIGN",
        "wf_pb.ApiMethod_API_METHOD_NOTIFY",
        "MethodEchoIntegers",
        "MethodProcessForeign",
        "MethodNotify",
        "func EchoIntegersToRequest(",
        "func EchoIntegersFromRequest(",
        "func EchoIntegersToResponse(",
        "func EchoIntegersFromResponse(",
        "func ProcessForeignToRequest(",
        "func ProcessForeignFromRequest(",
        "func NotifyToRequest(",
        "func NotifyFromRequest(",
        "func NotifyToResponse()",
        "func NotifyFromResponse(",
        "\"api#echo-integers.input\"",
        "\"api#process-foreign.code\"",
        "\"api#notify.priority\"",
    ] {
        assert!(
            api_go.contains(needle),
            "v0/api/glue.go missing `{needle}`:\n{api_go}"
        );
    }

    let dep_shared_go = &first.files()["witforge_deps/witforge_conformance_dep/shared/glue.go"];
    assert!(
        dep_shared_go.contains(
            "wf_pb \"example.com/witforge/conformance/proto/witforge/conformance_dep/v0\""
        ),
        "dep shared glue.go proto import mismatch:\n{dep_shared_go}"
    );
    assert!(
        dep_shared_go.contains(
            "wf_plain \"example.com/witforge/conformance/plain/witforge_deps/witforge_conformance_dep/shared\""
        ),
        "dep shared glue.go plain import mismatch:\n{dep_shared_go}"
    );

    let out = base.join("glue-out");
    first.write(&out).unwrap();
    assert_gofmt_if_available(&out);

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn go_glue_multi_major_disambiguated_deps_canonical_owner_reexports_and_retirement() {
    let base = scratch_dir("multi-major-deps");
    let wit = base.join("wit");
    let go_module = "example.com/acme/app/gen/glue";
    let plain_root = "example.com/acme/app/gen/plain";
    let proto_root = "example.com/acme/app/gen/proto";

    for (ver, pkg_ver, dep_extra) in [
        ("v0.1", "0.1.0", ""),
        ("v1.0", "1.0.0", ""),
        ("v2.0", "2.0.0", "\n    extra: bool,"),
    ] {
        let dir = wit.join(ver);
        fs::create_dir_all(dir.join("deps/common")).unwrap();
        fs::create_dir_all(dir.join("deps/shared")).unwrap();
        fs::write(
            dir.join("deps/common/common.wit"),
            "package acme:common@0.1.0;\n\
             interface ids {\n\
               record id { raw: u64 }\n\
             }\n",
        )
        .unwrap();
        fs::write(
            dir.join("deps/shared/dep.wit"),
            format!(
                "package acme:shared@0.1.0;\n\
                 interface core {{\n\
                   enum status {{ active, archived }}\n\
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
                   use acme:common/ids@0.1.0.{{id}};\n\
                   use acme:shared/core@0.1.0.{{tag, status}};\n\
                   record env {{ id: id, tag: tag, status: status }}\n\
                 }}\n"
            ),
        )
        .unwrap();
        lock_version_first(&dir);
    }

    let files = go_glue::plan(&wit, go_module, plain_root, proto_root).unwrap();

    for required in [
        "witforge_deps/acme_common/ids/glue.go",
        "witforge_deps/v0/acme_shared/core/glue.go",
        "witforge_deps/v1/acme_shared/core/glue.go",
        "witforge_deps/v2/acme_shared/core/glue.go",
    ] {
        assert!(
            files.files().contains_key(required),
            "missing `{required}`; keys: {:?}",
            files.files().keys().collect::<Vec<_>>()
        );
    }

    let v0_dep = &files.files()["witforge_deps/v0/acme_shared/core/glue.go"];
    assert!(
        v0_dep.contains("wf_pb \"example.com/acme/app/gen/proto/acme/shared/v0_m0\""),
        "v0 disambiguated proto import mismatch:\n{v0_dep}"
    );
    assert!(
        v0_dep.contains(
            "wf_plain \"example.com/acme/app/gen/plain/witforge_deps/v0/acme_shared/core\""
        ),
        "v0 scoped plain import mismatch:\n{v0_dep}"
    );

    let v1_dep = &files.files()["witforge_deps/v1/acme_shared/core/glue.go"];
    assert!(
        v1_dep.contains(
            "wf_owner \"example.com/acme/app/gen/glue/witforge_deps/v0/acme_shared/core\""
        ),
        "v1 must forward to canonical owner v0:\n{v1_dep}"
    );
    assert!(
        v1_dep.contains("return wf_owner.TagToProto(val)"),
        "v1 forwarding body mismatch:\n{v1_dep}"
    );
    assert!(
        v1_dep.contains("return wf_owner.StatusFromProto(msg)"),
        "v1 enum forwarding body mismatch:\n{v1_dep}"
    );

    let v2_dep = &files.files()["witforge_deps/v2/acme_shared/core/glue.go"];
    assert!(
        v2_dep.contains("wf_pb \"example.com/acme/app/gen/proto/acme/shared/v0_m2\""),
        "v2 disambiguated proto import mismatch:\n{v2_dep}"
    );
    assert!(
        v2_dep.contains("Extra"),
        "v2 dep glue must include v2-specific field:\n{v2_dep}"
    );

    // Retiring v0.1 via `--released` promotes v1 to canonical owner.
    let released = wit.join("released");
    fs::write(&released, "v0.1 retired\nv1.0\n").unwrap();
    let live_files =
        go_glue::plan_with_released(&wit, go_module, plain_root, proto_root, Some(&released))
            .unwrap();
    assert!(!live_files.files().contains_key("v0/api/glue.go"));
    assert!(live_files.files().contains_key("v1/api/glue.go"));
    assert!(live_files.files().contains_key("v2/api/glue.go"));
    let v1_promoted = &live_files.files()["witforge_deps/v1/acme_shared/core/glue.go"];
    assert!(
        v1_promoted.contains("wf_pb \"example.com/acme/app/gen/proto/acme/shared/v0_m1\""),
        "promoted v1 dep should import v1 proto:\n{v1_promoted}"
    );
    assert!(
        !v1_promoted.contains("wf_owner"),
        "promoted v1 dep must emit full glue rather than forwarding:\n{v1_promoted}"
    );

    let out = base.join("glue-out");
    files.write(&out).unwrap();
    assert_gofmt_if_available(&out);

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn go_glue_fixtures_generate_check_prune_ownership_escaping_and_guards() {
    let go_module = "example.com/witforge/fixtures/glue";
    let plain_root = "example.com/witforge/fixtures/plain";
    let proto_root = "example.com/witforge/fixtures/proto";

    for rel in [
        "versions/wit",
        "deps-full/wit",
        "deps-world/wit",
        "deps-convert/wit",
    ] {
        let planned = go_glue::plan(&fixture(rel), go_module, plain_root, proto_root).unwrap();
        assert!(!planned.files().is_empty(), "{rel}");
        assert!(planned.files().contains_key("doc.go"), "{rel}");
        assert!(planned.files().contains_key("glueerr/error.go"), "{rel}");
        assert!(planned.files().contains_key("helpers/helpers.go"), "{rel}");
    }

    // Multi-minor major (`versions/wit` has `v0.1` + `v0.2`): `v0` glue targets highest minor `v0_2`.
    let versions_plan =
        go_glue::plan(&fixture("versions/wit"), go_module, plain_root, proto_root).unwrap();
    let v0_types = &versions_plan.files()["v0/types/glue.go"];
    assert!(
        v0_types.contains("wf_plain \"example.com/witforge/fixtures/plain/v0_2/types\""),
        "v0 glue must target highest minor v0_2:\n{v0_types}"
    );

    let base = scratch_dir("guards");
    let wit = fixture("versions/wit");
    let out = base.join("go-glue-out");

    // 1. generate/check (`_with_options` + default) + stale owned subdir pruning.
    go_glue::generate_with_options(
        &wit,
        &out,
        go_module,
        plain_root,
        proto_root,
        &GoGlueOptions::new(),
    )
    .unwrap();
    assert_gofmt_if_available(&out);
    assert!(
        go_glue::check_with_options(
            &wit,
            &out,
            go_module,
            plain_root,
            proto_root,
            &GoGlueOptions::new(),
        )
        .unwrap()
        .is_empty()
    );

    let header = HeaderStyle::GoGenerated.format_header("go-glue", "example", "evolve");
    let stale_dir = out.join("v99/stale");
    fs::create_dir_all(&stale_dir).unwrap();
    fs::write(
        stale_dir.join("glue.go"),
        format!("{header}\npackage stale\n"),
    )
    .unwrap();
    let drift = go_glue::check(&wit, &out, go_module, plain_root, proto_root).unwrap();
    assert_eq!(drift.len(), 1);
    assert_eq!(drift[0].status, GeneratedFileStatus::Extra);
    go_glue::generate(&wit, &out, go_module, plain_root, proto_root).unwrap();
    assert!(
        !out.join("v99").exists(),
        "stale owned subdir must be pruned"
    );

    // 2. Foreign header (`go` in `go-glue` output root) is refused.
    let foreign_header = HeaderStyle::GoGenerated.format_header("go", "example", "evolve");
    fs::write(
        out.join("foreign.go"),
        format!("{foreign_header}\npackage foreign\n"),
    )
    .unwrap();
    let foreign_err = go_glue::generate(&wit, &out, go_module, plain_root, proto_root).unwrap_err();
    assert!(
        format!("{foreign_err:#}").contains("generator `go`"),
        "{foreign_err:#}"
    );
    fs::remove_file(out.join("foreign.go")).unwrap();

    // 3. Unowned file colliding with expected path is refused and never overwritten.
    fs::write(
        out.join("glueerr/error.go"),
        "// hand-written\npackage glueerr\n",
    )
    .unwrap();
    let unowned_check = go_glue::check(&wit, &out, go_module, plain_root, proto_root).unwrap();
    assert!(
        unowned_check
            .iter()
            .any(|f| f.status == GeneratedFileStatus::Unowned),
        "{unowned_check:?}"
    );
    assert!(go_glue::generate(&wit, &out, go_module, plain_root, proto_root).is_err());
    assert_eq!(
        fs::read_to_string(out.join("glueerr/error.go")).unwrap(),
        "// hand-written\npackage glueerr\n"
    );

    // 4. Invalid Go module paths (for all 3 arguments) are refused with WF9001 (not ContractError).
    for bad in [
        "",
        "../relative",
        "/leading/slash",
        "example.com/trailing/",
        "example.com/bad;alias",
        "example.com/con/pkg",
        "example.com/foo~1/pkg",
    ] {
        for (gm, pl, pr) in [
            (bad, plain_root, proto_root),
            (go_module, bad, proto_root),
            (go_module, plain_root, bad),
        ] {
            let err = go_glue::plan(&wit, gm, pl, pr).unwrap_err();
            assert!(
                !is_contract_error(&err),
                "`{bad}` must be operational WF9001, not ContractError: {err:#}"
            );
            assert!(
                witforge::error_diagnostics(&err)
                    .is_some_and(|diags| diags.iter().any(|d| d.code == "WF9001")),
                "`{bad}` must carry WF9001: {err:#}"
            );
        }
    }

    // 5. Output root inside WIT tree is refused before writing.
    let inside_err =
        go_glue::generate(&wit, &wit.join("inside"), go_module, plain_root, proto_root)
            .unwrap_err();
    assert!(
        format!("{inside_err:#}").contains("must not be the WIT root"),
        "{inside_err:#}"
    );

    // 6. Special package names (`main`, `init`, `documentation`, `type`), reserved `protoc-gen-go`
    //    field names (`string`, `reset`, `marshal`, `descriptor`), and `+build` doc sanitization.
    let edge_wit = base.join("edge-wit");
    let edge_v01 = edge_wit.join("v0.1");
    fs::create_dir_all(&edge_v01).unwrap();
    fs::write(
        edge_v01.join("contract.wit"),
        "package acme:edge@0.1.0;\n\
         interface main {\n\
           /// +build linux\n\
           /// Line after build tag.\n\
           record tricky {\n\
             %string: string,\n\
             reset: u8,\n\
             marshal: bool,\n\
             descriptor: list<u16, 2>,\n\
           }\n\
           call: func(t: tricky) -> tricky;\n\
         }\n\
         interface init {\n\
           record note { id: u8 }\n\
         }\n\
         interface documentation {\n\
           record info { text: string }\n\
         }\n",
    )
    .unwrap();
    lock_version_first(&edge_v01);

    let edge_files = go_glue::plan(
        &edge_wit,
        "example.com/acme/main",
        "example.com/acme/plain",
        "example.com/acme/proto",
    )
    .unwrap();
    assert!(
        edge_files.files()["doc.go"].contains("package main_\n"),
        "root `main` package must be escaped:\n{}",
        edge_files.files()["doc.go"]
    );
    assert!(edge_files.files().contains_key("v0/main_/glue.go"));
    assert!(edge_files.files().contains_key("v0/init_/glue.go"));
    assert!(edge_files.files().contains_key("v0/documentation_/glue.go"));
    let main_glue = &edge_files.files()["v0/main_/glue.go"];
    for needle in [
        "package main_",
        "String_",
        "Reset_",
        "Marshal_",
        "Descriptor_",
    ] {
        assert!(
            main_glue.contains(needle),
            "v0/main_/glue.go missing `{needle}`:\n{main_glue}"
        );
    }
    let edge_out = base.join("edge-out");
    edge_files.write(&edge_out).unwrap();
    assert_gofmt_if_available(&edge_out);

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn go_glue_audit_regressions_named_options_aliases_synth_oneofs_imports_and_collisions() {
    let base = scratch_dir("audit-regressions");
    let go_module = "example.com/acme/audit/glue";
    let plain_root = "example.com/acme/audit/plain";
    let proto_root = "example.com/acme/audit/proto";

    // 1. Distinct module-root validation (`WF9001`, not ContractError).
    let wit_versions = fixture("versions/wit");
    for (gm, pl, pr) in [
        (go_module, go_module, proto_root),
        (go_module, plain_root, go_module),
        (go_module, plain_root, plain_root),
    ] {
        let err = go_glue::plan(&wit_versions, gm, pl, pr).unwrap_err();
        assert!(!is_contract_error(&err), "{err:#}");
        assert!(
            witforge::error_diagnostics(&err)
                .is_some_and(|diags| diags.iter().any(|d| d.code == "WF9001")),
            "{err:#}"
        );
    }

    // 2. Digit-boundary top-level `protoc-gen-go` collision (`AFoo2bar` vs `AFoo2Bar` -> `AFoo2Bar`,
    //    distinct in raw proto and `prost`, colliding only in `protoc-gen-go`) is caught
    //    consistently by `subset::check_diagnostics` (`WF1003`), `proto::plan`, and `go_glue::plan`.
    let col_wit = base.join("col-wit");
    let col_v01 = col_wit.join("v0.1");
    fs::create_dir_all(&col_v01).unwrap();
    fs::write(
        col_v01.join("contract.wit"),
        "package acme:digitcol@0.1.0;\n\
         interface a {\n\
           record foo2bar { x: u32 }\n\
         }\n\
         interface a-foo2 {\n\
           record bar { x: u32 }\n\
         }\n",
    )
    .unwrap();
    let loaded_col = load(&col_v01).unwrap();
    let col_diags = subset::check_diagnostics(&loaded_col);
    assert!(
        col_diags.iter().any(|d| d.code == "WF1003"
            && d.message.contains("protoc-gen-go")
            && d.message.contains("AFoo2Bar")),
        "expected WF1003 protoc-gen-go collision for AFoo2bar vs AFoo2Bar, got: {col_diags:#?}"
    );
    let proto_err = witforge::emit::proto::plan(&col_wit).unwrap_err();
    assert!(is_contract_error(&proto_err), "{proto_err:#}");
    assert!(
        format!("{proto_err:#}").contains("protoc-gen-go"),
        "{proto_err:#}"
    );
    let glue_err = go_glue::plan(&col_wit, go_module, plain_root, proto_root).unwrap_err();
    assert!(is_contract_error(&glue_err), "{glue_err:#}");
    assert!(
        format!("{glue_err:#}").contains("protoc-gen-go"),
        "{glue_err:#}"
    );

    // 3. Reproducers for all 4 Critical findings + empty-arm guards + local alias inlining + int64 enum.
    let reg_wit = base.join("reg-wit");
    let reg_v01 = reg_wit.join("v0.1");
    fs::create_dir_all(reg_v01.join("deps/dep-a")).unwrap();
    fs::write(
        reg_v01.join("deps/dep-a/types.wit"),
        "package dep:a@0.1.0;\n\
         interface types {\n\
           enum color { red, blue }\n\
           record tag { id: u8 }\n\
           type opt-color = option<color>;\n\
         }\n",
    )
    .unwrap();
    fs::write(
        reg_v01.join("contract.wit"),
        "package acme:reg@0.1.0;\n\
         interface a {\n\
           use dep:a/types@0.1.0.{opt-color, tag};\n\
           type my-u8 = u8;\n\
           type hash = list<u8, 4>;\n\
           type opt-list = option<list<u16>>;\n\
           type opt-of-opt-list = option<opt-list>;\n\
           type opt-opt-color = option<opt-color>;\n\
           enum level { low, high }\n\
           variant kind { empty-arm, payload(u8) }\n\
           type bare = result;\n\
           record synth {\n\
             a: u32,\n\
             x-a: u32,\n\
             get-x-a: u32,\n\
             get-a: u32,\n\
             aliased: my-u8,\n\
             opt-tag: option<tag>,\n\
             opt-hash: option<hash>,\n\
             inline-results: list<result<u8, string>>,\n\
           }\n\
           send: func(a: u32, x-a: u32);\n\
         }\n\
         interface b {\n\
           use a.{opt-list, opt-opt-color};\n\
           type opt3 = option<opt-opt-color>;\n\
           record holder {\n\
             xs: list<opt-list>,\n\
             nested: option<opt-list>,\n\
           }\n\
         }\n",
    )
    .unwrap();
    lock_version_first(&reg_v01);

    let planned = go_glue::plan(&reg_wit, go_module, plain_root, proto_root).unwrap();
    let a_glue = &planned.files()["v0/a/glue.go"];
    let b_glue = &planned.files()["v0/b/glue.go"];
    let err_go = &planned.files()["glueerr/error.go"];

    // Core #1: `OptOfOptList` encodes/decodes via `WitforgeListOfU16` (not `AOptList`).
    assert!(
        a_glue.contains("&wf_pb.WitforgeListOfU16{Values:"),
        "OptOfOptList must construct WitforgeListOfU16:\n{a_glue}"
    );
    // Core #2: `b` uses `wf_g_a.OptListToProto` / `wf_g_a.OptListFromProto` for `WitforgeOptionOfAOptList`.
    assert!(
        b_glue.contains("wf_g_a.OptListToProto(") && b_glue.contains("wf_g_a.OptListFromProto("),
        "b/glue.go must use AOptList wrapper via wf_g_a:\n{b_glue}"
    );
    // Core #3: proto3 `optional` synthetic oneofs rename `x_a` -> `XA_` and `get_a` -> `GetA_`,
    // while `get_x_a` stays `GetXA`.
    for field_ref in ["wf_out.A ", "wf_out.XA_ ", "wf_out.GetXA ", "wf_out.GetA_ "] {
        assert!(
            a_glue.contains(field_ref),
            "v0/a/glue.go missing synthetic-oneof-resolved field `{field_ref}`:\n{a_glue}"
        );
    }
    // Core #4: `b/glue.go` (`opt3 = option<opt-opt-color>`) must not carry an unused
    // `wf_pb_dep_a` import from probing foreign option wrappers, while `a/glue.go` only imports
    // `wf_pb_dep_a` for its `use`-alias signatures (`OptColor*`/`Tag*`) and references it there.
    assert!(
        !b_glue.contains("wf_pb_dep_a"),
        "v0/b/glue.go must not import unused wf_pb_dep_a:\n{b_glue}"
    );
    assert!(
        a_glue.contains("wf_pb_dep_a.TypesTag") && a_glue.contains("wf_pb_dep_a.TypesColor"),
        "v0/a/glue.go should use wf_pb_dep_a in re-exported signatures:\n{a_glue}"
    );
    // Core #6: nil `*pb.Empty` checks on unit variant, empty result, and option None arms.
    assert!(
        a_glue.contains("if wf_c == nil || wf_c.EmptyArm == nil"),
        "missing unit-variant nil Empty check:\n{a_glue}"
    );
    assert!(
        a_glue.contains("if wf_c == nil || wf_c.Ok == nil")
            && a_glue.contains("if wf_c == nil || wf_c.Err == nil"),
        "missing empty result nil Empty checks:\n{a_glue}"
    );
    assert!(
        a_glue.contains("if wf_c == nil || wf_c.None == nil"),
        "missing option None nil Empty check:\n{a_glue}"
    );
    // Core #7: local alias `aliased: my-u8` reports `"a/synth.aliased"`, and `opt-hash: option<hash>`
    // calls `HashFromProto`.
    assert!(
        a_glue.contains("DecodeU8(*msg.Aliased, \"a/synth.aliased\")"),
        "local alias should inline with field identity:\n{a_glue}"
    );
    assert!(
        a_glue.contains("HashFromProto(msg.OptHash)"),
        "named fixed-bytes inside option should delegate to HashFromProto:\n{a_glue}"
    );
    // Core #8: `UnknownEnumValue` takes `int64` and `emit_enum` passes `int64(val)` / `int64(msg)`.
    assert!(
        err_go.contains("func UnknownEnumValue(enumeration string, value int64) *GlueError"),
        "UnknownEnumValue must accept int64:\n{err_go}"
    );
    assert!(
        a_glue.contains("UnknownEnumValue(\"a/level\", int64(val))")
            && a_glue.contains("UnknownEnumValue(\"a/level\", int64(msg))"),
        "emit_enum must pass int64 without int32 truncation:\n{a_glue}"
    );

    let reg_out = base.join("reg-out");
    planned.write(&reg_out).unwrap();
    assert_gofmt_if_available(&reg_out);

    fs::remove_dir_all(&base).unwrap();
}
