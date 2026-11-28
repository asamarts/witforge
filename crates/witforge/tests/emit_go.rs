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

    // 5. Go special directory/package names (`internal`, `testdata`, `vendor`, `con`, `my-test`,
    //    `main`, `init`, `documentation`), GOOS/GOARCH suffixes (`api-linux`), keyword/predeclared
    //    params, variant/enum suffix collision disambiguation, resolved aliases, named structural
    //    types, `+build`-safe doc comments, and Go `// Deprecated:` doc comments.
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
         interface main { record m { id: u8 } }\n\
         interface init { record i { id: u8 } }\n\
         interface documentation { record doc-entry { title: string } }\n\
         interface api-linux {\n\
           use con.{a as renamed-a};\n\
           use internal.{b};\n\
           use testdata.{c};\n\
           use vendor.{d};\n\
           use my-test.{e};\n\
           use main.{m};\n\
           use init.{i};\n\
           use documentation.{doc-entry};\n\
           type byte-alias = u8;\n\
           type byte-slice = list<byte-alias>;\n\
           type fixed-byte-arr = list<byte-alias, 4>;\n\
           type opt-u32 = option<u32>;\n\
           type nested-via-alias = option<opt-u32>;\n\
           type named-list = list<u16>;\n\
           type named-map = map<string, u32>;\n\
           record foo-bar { n: u8 }\n\
           variant foo { bar(u16), unit }\n\
           record baz-qux { n: u8 }\n\
           enum baz { qux, other }\n\
           ///   Multi-line block doc.\n\
           ///\n\
           ///\n\
           ///   +build ignore\n\
           ///     indented_snippet()\n\
           @since(version = 0.1.0)\n\
           @deprecated(version = 0.1.0)\n\
           record holder {\n\
             a: renamed-a,\n\
             b: b,\n\
             c: c,\n\
             d: d,\n\
             e: e,\n\
             m: m,\n\
             i: i,\n\
             doc-entry: doc-entry,\n\
             triple: option<option<option<u8>>>,\n\
             opt-list: option<list<u16>>,\n\
             opt-map: option<map<string, u8>>,\n\
             opt-var: option<foo>,\n\
             bs: byte-slice,\n\
             fb: fixed-byte-arr,\n\
             nva: nested-via-alias,\n\
             nl: named-list,\n\
             nm: named-map,\n\
           }\n\
           call: func(%type: string, %string: u32, %error: bool, len: u8, h: holder) -> option<holder>;\n\
         }\n",
    )
    .unwrap();
    lock_version_first(&edge_v01);

    let edge_files = go::plan(&edge_wit, "example.com/acme/edge").unwrap();
    for expected in [
        "doc.go",
        "v0_1/empty/types.go",
        "v0_1/con_/types.go",
        "v0_1/internal_/types.go",
        "v0_1/testdata_/types.go",
        "v0_1/vendor_/types.go",
        "v0_1/my_test_/types.go",
        "v0_1/main_/types.go",
        "v0_1/init_/types.go",
        "v0_1/documentation_/types.go",
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
        "type RenamedA = wf_if_con_.A",
        "type ByteAlias = uint8",
        "type ByteSlice = []byte",
        "type FixedByteArr = [4]byte",
        "type OptU32 = *uint32",
        "type NestedViaAlias = wf_types.Option[OptU32]",
        "type NamedList = []uint16",
        "type NamedMap = map[string]uint32",
        "type Foo_Bar struct {",
        "type FooUnit struct{}",
        "Baz_Qux  Baz = 1",
        "BazOther Baz = 2",
        "// Multi-line block doc.",
        "// \\+build ignore",
        "// indented_snippet()",
        "// Stable since WIT version 0.1.0.",
        "// Deprecated: Deprecated in WIT version 0.1.0.",
        "Triple   wf_types.Option[wf_types.Option[*uint8]]",
        "OptList  *[]uint16",
        "OptMap   *map[string]uint8",
        "OptVar   *Foo",
        "Call(type_ string, string_ uint32, error_ bool, len_ uint8, h Holder) *Holder",
    ] {
        assert!(
            linux_go.contains(needle),
            "v0_1/api_linux/types.go missing `{needle}`:\n{linux_go}"
        );
    }
    assert!(
        !linux_go.contains("// +build"),
        "raw `+build` directive must never appear in emitted Go:\n{linux_go}"
    );
    assert!(
        !linux_go.contains("//\n//\n"),
        "consecutive blank doc lines must be collapsed:\n{linux_go}"
    );

    fs::remove_dir_all(&base).unwrap();
}

/// Detect whether `gofmt` and Go 1.22+ are available on `PATH` for live Tier 1+ verification.
fn go_toolchain_available() -> bool {
    let gofmt_ok = std::process::Command::new("gofmt")
        .arg("-h")
        .output()
        .is_ok();
    if !gofmt_ok {
        return false;
    }
    let Ok(out) = std::process::Command::new("go").arg("version").output() else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // Parse `go version go<major>.<minor>...`
    let Some(ver) = text
        .split_whitespace()
        .find_map(|tok| tok.strip_prefix("go"))
    else {
        return false;
    };
    let mut parts = ver.split('.');
    let major: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor: u32 = parts
        .next()
        .map(|s| {
            s.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    major > 1 || (major == 1 && minor >= 22)
}

fn verify_live_go_tree(work_root: &Path, module: &str, out: &Path, extra_test: Option<&str>) {
    let diff = std::process::Command::new("gofmt")
        .args(["-d", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(diff.status.success(), "gofmt failed: {diff:?}");
    assert!(
        diff.stdout.is_empty(),
        "gofmt diff on `{}`:\n{}",
        out.display(),
        String::from_utf8_lossy(&diff.stdout)
    );

    let build_dir = work_root.join(format!(
        "go-verify-{}",
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    copy_dir_recursive(out, &build_dir);
    fs::write(
        build_dir.join("go.mod"),
        format!("module {module}\n\ngo 1.22\n"),
    )
    .unwrap();
    if let Some(test_src) = extra_test {
        fs::write(build_dir.join("verify_test.go"), test_src).unwrap();
    }

    let gopath = work_root.join("gopath");
    let gocache = work_root.join("gocache");
    let gomodcache = work_root.join("gomodcache");
    for args in [
        vec!["build", "./..."],
        vec!["vet", "./..."],
        vec!["test", "./..."],
    ] {
        let status = std::process::Command::new("go")
            .args(&args)
            .current_dir(&build_dir)
            .env("GOPATH", &gopath)
            .env("GOCACHE", &gocache)
            .env("GOMODCACHE", &gomodcache)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "`go {}` failed in {}:\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            build_dir.display(),
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

#[allow(clippy::too_many_lines)]
#[test]
fn go_standalone_emit_version_transitive_deps_worlds_collisions_and_live_toolchain() {
    let base = scratch_dir("advanced-and-live");

    // 1. Standalone `emit_version` / `emit_version_with_options`, `ensure_outside_wit_root`,
    //    and invalid module path error classification (`WF9001`, non-`ContractError`).
    let conf_bundle = base.join("conf");
    witforge::conformance::generate(
        &conf_bundle,
        &witforge::conformance::ConformanceOptions::new(),
    )
    .unwrap();
    let conf_wit = conf_bundle.join("wit");
    let v01_model =
        witforge::model::build(&witforge::load(&conf_wit.join("v0.1")).unwrap()).unwrap();
    let single = go::emit_version(&v01_model, "example.com/witforge/conf").unwrap();
    assert_eq!(
        single,
        go::emit_version_with_options(&v01_model, "example.com/witforge/conf", &GoOptions::new())
            .unwrap()
    );
    assert!(single.contains_key("types/types.go"));
    assert!(single.contains_key("api/types.go"));
    assert!(!single.contains_key("doc.go"));
    assert!(single["types/types.go"].contains("example.com/witforge/conf/witforge_types"));
    let bad_mod_err = go::emit_version(&v01_model, "example.com/con/pkg").unwrap_err();
    assert!(!is_contract_error(&bad_mod_err), "{bad_mod_err:#}");
    assert!(
        witforge::error_diagnostics(&bad_mod_err)
            .unwrap_or_default()
            .iter()
            .any(|d| d.code == witforge::diagnostic::codes::CLI_USAGE),
        "{bad_mod_err:#}"
    );
    assert!(
        go::generate(
            &conf_wit,
            &conf_wit.join("inside"),
            "example.com/witforge/conf"
        )
        .is_err()
    );
    assert!(
        go::check(
            &conf_wit,
            &conf_wit.join("inside"),
            "example.com/witforge/conf"
        )
        .is_err()
    );

    // 2. Transitive multi-major deps (`dep-b -> dep-a`), alias collision `_2` (`a:b/c-d` vs
    //    `a:b-c/d`), mixed empty/non-empty worlds with differing key lengths (`v0_1`, `v10_0`,
    //    `v100_0`) and deprecated world, plus `main`/`init`/`documentation` interfaces and
    //    `+build` doc comments.
    let wit = base.join("complex-wit");
    for (ver_dir, pkg_ver, dep_b_ver, b_extra, has_world) in [
        ("v0.1", "0.1.0", "0.1.0", "", true),
        ("v10.0", "10.0.0", "10.0.0", "extra: bool,", false),
        ("v100.0", "100.0.0", "0.1.0", "", true),
    ] {
        let dir = wit.join(ver_dir);
        let dep_a = dir.join("deps/dep-a");
        let dep_b = dir.join("deps/dep-b");
        let dep_c1 = dir.join("deps/a-b");
        let dep_c2 = dir.join("deps/a-bc");
        for d in [&dep_a, &dep_b, &dep_c1, &dep_c2] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(
            dep_a.join("a.wit"),
            "package dep:a@0.1.0;\ninterface base { record id { raw: u64 } }\n",
        )
        .unwrap();
        fs::write(
            dep_b.join("b.wit"),
            format!(
                "package dep:b@{dep_b_ver};\n\
                 interface wrap {{\n\
                   use dep:a/base@0.1.0.{{id}};\n\
                   enum state {{ ok, fail }}\n\
                   record box {{ id: id, state: state, {b_extra} }}\n\
                 }}\n"
            ),
        )
        .unwrap();
        fs::write(
            dep_c1.join("c1.wit"),
            "package a:b@0.1.0;\ninterface c-d { record left { v: u8 } }\n",
        )
        .unwrap();
        fs::write(
            dep_c2.join("c2.wit"),
            "package a:b-c@0.1.0;\ninterface d { record right { v: u8 } }\n",
        )
        .unwrap();
        let world_decl = if has_world {
            format!(
                "/// World doc.\n\
                 /// +build ignore\n\
                 @since(version = {pkg_ver})\n\
                 @deprecated(version = {pkg_ver})\n\
                 world svc {{\n\
                   import dep:b/wrap@{dep_b_ver};\n\
                   export api;\n\
                 }}\n"
            )
        } else {
            String::new()
        };
        fs::write(
            dir.join("contract.wit"),
            format!(
                "package acme:complex@{pkg_ver};\n\
                 interface main {{ record entry {{ v: u32 }} }}\n\
                 interface init {{ record seed {{ v: u32 }} }}\n\
                 interface documentation {{ record note {{ text: string }} }}\n\
                 interface api {{\n\
                   use dep:b/wrap@{dep_b_ver}.{{box, state}};\n\
                   use a:b/c-d@0.1.0.{{left}};\n\
                   use a:b-c/d@0.1.0.{{right}};\n\
                   use main.{{entry}};\n\
                   use init.{{seed}};\n\
                   use documentation.{{note}};\n\
                   /// +build linux\n\
                   record payload {{\n\
                     b: box,\n\
                     s: state,\n\
                     l: left,\n\
                     r: right,\n\
                     e: entry,\n\
                     seed: seed,\n\
                     n: note,\n\
                   }}\n\
                 }}\n\
                 {world_decl}"
            ),
        )
        .unwrap();
        lock_version_first(&dir);
    }

    let complex_out = base.join("complex-go");
    let complex_mod = "example.com/acme/main/v2";
    go::generate(&wit, &complex_out, complex_mod).unwrap();
    assert_eq!(go::check(&wit, &complex_out, complex_mod).unwrap(), []);

    let complex_plan = go::plan(&wit, complex_mod).unwrap();
    // Root `doc.go` escapes `main` -> `package main_`.
    assert!(complex_plan.files()["doc.go"].contains("package main_\n"));
    // `a:b/c-d` and `a:b-c/d` both want `wf_dep_a_b_c_d`; second gets `_2`.
    let api_v01 = &complex_plan.files()["v0_1/api/types.go"];
    assert!(api_v01.contains("wf_dep_a_b_c_d "), "{api_v01}");
    assert!(api_v01.contains("wf_dep_a_b_c_d_2 "), "{api_v01}");
    // Canonical owner re-export for `v100` pointing at `v0`.
    let v100_dep = &complex_plan.files()["witforge_deps/v100/dep_b/wrap/types.go"];
    assert!(v100_dep.contains("type Box = wf_owner.Box"), "{v100_dep}");
    // Worlds alignment across `v0_1`, `v10_0`, `v100_0` + deprecated world + empty V10_0.
    let worlds_go = &complex_plan.files()["worlds/worlds.go"];
    assert!(worlds_go.contains("var V10_0 = []World{}"), "{worlds_go}");
    assert!(worlds_go.contains("\"v0_1\":   V0_1,"), "{worlds_go}");
    assert!(worlds_go.contains("\"v10_0\":  V10_0,"), "{worlds_go}");
    assert!(worlds_go.contains("\"v100_0\": V100_0,"), "{worlds_go}");
    assert!(worlds_go.contains("// \\+build ignore"), "{worlds_go}");
    assert!(worlds_go.contains("// Deprecated:"), "{worlds_go}");

    // 3. Cross-major dependency directory collision (`v0.1` uses `my-ns:dep`, `v1.0` uses
    //    `my:ns-dep`) is reported as a `ContractError` when merging `witforge_deps/`.
    let coll_wit = base.join("dep-coll-wit");
    let coll_v01 = coll_wit.join("v0.1");
    let coll_v10 = coll_wit.join("v1.0");
    fs::create_dir_all(coll_v01.join("deps/d1")).unwrap();
    fs::create_dir_all(coll_v10.join("deps/d2")).unwrap();
    fs::write(
        coll_v01.join("deps/d1/d.wit"),
        "package my-ns:dep@0.1.0;\ninterface a { record x { v: u8 } }\n",
    )
    .unwrap();
    fs::write(
        coll_v01.join("contract.wit"),
        "package acme:coll@0.1.0;\n\
         interface api {\n\
           use my-ns:dep/a@0.1.0.{x};\n\
           record pair { x: x }\n\
         }\n",
    )
    .unwrap();
    lock_version_first(&coll_v01);
    fs::write(
        coll_v10.join("deps/d2/d.wit"),
        "package my:ns-dep@0.1.0;\ninterface b { record y { v: u8 } }\n",
    )
    .unwrap();
    fs::write(
        coll_v10.join("contract.wit"),
        "package acme:coll@1.0.0;\n\
         interface api {\n\
           use my:ns-dep/b@0.1.0.{y};\n\
           record pair { y: y }\n\
         }\n",
    )
    .unwrap();
    lock_version_first(&coll_v10);
    let coll_err = go::plan(&coll_wit, "example.com/acme/coll").unwrap_err();
    assert!(is_contract_error(&coll_err), "{coll_err:#}");

    // 4. Live Go 1.22+ toolchain verification (`gofmt -d`, `go build`, `go vet`, `go test`)
    //    when `go` >= 1.22 and `gofmt` are installed on PATH.
    if go_toolchain_available() {
        let conf_go = base.join("conf-go");
        go::generate(&conf_wit, &conf_go, "example.com/witforge/conf").unwrap();
        verify_live_go_tree(
            &base,
            "example.com/witforge/conf",
            &conf_go,
            Some(
                "package conf_test\n\n\
                 import (\n\
                 \t\"testing\"\n\
                 \t\"example.com/witforge/conf/v0_1/types\"\n\
                 \t\"example.com/witforge/conf/witforge_types\"\n\
                 )\n\n\
                 func TestConformancePlainGo(t *testing.T) {\n\
                 \tvar zero types.Color\n\
                 \tif zero.Valid() || zero.String() != \"\" {\n\
                 \t\tt.Fatalf(\"zero color must be invalid\")\n\
                 \t}\n\
                 \tif !types.ColorRed.Valid() || types.ColorRed.String() != \"red\" {\n\
                 \t\tt.Fatalf(\"ColorRed mismatch\")\n\
                 \t}\n\
                 \tvar nilPtr *uint16\n\
                 \tsomeNil := witforge_types.Some(nilPtr)\n\
                 \tif !someNil.Some || someNil.Value != nil {\n\
                 \t\tt.Fatalf(\"Some(nil) collapsed\")\n\
                 \t}\n\
                 \tvar ch types.Choice = types.ChoiceFixed{Value: [2]uint16{1, 2}}\n\
                 \tif _, ok := ch.(types.ChoiceFixed); !ok {\n\
                 \t\tt.Fatalf(\"ChoiceFixed mismatch\")\n\
                 \t}\n\
                 }\n",
            ),
        );

        verify_live_go_tree(
            &base,
            complex_mod,
            &complex_out,
            Some(
                "package main__test\n\n\
                 import (\n\
                 \t\"testing\"\n\
                 \tv0wrap \"example.com/acme/main/v2/witforge_deps/v0/dep_b/wrap\"\n\
                 \tv100wrap \"example.com/acme/main/v2/witforge_deps/v100/dep_b/wrap\"\n\
                 \t\"example.com/acme/main/v2/worlds\"\n\
                 )\n\n\
                 func TestComplexEdgeGo(t *testing.T) {\n\
                 \tvar a v0wrap.Box = v100wrap.Box{State: v100wrap.StateOk}\n\
                 \tif !a.State.Valid() || a.State.String() != \"ok\" {\n\
                 \t\tt.Fatalf(\"canonical alias mismatch: %+v\", a)\n\
                 \t}\n\
                 \tif len(worlds.V10_0) != 0 || len(worlds.ByVersion[\"v100_0\"]) != 1 {\n\
                 \t\tt.Fatalf(\"worlds mismatch\")\n\
                 \t}\n\
                 }\n",
            ),
        );
    }

    fs::remove_dir_all(&base).unwrap();
}
