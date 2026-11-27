//! Tier 1 offline + strict `tsc --noEmit` verification tests for `witforge::emit::typescript`
//! (`W13-002`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::emit::output::{GeneratedFileStatus, HeaderStyle};
use witforge::emit::typescript::{self, TypeScriptOptions};
use witforge::{conformance, is_contract_error, load, model};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("witforge-ts-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn lock_version_dir(dir: &Path, against: Option<&Path>) {
    let baseline = match against {
        Some(prev) => witforge::workflow::Baseline::Against(prev.to_path_buf()),
        None => witforge::workflow::Baseline::First,
    };
    let report = witforge::workflow::lock(
        &witforge::workflow::VersionOptions::new(dir.to_path_buf()).with_baseline(baseline),
    )
    .unwrap();
    assert!(
        report.is_ok(),
        "lock failed for {}: {report:?}",
        dir.display()
    );
    report.write().unwrap();
}

fn find_tsc() -> Option<PathBuf> {
    let local = repo_root().join("site/node_modules/.bin/tsc");
    if local.is_file() {
        return Some(local);
    }
    let path_tsc = PathBuf::from("tsc");
    Command::new(&path_tsc)
        .arg("--version")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|_| path_tsc)
}

#[allow(clippy::too_many_lines)]
fn write_all_constructs_contract(wit_root: &Path) {
    let v01 = wit_root.join("v0.1");
    let dep = v01.join("deps/dep-shared");
    fs::create_dir_all(&dep).unwrap();
    fs::write(
        dep.join("shared.wit"),
        r"package acme:shared@0.1.0;

/// Shared dependency types.
interface common {
  record tag {
    key: string,
    code: u16,
  }

  enum level {
    low,
    high,
  }
}
",
    )
    .unwrap();

    fs::write(
        v01.join("contract.wit"),
        r"package acme:showcase@0.1.0;

/// Core types exercising every supported WIT construct and TypeScript edge case.
@since(version = 0.1.0)
interface types {
  use acme:shared/common@0.1.0.{tag, level};

  type byte = u8;
  type id-list = list<u64>;
  type fixed-bytes = list<byte, 4>;
  type fixed-nums = list<u16, 3>;
  type maybe-u16 = option<u16>;
  type nested-maybe = option<option<u16>>;
  type lookup = map<string, u32>;
  type outcome = result<u16, string>;
  type ok-only = result<u8>;
  type err-only = result<_, color>;
  type bare-res = result;

  /// Color enum with docs containing `*/` delimiter safety test.
  @since(version = 0.1.0)
  @deprecated(version = 0.1.0)
  enum color {
    /// Warm red (`a */ b`).
    red,
    green,
    blue,
  }

  flags perms {
    read,
    write,
    default,
  }

  /// Record with every primitive, container, keyword field, and foreign type.
  record kitchen-sink {
    a-bool: bool,
    a-u8: u8,
    a-u16: u16,
    a-u32: u32,
    a-u64: u64,
    a-s8: s8,
    a-s16: s16,
    a-s32: s32,
    a-s64: s64,
    a-f32: f32,
    a-f64: f64,
    a-char: char,
    a-string: string,
    raw-bytes: list<u8>,
    alias-bytes: list<byte>,
    fixed-raw: list<u8, 4>,
    fixed-nums: list<u16, 3>,
    nested-bytes: list<list<u8>>,
    nested-nums: list<list<u16>>,
    nested-results: list<result<u8, color>>,
    opt-u8: option<u8>,
    nested-opt: option<option<u16>>,
    str-map: map<string, u8>,
    u64-map: map<u64, s64>,
    char-map: map<char, color>,
    nested-maps: list<map<u8, u16>>,
    both-res: result<u16, string>,
    only-ok: result<u8>,
    only-err: result<_, color>,
    bare: result,
    foreign-tag: tag,
    foreign-level: level,
    %type: string,
    %class: u32,
    %default: bool,
    %import: perms,
  }

  variant choice {
    /// Unit variant case.
    unit-case,
    small(u8),
    nested(kitchen-sink),
    fixed(list<u16, 2>),
  }
}

/// Service interface with functions, keyword parameters, and cross-interface types.
interface api {
  use types.{kitchen-sink, choice, color};
  use acme:shared/common@0.1.0.{tag};

  /// Transform input and return a result.
  @unstable(feature = preview-api)
  process: func(input: kitchen-sink, %default: choice, %class: tag) -> result<color, string>;

  /// Void-returning function.
  notify: func(label: string, %return: u8);
}

/// Empty interface emits `export {};` so isolatedModules treats it as a module.
interface empty {}

/// Interface whose name collides with directory barrel `index.ts`.
interface index {
  record entry {
    name: string,
  }
}

/// Interfaces whose names match Windows reserved device stems.
interface con {
  record conn {
    id: u32,
  }
}

interface nul {
  record nothing {
    flag: bool,
  }
}

/// Interface declaring types that shadow built-in `Uint8Array` and `ReadonlyMap`.
interface shadow {
  record uint8-array {
    len: u32,
  }

  record readonly-map {
    size: u32,
  }

  record carrier {
    real-bytes: list<u8>,
    real-map: map<string, u32>,
    local-bytes: uint8-array,
    local-map: readonly-map,
  }
}

world plugin {
  import acme:shared/common@0.1.0;
  import types;
  export api;
}
",
    )
    .unwrap();
    lock_version_dir(&v01, None);
}

fn write_three_major_dep_layout_contract(wit_root: &Path) {
    // v0.1 and v1.0 share identical `acme:evolved@0.1.0` (canonical owner = 0, so v1 re-exports
    // from v0!), while v2.0 upgrades `acme:evolved@0.2.0` (distinct!). `acme:const@0.1.0` is
    // identical across all three majors (shared at `witforge_deps/acme_const/`).
    let const_wit = r"package acme:const@0.1.0;
interface ids {
  record trace-id {
    value: u64,
  }
}
";
    let evolved_v0 = r"package acme:evolved@0.1.0;
interface meta {
  record envelope {
    label: string,
  }
}
";
    let evolved_v1 = r"package acme:evolved@0.2.0;
interface meta {
  record envelope {
    label: string,
    revision: u32,
  }
}
";

    for (ver_dir, pkg_ver, evolved_src) in [
        ("v0.1", "0.1.0", evolved_v0),
        ("v1.0", "1.0.0", evolved_v0),
        ("v2.0", "2.0.0", evolved_v1),
    ] {
        let dir = wit_root.join(ver_dir);
        fs::create_dir_all(dir.join("deps/acme-const")).unwrap();
        fs::create_dir_all(dir.join("deps/acme-evolved")).unwrap();
        fs::write(dir.join("deps/acme-const/const.wit"), const_wit).unwrap();
        fs::write(dir.join("deps/acme-evolved/evolved.wit"), evolved_src).unwrap();
        let dep_ver = if ver_dir == "v2.0" { "0.2.0" } else { "0.1.0" };
        fs::write(
            dir.join("contract.wit"),
            format!(
                "package acme:multi@{pkg_ver};\n\
                 interface api {{\n\
                   use acme:const/ids@0.1.0.{{trace-id}};\n\
                   use acme:evolved/meta@{dep_ver}.{{envelope}};\n\
                   record msg {{\n\
                     trace: trace-id,\n\
                     env: envelope,\n\
                   }}\n\
                 }}\n"
            ),
        )
        .unwrap();
        lock_version_dir(&dir, None);
    }
}

#[allow(clippy::too_many_lines)]
#[test]
fn emits_all_wit_constructs_deterministically_with_ownership_drift_and_pruning() {
    let base = scratch("all-constructs");
    let wit_root = base.join("wit");
    write_all_constructs_contract(&wit_root);

    let first = typescript::plan(&wit_root).unwrap();
    let second = typescript::plan(&wit_root).unwrap();
    assert_eq!(first, second, "TypeScript emission must be deterministic");
    assert!(first.is_nested());

    let expected_header = "// @generated by witforge (ts, acme:showcase); do not edit.";
    assert_eq!(first.header(), expected_header);
    for (rel_path, content) in first.files() {
        assert!(
            !content.contains('\r'),
            "{rel_path} must use LF line endings only"
        );
        assert!(
            content.starts_with(expected_header),
            "{rel_path} must start with ownership header"
        );
        assert!(
            !content.contains("\nexport enum "),
            "{rel_path} must not emit non-erasable TS enum declarations"
        );
    }

    // Verify expected files including Windows-reserved and `index` interface escapes.
    for path in [
        "index.ts",
        "witforge_types.ts",
        "worlds.ts",
        "v0_1/index.ts",
        "v0_1/types.ts",
        "v0_1/api.ts",
        "v0_1/empty.ts",
        "v0_1/index_.ts",
        "v0_1/con_.ts",
        "v0_1/nul_.ts",
        "v0_1/shadow.ts",
        "witforge_deps/index.ts",
        "witforge_deps/acme_shared/index.ts",
        "witforge_deps/acme_shared/common.ts",
    ] {
        assert!(
            first.files().contains_key(path),
            "missing expected TS file `{path}` in {:?}",
            first.files().keys().collect::<Vec<_>>()
        );
    }

    let types_ts = &first.files()["v0_1/types.ts"];
    assert!(types_ts.contains("import type { Option } from \"../witforge_types.js\";"));
    assert!(types_ts.contains("export const Color = {"));
    assert!(types_ts.contains("export type Color = (typeof Color)[keyof typeof Color];"));
    assert!(types_ts.contains("@deprecated Deprecated in WIT version 0.1.0."));
    assert!(types_ts.contains("Warm red (`a *\\/ b`)."));
    assert!(types_ts.contains("readonly aU64: bigint;"));
    assert!(types_ts.contains("readonly aS64: bigint;"));
    assert!(types_ts.contains("readonly rawBytes: Uint8Array;"));
    assert!(types_ts.contains("readonly aliasBytes: Uint8Array;"));
    assert!(types_ts.contains("readonly fixedRaw: Uint8Array;"));
    assert!(types_ts.contains("readonly nestedBytes: readonly Uint8Array[];"));
    assert!(types_ts.contains("readonly nestedNums: readonly (readonly number[])[];"));
    assert!(types_ts.contains("readonly nestedOpt: Option<Option<number>>;"));
    assert!(types_ts.contains("readonly strMap: ReadonlyMap<string, number>;"));
    assert!(types_ts.contains("readonly type_: string;"));
    assert!(types_ts.contains("readonly class_: number;"));
    assert!(types_ts.contains("readonly default_: boolean;"));
    assert!(types_ts.contains("readonly import_: Perms;"));

    let shadow_ts = &first.files()["v0_1/shadow.ts"];
    assert!(shadow_ts.contains("_WitforgeBytes"));
    assert!(shadow_ts.contains("_WitforgeReadonlyMap"));

    let empty_ts = &first.files()["v0_1/empty.ts"];
    assert!(empty_ts.contains("export {};"));

    let worlds_ts = &first.files()["worlds.ts"];
    assert!(worlds_ts.contains("export interface WorldMeta"));
    assert!(worlds_ts.contains("export const WORLDS = {"));
    assert!(worlds_ts.contains("\"acme:shared/common\""));

    // Verify emit_version matches plan's v0_1 files byte-for-byte.
    let single_model = model::build(&load(&wit_root.join("v0.1")).unwrap()).unwrap();
    let single_files = typescript::emit_version(&single_model).unwrap();
    for (file_name, content) in &single_files {
        assert_eq!(
            first.files().get(&format!("v0_1/{file_name}")),
            Some(content),
            "emit_version mismatch on {file_name}"
        );
    }

    // Write, check, tamper -> Changed + diff, and stale nested directory pruning.
    let out = base.join("gen-ts");
    typescript::generate(&wit_root, &out).unwrap();
    assert!(typescript::check(&wit_root, &out).unwrap().is_empty());

    let api_path = out.join("v0_1/api.ts");
    let orig = fs::read_to_string(&api_path).unwrap();
    fs::write(&api_path, format!("{orig}// drift\n")).unwrap();
    let findings = typescript::check(&wit_root, &out).unwrap();
    assert_eq!(findings.len(), 1);
    assert!(matches!(
        findings[0].status,
        GeneratedFileStatus::Changed { .. }
    ));
    let diff = first.unified_diff(&out).unwrap();
    assert!(diff.contains("-// drift"), "{diff}");

    // Stale owned file inside an obsolete subdirectory is pruned cleanly together with its dir.
    let stale_dir = out.join("v0_0");
    fs::create_dir_all(&stale_dir).unwrap();
    fs::write(
        stale_dir.join("old.ts"),
        format!("{expected_header}\nexport {{}};\n"),
    )
    .unwrap();
    typescript::generate(&wit_root, &out).unwrap();
    assert!(!stale_dir.exists(), "empty stale subdir should be pruned");
    assert!(typescript::check(&wit_root, &out).unwrap().is_empty());

    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn multi_version_retired_and_multi_major_dep_layout_disambiguation() {
    let base = scratch("multi-major");
    let wit_root = base.join("wit");
    write_three_major_dep_layout_contract(&wit_root);

    let planned = typescript::plan(&wit_root).unwrap();
    assert!(
        planned
            .files()
            .contains_key("witforge_deps/acme_const/ids.ts")
    );
    assert!(
        planned
            .files()
            .contains_key("witforge_deps/v0/acme_evolved/meta.ts")
    );
    assert!(
        planned
            .files()
            .contains_key("witforge_deps/v1/acme_evolved/meta.ts")
    );
    assert!(
        planned
            .files()
            .contains_key("witforge_deps/v2/acme_evolved/meta.ts")
    );

    // v1's scoped dep matches v0's canonical owner and re-exports directly from v0!
    let v1_meta = &planned.files()["witforge_deps/v1/acme_evolved/meta.ts"];
    assert!(
        v1_meta.contains("export * from \"../../v0/acme_evolved/meta.js\";"),
        "{v1_meta}"
    );
    let v2_meta = &planned.files()["witforge_deps/v2/acme_evolved/meta.ts"];
    assert!(v2_meta.contains("readonly revision: number;"), "{v2_meta}");

    // Retiring v0.1 via `--released` drops v0_1 and shifts canonical owner of v1 to v1 (no broken
    // v0 re-export!).
    let released = wit_root.join("released");
    fs::write(&released, "v0.1 retired\nv1.0\nv2.0\n").unwrap();
    let after_retire = typescript::plan_with_released(&wit_root, Some(&released)).unwrap();
    assert!(!after_retire.files().contains_key("v0_1/index.ts"));
    assert!(after_retire.files().contains_key("v1_0/index.ts"));
    let v1_meta_owned = &after_retire.files()["witforge_deps/v1/acme_evolved/meta.ts"];
    assert!(
        v1_meta_owned.contains("export interface Envelope"),
        "after retiring v0.1, v1 becomes canonical owner:\n{v1_meta_owned}"
    );

    // Existing workspace fixtures all plan cleanly without errors.
    for rel in ["versions/wit", "deps-full/wit", "deps-world/wit"] {
        let f = typescript::plan(&fixture(rel)).unwrap();
        assert!(f.files().contains_key("index.ts"), "{rel}");
    }

    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn collision_refusals_and_output_ownership_guards() {
    let base = scratch("errors");

    // 1. Type colliding with its own function-bearing interface name reports ContractError
    //    (constructed directly as a `Model` since `model::build` also checks Rust trait clashes).
    let collide_model = model::Model::new(
        model::Package::new(
            "acme".to_string(),
            "collide".to_string(),
            model::Version::new(0, 1, 0),
            String::new(),
        ),
        vec![model::Interface::new(
            "service".to_string(),
            String::new(),
            model::Stability::Unknown,
            vec![model::TypeDef::new(
                "service".to_string(),
                String::new(),
                model::Stability::Unknown,
                model::TypeDefKind::Record(vec![model::Field::new(
                    "id".to_string(),
                    String::new(),
                    model::TypeExpr::U32,
                )]),
            )],
            vec![model::Function::new(
                "ping".to_string(),
                String::new(),
                model::Stability::Unknown,
                Vec::new(),
                Some(model::TypeExpr::Named(model::QualifiedName::new(
                    "service".to_string(),
                    "service".to_string(),
                ))),
            )],
        )],
        Vec::new(),
        Vec::new(),
    );
    let err = typescript::emit_version(&collide_model).unwrap_err();
    assert!(
        is_contract_error(&err),
        "interface/type name collision must be ContractError: {err:#}"
    );
    assert!(
        err.to_string().contains("TypeScript name `Service`"),
        "{err:#}"
    );

    // 2. Foreign witforge output (Rust header) and unowned file at expected path are refused.
    let ok_root = base.join("ok");
    let ok_v01 = ok_root.join("v0.1");
    fs::create_dir_all(&ok_v01).unwrap();
    fs::write(
        ok_v01.join("contract.wit"),
        "package acme:ok@0.1.0;\ninterface api { record item { id: u32 } }\n",
    )
    .unwrap();
    lock_version_dir(&ok_v01, None);

    let out = base.join("out");
    fs::create_dir_all(out.join("v0_1")).unwrap();
    fs::write(
        out.join("v0_1/api.ts"),
        format!(
            "{}\n",
            HeaderStyle::LineComment.format_header("rust", "acme", "ok")
        ),
    )
    .unwrap();
    assert!(
        typescript::generate(&ok_root, &out).is_err(),
        "must refuse foreign Rust header in TypeScript output root"
    );

    fs::write(out.join("v0_1/api.ts"), "// hand-written\n").unwrap();
    let findings = typescript::check(&ok_root, &out).unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f.status == GeneratedFileStatus::Unowned),
        "{findings:?}"
    );
    assert!(
        typescript::generate(&ok_root, &out).is_err(),
        "must refuse overwriting unowned file"
    );

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn emitted_typescript_compiles_with_strict_tsc_nodenext() {
    let Some(tsc) = find_tsc() else {
        eprintln!("skipping tsc compile check: `tsc` not found");
        return;
    };

    let base = scratch("tsc-strict");
    let all_wit = base.join("all-wit");
    write_all_constructs_contract(&all_wit);
    let multi_wit = base.join("multi-wit");
    write_three_major_dep_layout_contract(&multi_wit);
    let conf_bundle = base.join("conformance");
    conformance::generate(&conf_bundle, &conformance::ConformanceOptions::new()).unwrap();

    let proj = base.join("ts-project");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("package.json"), "{\"type\": \"module\"}\n").unwrap();
    fs::write(
        proj.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "exactOptionalPropertyTypes": true,
    "verbatimModuleSyntax": true,
    "isolatedModules": true,
    "erasableSyntaxOnly": true,
    "noUnusedLocals": true,
    "noEmit": true
  },
  "include": ["gen/**/*.ts", "consumer_check.ts"]
}
"#,
    )
    .unwrap();

    for (name, wit_dir) in [
        ("all", all_wit.as_path()),
        ("multi", multi_wit.as_path()),
        ("conformance", conf_bundle.join("wit").as_path()),
        ("versions", fixture("versions/wit").as_path()),
        ("deps_full", fixture("deps-full/wit").as_path()),
        ("deps_world", fixture("deps-world/wit").as_path()),
    ] {
        typescript::generate_with_options(
            wit_dir,
            &proj.join("gen").join(name),
            &TypeScriptOptions::new(),
        )
        .unwrap();
    }

    // Consumer program exercising strict types, Option<Option<T>>, exhaustive switches,
    // canonical-owner dep identity across v0 and v1, worlds metadata, and keyword-escaped fields.
    fs::write(
        proj.join("consumer_check.ts"),
        r#"import { NONE, some, v0_1 as allV01, WORLDS, type Option, type WorldMeta } from "./gen/all/index.js";
import { v0_1 as multiV01, v1_0 as multiV10, v2_0 as multiV20 } from "./gen/multi/index.js";
import { v0_1 as confV01 } from "./gen/conformance/index.js";

// 1. Nested Option<Option<number>> distinguishes none, some(none), and some(some(42)).
const nestedNone: Option<Option<number>> = NONE;
const nestedSomeNone: Option<Option<number>> = some(NONE);
const nestedSomeSome: Option<Option<number>> = some(some(42));

export function describeNested(x: Option<Option<number>>): string {
  if (x.tag === "none") return "none";
  if (x.value.tag === "none") return "some(none)";
  return `some(some(${x.value.value}))`;
}

// 2. Enum const object + string literal union exhaustiveness.
export function describeColor(c: allV01.types.Color): number {
  switch (c) {
    case allV01.types.Color.Red:
      return 1;
    case allV01.types.Color.Green:
      return 2;
    case allV01.types.Color.Blue:
      return 3;
  }
}

// 3. Variant discriminated union exhaustiveness.
export function describeChoice(c: allV01.types.Choice): string {
  switch (c.tag) {
    case "unit-case":
      return "unit";
    case "small":
      return String(c.value);
    case "nested":
      return `${c.value.type_}:${c.value.aU64}`;
    case "fixed":
      return String(c.value.length);
  }
}

// 4. All four Result shapes compile cleanly.
export const sampleResults: confV01.types.Results = {
  both: { tag: "ok", value: 1 },
  onlyOk: { tag: "err" },
  onlyErr: { tag: "ok" },
  bare: { tag: "err" },
};

// 5. Canonical-owner dep re-export shares type identity across v0_1 and v1_0, while v2_0 has
//    its own upgraded Envelope with `revision`.
export function forwardV0ToV1(m: multiV01.api.Msg): multiV10.api.Msg {
  return { trace: m.trace, env: m.env };
}

export function upgradeToV2(m: multiV10.api.Msg): multiV20.api.Msg {
  return { trace: m.trace, env: { label: m.env.label, revision: 2 } };
}

// 6. Shadowed Uint8Array and ReadonlyMap interface compiles cleanly.
export const shadowSample: allV01.shadow.Carrier = {
  realBytes: new Uint8Array([1, 2, 3]),
  realMap: new Map([["a", 1]]),
  localBytes: { len: 3 },
  localMap: { size: 1 },
};

// 7. World metadata satisfies WorldMeta and preserves literal names.
export const firstWorld: WorldMeta = WORLDS.v0_1[0];
export const worldName: "plugin" = WORLDS.v0_1[0].name;

// Prevent unused locals.
export const checkedValues = [
  describeNested(nestedNone),
  describeNested(nestedSomeNone),
  describeNested(nestedSomeSome),
  describeColor(allV01.types.Color.Blue),
];
"#,
    )
    .unwrap();

    let output = Command::new(&tsc)
        .args(["--project", proj.join("tsconfig.json").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "tsc --noEmit failed on generated TypeScript:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    fs::remove_dir_all(&base).unwrap();
}
