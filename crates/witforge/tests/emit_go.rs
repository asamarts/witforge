//! Offline Tier 1 integration tests for the Go 1.22+ plain emitter (`W13-006`).
//!
//! Runs with zero external toolchains (`go`, `gofmt`) required so it passes on Linux, macOS,
//! Windows, and MSRV `1.88` inside `cargo test --workspace --locked`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use witforge::emit::go::{self, GoOptions};
use witforge::emit::output::{GeneratedFileStatus, HeaderStyle};
use witforge::lock::{LockMode, WireLock};
use witforge::{is_contract_error, load, model, subset};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn scratch_dir(label: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("witforge-go-{label}-{}-{n}", std::process::id()));
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
fn go_conformance_contract_emits_deterministic_lf_only_packages() {
    let base = scratch_dir("conformance-offline");
    let bundle_dir = base.join("bundle");
    witforge::conformance::generate(
        &bundle_dir,
        &witforge::conformance::ConformanceOptions::new(),
    )
    .unwrap();
    let wit_root = bundle_dir.join("wit");
    let module = "example.com/witforge/conformance/plain";

    let first = go::plan(&wit_root, module).unwrap();
    let second = go::plan_with_options(&wit_root, module, None, &GoOptions::new()).unwrap();
    assert_eq!(first, second, "go output must be deterministic");

    let expected_header = HeaderStyle::GoGenerated.format_header("go", "witforge", "conformance");
    for (rel, content) in first.files() {
        assert!(!content.contains('\r'), "{rel} contains CR");
        assert!(
            content.starts_with(&expected_header),
            "{rel} missing Go ownership header:\n{content}"
        );
        assert!(content.ends_with('\n'), "{rel} missing trailing newline");
    }

    for required in [
        "doc.go",
        "witforge_types/types.go",
        "v0_1/types/types.go",
        "v0_1/api/types.go",
        "witforge_deps/witforge_conformance_dep/shared/types.go",
    ] {
        assert!(
            first.files().contains_key(required),
            "missing expected Go file `{required}`; got: {:?}",
            first.files().keys().collect::<Vec<_>>()
        );
    }

    assert!(
        first.files()["doc.go"].contains("package plain\n"),
        "doc.go:\n{}",
        first.files()["doc.go"]
    );

    let shared_go = &first.files()["witforge_types/types.go"];
    for needle in [
        "package witforge_types",
        "type Option[T any] struct {",
        "func Some[T any](value T) Option[T]",
        "func None[T any]() Option[T]",
        "type Result[T, E any] struct {",
        "func NewOk[T, E any](value T) Result[T, E]",
        "func NewErr[T, E any](err E) Result[T, E]",
    ] {
        assert!(
            shared_go.contains(needle),
            "witforge_types/types.go missing `{needle}`:\n{shared_go}"
        );
    }

    let types_go = &first.files()["v0_1/types/types.go"];
    for needle in [
        "package types",
        "wf_dep_witforge_conformance_dep_shared \"example.com/witforge/conformance/plain/witforge_deps/witforge_conformance_dep/shared\"",
        "wf_types \"example.com/witforge/conformance/plain/witforge_types\"",
        "type Integers struct {",
        "\tAU8  uint8",
        "\tAU16 uint16",
        "\tAU32 uint32",
        "\tAU64 uint64",
        "\tAS8  int8",
        "\tAS16 int16",
        "\tAS32 int32",
        "\tAS64 int64",
        "type Scalars struct {",
        "\tAF32    float32",
        "\tAF64    float64",
        "\tABool   bool",
        "\tAChar   rune",
        "\tAString string",
        "type Color uint32",
        "\tColorRed   Color = 1",
        "\tColorGreen Color = 2",
        "\tColorBlue  Color = 3",
        "func (v Color) Valid() bool {",
        "case ColorRed, ColorGreen, ColorBlue:",
        "func (v Color) String() string {",
        "return \"red\"",
        "type Permissions struct {",
        "\tRead    bool",
        "\tWrite   bool",
        "\tExecute bool",
        "type Choice interface {",
        "\tisChoice()",
        "type ChoiceUnitCase struct{}",
        "func (ChoiceUnitCase) isChoice() {}",
        "type ChoiceSmall struct {",
        "\tValue uint8",
        "type Outcome = wf_types.Result[uint16, string]",
        "type OkOnly = wf_types.Result[uint8, struct{}]",
        "type ErrOnly = wf_types.Result[struct{}, Color]",
        "type BareResult = wf_types.Result[struct{}, struct{}]",
        "\tNestedOpt     wf_types.Option[*uint16]",
        "\tFixedBytes  [4]byte",
        "\tFixedU16s   [3]uint16",
        "\tFixedChars  [2]rune",
        "\tCharToColor map[rune]Color",
        "\tBoolToS16   map[bool]int16",
        "\tU64ToS64    map[uint64]int64",
    ] {
        assert!(
            types_go.contains(needle),
            "v0_1/types/types.go missing `{needle}`:\n{types_go}"
        );
    }

    let api_go = &first.files()["v0_1/api/types.go"];
    for needle in [
        "package api",
        "type Api interface {",
        "\tEchoIntegers(input Integers) Integers",
        "\tProcessForeign(meta SharedMeta, status SharedStatus, code uint16) wf_types.Result[Point, Color]",
        "\tNotify(label string, priority uint8)",
    ] {
        assert!(
            api_go.contains(needle),
            "v0_1/api/types.go missing `{needle}`:\n{api_go}"
        );
    }

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn go_multi_major_disambiguated_deps_canonical_owner_reexports_and_worlds() {
    let base = scratch_dir("multi-major-deps");
    let wit = base.join("wit");
    let module = "example.com/acme/app/gen/go";

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
                   variant choice {{ empty, code(u8) }}\n\
                   record tag {{\n\
                     id: u8,{dep_extra}\n\
                   }}\n\
                   ping: func(t: tag) -> status;\n\
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
                   use acme:shared/core@0.1.0.{{tag, status, choice}};\n\
                   record env {{ id: id, tag: tag, status: status, choice: choice }}\n\
                   run: func(e: env) -> env;\n\
                 }}\n\
                 world svc {{\n\
                   import acme:common/ids@0.1.0;\n\
                   export api;\n\
                 }}\n"
            ),
        )
        .unwrap();
        lock_version_first(&dir);
    }

    let files = go::plan(&wit, module).unwrap();

    // Shared dep stays un-scoped; differing dep is scoped per major with v1 re-exporting v0.
    assert!(
        files
            .files()
            .contains_key("witforge_deps/acme_common/ids/types.go")
    );
    assert!(
        files
            .files()
            .contains_key("witforge_deps/v0/acme_shared/core/types.go")
    );
    assert!(
        files
            .files()
            .contains_key("witforge_deps/v1/acme_shared/core/types.go")
    );
    assert!(
        files
            .files()
            .contains_key("witforge_deps/v2/acme_shared/core/types.go")
    );

    let v1_reexport = &files.files()["witforge_deps/v1/acme_shared/core/types.go"];
    for needle in [
        "import wf_owner \"example.com/acme/app/gen/go/witforge_deps/v0/acme_shared/core\"",
        "type Status = wf_owner.Status",
        "StatusActive   = wf_owner.StatusActive",
        "StatusArchived = wf_owner.StatusArchived",
        "type Choice = wf_owner.Choice",
        "type ChoiceEmpty = wf_owner.ChoiceEmpty",
        "type ChoiceCode = wf_owner.ChoiceCode",
        "type Tag = wf_owner.Tag",
        "type Core = wf_owner.Core",
    ] {
        assert!(
            v1_reexport.contains(needle),
            "v1 canonical-owner re-export missing `{needle}`:\n{v1_reexport}"
        );
    }

    let v2_dep = &files.files()["witforge_deps/v2/acme_shared/core/types.go"];
    assert!(v2_dep.contains("Extra bool"), "{v2_dep}");

    // Worlds metadata package covers all 3 live versions.
    let worlds_go = &files.files()["worlds/worlds.go"];
    for needle in [
        "package worlds",
        "type World struct {",
        "var V0_1 = []World{",
        "var V1_0 = []World{",
        "var V2_0 = []World{",
        "Name:    \"svc\",",
        "Imports: []string{\"acme:common/ids\", \"acme:shared/core\"},",
        "Exports: []string{\"api\"},",
        "var ByVersion = map[string][]World{",
        "\"v0_1\": V0_1,",
        "\"v1_0\": V1_0,",
        "\"v2_0\": V2_0,",
    ] {
        assert!(
            worlds_go.contains(needle),
            "worlds/worlds.go missing `{needle}`:\n{worlds_go}"
        );
    }

    // Retiring `v0.1` promotes `v1` to canonical owner.
    let released = wit.join("released");
    fs::write(&released, "v0.1 retired\nv1.0\n").unwrap();
    let live = go::plan_with_released(&wit, module, Some(&released)).unwrap();
    assert!(!live.files().contains_key("v0_1/api/types.go"));
    assert!(live.files().contains_key("v1_0/api/types.go"));
    assert!(
        !live
            .files()
            .contains_key("witforge_deps/v0/acme_shared/core/types.go")
    );
    let v1_promoted = &live.files()["witforge_deps/v1/acme_shared/core/types.go"];
    assert!(
        v1_promoted.contains("type Tag struct {"),
        "promoted v1 dep must emit full definitions:\n{v1_promoted}"
    );
    assert!(!v1_promoted.contains("wf_owner"), "{v1_promoted}");

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn go_fixtures_generate_check_prune_ownership_edge_names_and_deprecations() {
    let module = "example.com/witforge/fixtures";
    for rel in [
        "versions/wit",
        "deps-full/wit",
        "deps-world/wit",
        "deps-convert/wit",
    ] {
        let planned = go::plan(&fixture(rel), module).unwrap();
        assert!(!planned.files().is_empty(), "{rel}");
        assert!(
            planned.files().contains_key("witforge_types/types.go"),
            "{rel}"
        );
    }

    let base = scratch_dir("guards-and-edges");
    let wit = fixture("versions/wit");
    let out = base.join("go-out");

    // 1. generate_with_options + check_with_options + stale owned subdir pruning.
    go::generate_with_options(&wit, &out, module, &GoOptions::new()).unwrap();
    assert!(
        go::check_with_options(&wit, &out, module, &GoOptions::new())
            .unwrap()
            .is_empty()
    );

    let header = HeaderStyle::GoGenerated.format_header("go", "example", "evolve");
    let stale_dir = out.join("v99_0/old");
    fs::create_dir_all(&stale_dir).unwrap();
    fs::write(
        stale_dir.join("types.go"),
        format!("{header}\npackage old\n"),
    )
    .unwrap();
    let drift = go::check(&wit, &out, module).unwrap();
    assert_eq!(drift.len(), 1);
    assert_eq!(drift[0].status, GeneratedFileStatus::Extra);
    go::generate(&wit, &out, module).unwrap();
    assert!(
        !out.join("v99_0").exists(),
        "stale owned subdir must be pruned"
    );

    // 2. Foreign header (`rust` in `go` output root) is refused.
    let foreign_header = HeaderStyle::LineComment.format_header("rust", "example", "evolve");
    fs::write(out.join("foreign.go"), format!("{foreign_header}\n")).unwrap();
    let foreign_err = go::generate(&wit, &out, module).unwrap_err();
    assert!(
        format!("{foreign_err:#}").contains("generator `rust`"),
        "{foreign_err:#}"
    );
    fs::remove_file(out.join("foreign.go")).unwrap();

    // 3. Unowned file colliding with expected path is refused and never overwritten.
    let shared_path = out.join("witforge_types/types.go");
    fs::write(&shared_path, "// hand-written\n").unwrap();
    let unowned_check = go::check(&wit, &out, module).unwrap();
    assert!(
        unowned_check
            .iter()
            .any(|f| f.status == GeneratedFileStatus::Unowned),
        "{unowned_check:?}"
    );
    assert!(go::generate(&wit, &out, module).is_err());
    assert_eq!(
        fs::read_to_string(&shared_path).unwrap(),
        "// hand-written\n"
    );

    // 4. Invalid Go module paths are refused as operational (non-ContractError) errors.
    for bad in [
        "",
        "/abs/mod",
        "mod/",
        "a//b",
        "../rel",
        "example.com/pkg;alias",
        "example.com/has space",
    ] {
        let err = go::plan(&wit, bad).unwrap_err();
        assert!(
            !is_contract_error(&err),
            "`{bad}` should not be ContractError: {err:#}"
        );
    }

    // 5. Go special directory names (`internal`, `testdata`, `vendor`, `con`, `my-test`),
    //    GOOS/GOARCH suffixes (`api-linux`, `api-amd64`), keywords/predeclared params,
    //    variant/enum suffix collision disambiguation, triple options, empty types, and
    //    Go `// Deprecated:` doc comments.
    let edge_wit = base.join("edge-wit");
    let edge_v01 = edge_wit.join("v0.1");
    fs::create_dir_all(&edge_v01).unwrap();
    fs::write(
        edge_v01.join("contract.wit"),
        "package acme:edge@0.1.0;\n\
         interface empty {}\n\
         interface con { record a { id: u8 } }\n\
         interface internal { record b { id: u8 } }\n\
         interface testdata { record c { id: u8 } }\n\
         interface vendor { record d { id: u8 } }\n\
         interface my-test { record e { id: u8 } }\n\
         interface api-linux {\n\
           use con.{a};\n\
           use internal.{b};\n\
           use testdata.{c};\n\
           use vendor.{d};\n\
           use my-test.{e};\n\
           record foo-bar { n: u8 }\n\
           variant foo { bar(u16), unit }\n\
           record baz-qux { n: u8 }\n\
           enum baz { qux, other }\n\
           /// Multi-line block doc.\n\
           /// Second doc line.\n\
           @since(version = 0.1.0)\n\
           @deprecated(version = 0.1.0)\n\
           record holder {\n\
             a: a,\n\
             b: b,\n\
             c: c,\n\
             d: d,\n\
             e: e,\n\
             triple: option<option<option<u8>>>,\n\
           }\n\
           call: func(%type: string, %string: u32, %error: bool, len: u8, h: holder) -> option<holder>;\n\
         }\n",
    )
    .unwrap();
    lock_version_first(&edge_v01);

    let edge_files = go::plan(&edge_wit, "example.com/acme/edge").unwrap();
    for expected in [
        "v0_1/empty/types.go",
        "v0_1/con_/types.go",
        "v0_1/internal_/types.go",
        "v0_1/testdata_/types.go",
        "v0_1/vendor_/types.go",
        "v0_1/my_test_/types.go",
        "v0_1/api_linux/types.go",
    ] {
        assert!(
            edge_files.files().contains_key(expected),
            "missing `{expected}`; got: {:?}",
            edge_files.files().keys().collect::<Vec<_>>()
        );
    }

    let empty_go = &edge_files.files()["v0_1/empty/types.go"];
    assert!(!empty_go.contains("import"), "{empty_go}");

    let linux_go = &edge_files.files()["v0_1/api_linux/types.go"];
    for needle in [
        "package api_linux",
        "type Foo_Bar struct {",
        "type FooUnit struct{}",
        "Baz_Qux  Baz = 1",
        "BazOther Baz = 2",
        "// Multi-line block doc.",
        "// Stable since WIT version 0.1.0.",
        "// Deprecated: Deprecated in WIT version 0.1.0.",
        "Triple wf_types.Option[wf_types.Option[*uint8]]",
        "Call(type_ string, string_ uint32, error_ bool, len_ uint8, h Holder) *Holder",
    ] {
        assert!(
            linux_go.contains(needle),
            "v0_1/api_linux/types.go missing `{needle}`:\n{linux_go}"
        );
    }

    fs::remove_dir_all(&base).unwrap();
}
