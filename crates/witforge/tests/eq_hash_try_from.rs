//! End-to-end compile, formatting, Clippy, `no_std`, and runtime tests for opt-in float-free
//! `Eq`/`Hash` derives and `convert.rs` `TryFrom` impls (`W11-045`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::emit::rust::RustOptions;
use witforge::workflow::{Baseline, VersionOptions, lock};

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("witforge-w11045-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn lock_version(dir: &Path, against: Option<&Path>) {
    let baseline = match against {
        Some(prev) => Baseline::Against(prev.to_path_buf()),
        None => Baseline::First,
    };
    let opts = VersionOptions::new(dir.to_path_buf()).with_baseline(baseline);
    let outcome = lock(&opts).unwrap();
    assert!(outcome.is_ok(), "{:?}", outcome.report.findings);
    outcome.write().unwrap();
}

fn check_rustfmt_both_editions(files: &[PathBuf]) {
    for edition in ["2021", "2024"] {
        let mut cmd = Command::new("rustfmt");
        cmd.args(["--edition", edition, "--check"]);
        for path in files {
            cmd.arg(path);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "rustfmt --edition {edition} failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

#[allow(clippy::too_many_lines)]
fn write_contract_root(wit: &Path) {
    // Dependency with BOTH a float-free record (`key`) and a float-bearing record (`metric`).
    let dep_v0 = "package example:shared@0.1.0;\n\
                  interface common {\n\
                    record key { ns: string, id: u64 }\n\
                    record metric { label: string, score: f64 }\n\
                  }\n";

    let v01 = "package example:ergo@0.1.0;\n\
               interface types {\n\
                 use example:shared/common@0.1.0.{key, metric};\n\
                 type id-alias = u32;\n\
                 type key-alias = key;\n\
                 type tag-list = list<string>;\n\
                 type score-alias = f32;\n\
                 enum status { open, closed }\n\
                 flags perms { read, write }\n\
                 record float-free {\n\
                   id: id-alias,\n\
                   k: key-alias,\n\
                   fixed: list<u8, 4>,\n\
                   labels: map<string, u32>,\n\
                   letter: char,\n\
                   st: status,\n\
                   p: perms,\n\
                 }\n\
                 record with-direct-float {\n\
                   id: u32,\n\
                   weight: f64,\n\
                 }\n\
                 record with-alias-float {\n\
                   id: u32,\n\
                   scores: map<string, score-alias>,\n\
                 }\n\
                 record with-dep-float {\n\
                   id: u32,\n\
                   m: metric,\n\
                 }\n\
                 variant clean-choice {\n\
                   empty,\n\
                   item(float-free),\n\
                 }\n\
                 variant float-choice {\n\
                   empty,\n\
                   sample(option<f32>),\n\
                 }\n\
                 type float-free-alias = float-free;\n\
               }\n";

    let v02 = "package example:ergo@0.2.0;\n\
               interface types {\n\
                 use example:shared/common@0.1.0.{key, metric};\n\
                 type id-alias = u32;\n\
                 type key-alias = key;\n\
                 type tag-list = list<string>;\n\
                 type score-alias = f32;\n\
                 enum status { open, closed, archived }\n\
                 flags perms { read, write, admin }\n\
                 record float-free {\n\
                   id: id-alias,\n\
                   k: key-alias,\n\
                   fixed: list<u8, 4>,\n\
                   labels: map<string, u32>,\n\
                   letter: char,\n\
                   st: status,\n\
                   p: perms,\n\
                   note: option<string>,\n\
                 }\n\
                 record with-direct-float {\n\
                   id: u32,\n\
                   weight: f64,\n\
                 }\n\
                 record with-alias-float {\n\
                   id: u32,\n\
                   scores: map<string, score-alias>,\n\
                 }\n\
                 record with-dep-float {\n\
                   id: u32,\n\
                   m: metric,\n\
                 }\n\
                 variant clean-choice {\n\
                   empty,\n\
                   item(float-free),\n\
                   extra(u64),\n\
                 }\n\
                 variant float-choice {\n\
                   empty,\n\
                   sample(option<f32>),\n\
                 }\n\
                 type float-free-alias = float-free;\n\
               }\n";

    // Major v1.0 adds a required field on `float-free` (omits v0.2 -> v1.0 `TryFrom`, keeps
    // v1.0 -> v0.2 `TryFrom`).
    let v10 = "package example:ergo@1.0.0;\n\
               interface types {\n\
                 use example:shared/common@0.1.0.{key, metric};\n\
                 type id-alias = u32;\n\
                 type key-alias = key;\n\
                 type tag-list = list<string>;\n\
                 type score-alias = f32;\n\
                 enum status { open, closed, archived }\n\
                 flags perms { read, write, admin }\n\
                 record float-free {\n\
                   id: id-alias,\n\
                   k: key-alias,\n\
                   fixed: list<u8, 4>,\n\
                   labels: map<string, u32>,\n\
                   letter: char,\n\
                   st: status,\n\
                   p: perms,\n\
                   note: option<string>,\n\
                   required-v1: u64,\n\
                 }\n\
                 record with-direct-float {\n\
                   id: u32,\n\
                   weight: f64,\n\
                 }\n\
                 record with-alias-float {\n\
                   id: u32,\n\
                   scores: map<string, score-alias>,\n\
                 }\n\
                 record with-dep-float {\n\
                   id: u32,\n\
                   m: metric,\n\
                 }\n\
                 variant clean-choice {\n\
                   empty,\n\
                   item(float-free),\n\
                   extra(u64),\n\
                 }\n\
                 variant float-choice {\n\
                   empty,\n\
                   sample(option<f32>),\n\
                 }\n\
                 type float-free-alias = float-free;\n\
               }\n";

    for (ver, body) in [("v0.1", v01), ("v0.2", v02), ("v1.0", v10)] {
        let dir = wit.join(ver);
        let dep_dir = dir.join("deps/shared");
        fs::create_dir_all(&dep_dir).unwrap();
        fs::write(dep_dir.join("common.wit"), dep_v0).unwrap();
        fs::write(dir.join("contract.wit"), body).unwrap();
    }
    lock_version(&wit.join("v0.1"), None);
    fs::copy(wit.join("v0.1/wire.lock"), wit.join("v0.2/wire.lock")).unwrap();
    lock_version(&wit.join("v0.2"), Some(&wit.join("v0.1/wire.lock")));
    lock_version(&wit.join("v1.0"), None);
}

#[allow(clippy::too_many_lines)]
#[test]
fn eq_hash_and_try_from_compile_format_and_roundtrip_in_std_and_no_std() {
    let root = scratch_dir("std-nostd");
    let wit = root.join("wit");
    write_contract_root(&wit);

    // Default options emit neither `Eq, Hash` nor `TryFrom`.
    let default_files = witforge::emit::rust::plan(&wit).unwrap();
    let default_v01 = default_files.files().get("v0_1.rs").unwrap();
    assert!(!default_v01.contains("Eq, Hash"), "{default_v01}");
    let default_conv = default_files.files().get("convert.rs").unwrap();
    assert!(!default_conv.contains("TryFrom"), "{default_conv}");

    // Opt-in `eq_hash = true` + `try_from = true` + `serde = true` in `std` mode.
    let opts = RustOptions::new()
        .with_serde(true)
        .with_eq_hash(true)
        .with_try_from(true);
    let out_std = root.join("crate-std/src/generated");
    let files = witforge::emit::rust::plan_with_options(&wit, None, &opts).unwrap();
    files.write(&out_std).unwrap();

    // Verify rustfmt parity across Edition 2021 and Edition 2024 on every emitted file!
    let rs_files: Vec<PathBuf> = files.files().keys().map(|rel| out_std.join(rel)).collect();
    check_rustfmt_both_editions(&rs_files);

    // Check derive classification: float-free types get `Eq, Hash`, float-bearing types do NOT.
    let v01_rs = fs::read_to_string(out_std.join("v0_1.rs")).unwrap();
    assert!(
        v01_rs.contains(
            "#[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]\n    pub struct FloatFree"
        ),
        "{v01_rs}"
    );
    assert!(
        v01_rs.contains(
            "#[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]\n    pub enum CleanChoice"
        ),
        "{v01_rs}"
    );
    for float_ty in [
        "pub struct WithDirectFloat",
        "pub struct WithAliasFloat",
        "pub struct WithDepFloat",
        "pub enum FloatChoice",
    ] {
        let needle = format!(
            "#[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]\n    {float_ty}"
        );
        assert!(
            v01_rs.contains(&needle),
            "float-bearing `{float_ty}` must not derive Eq/Hash:\n{v01_rs}"
        );
    }

    // Check dependency derives: `Key` gets `Eq, Hash`, `Metric` does NOT.
    let deps_rs = fs::read_to_string(out_std.join("witforge_deps.rs")).unwrap();
    assert!(
        deps_rs.contains(
            "#[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]\n        pub struct Key"
        ),
        "{deps_rs}"
    );
    assert!(
        deps_rs.contains(
            "#[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]\n        pub struct Metric"
        ),
        "{deps_rs}"
    );

    // Check `convert.rs` emits `TryFrom` for nominal types, skips aliases, and omits v0.2 -> v1.0
    // `FloatFree` while keeping v1.0 -> v0.2 `FloatFree`.
    let conv_rs = fs::read_to_string(out_std.join("convert.rs")).unwrap();
    assert!(
        conv_rs
            .contains("impl ::core::convert::TryFrom<&super::__witforge_from::types::FloatFree>"),
        "{conv_rs}"
    );
    assert!(
        conv_rs.contains("impl ::core::convert::TryFrom<super::__witforge_from::types::FloatFree>"),
        "{conv_rs}"
    );
    assert!(
        !conv_rs.contains("FloatFreeAlias>"),
        "aliases must not emit TryFrom: {conv_rs}"
    );
    assert!(
        !conv_rs.contains("TagList>"),
        "structural aliases must not emit TryFrom: {conv_rs}"
    );
    assert!(
        !conv_rs.contains("KeyAlias>"),
        "use aliases must not emit TryFrom: {conv_rs}"
    );

    // Build and run a real scratch consumer crate with `-D warnings -D missing_docs` exercising
    // `HashSet`/`HashMap` keys, `TryFrom<&From>`, `TryFrom<From>`, error cases, and cross-major!
    let crate_std = root.join("crate-std");
    let workspace_lock = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    fs::copy(&workspace_lock, crate_std.join("Cargo.lock")).unwrap();
    fs::write(
        crate_std.join("Cargo.toml"),
        "[package]\n\
         name = \"ergo-std-test\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\
         \n\
         [dependencies]\n\
         serde = { version = \"1\", features = [\"derive\"] }\n",
    )
    .unwrap();
    fs::write(
        crate_std.join("src/lib.rs"),
        "#![deny(warnings, missing_docs)]\n\
         //! Test crate for W11-045.\n\
         /// Generated contract bindings.\n\
         pub mod generated;\n\
         \n\
         #[cfg(test)]\n\
         mod tests {\n\
             use std::collections::{BTreeMap, HashSet};\n\
             use super::generated::{convert::ConversionError, v0_1, v0_2, v1_0, witforge_deps};\n\
             \n\
             #[test]\n\
             fn eq_hash_and_try_from_work_at_runtime() {\n\
                 let mut set: HashSet<v0_1::types::FloatFree> = HashSet::new();\n\
                 let mut labels = BTreeMap::new();\n\
                 labels.insert(\"k\".to_string(), 7);\n\
                 let val = v0_1::types::FloatFree {\n\
                     id: 1,\n\
                     k: witforge_deps::example_shared::common::Key { ns: \"a\".into(), id: 2 },\n\
                     fixed: [1, 2, 3, 4],\n\
                     labels,\n\
                     letter: 'λ',\n\
                     st: v0_1::types::Status::Open,\n\
                     p: v0_1::types::Perms { read: true, write: false },\n\
                 };\n\
                 assert!(set.insert(val.clone()));\n\
                 assert!(!set.insert(val.clone()));\n\
                 \n\
                 // Borrowed TryFrom (&v0_1 -> v0_2)\n\
                 let v2_from_ref: v0_2::types::FloatFree = (&val).try_into().unwrap();\n\
                 assert_eq!(v2_from_ref.note, None);\n\
                 // Owned TryFrom (v0_1 -> v0_2)\n\
                 let v2_owned: v0_2::types::FloatFree = val.clone().try_into().unwrap();\n\
                 assert_eq!(v2_from_ref, v2_owned);\n\
                 \n\
                 // Reverse TryFrom (v0_2 -> v0_1) roundtrips and surfaces UnknownCase\n\
                 let back: v0_1::types::FloatFree = (&v2_owned).try_into().unwrap();\n\
                 assert_eq!(back, val);\n\
                 let archived_err: Result<v0_1::types::Status, ConversionError> =\n\
                     v0_2::types::Status::Archived.try_into();\n\
                 assert!(matches!(archived_err, Err(ConversionError::UnknownCase { .. })));\n\
                 \n\
                 // Cross-major reverse TryFrom (v1_0 -> v0_2) works even though v0_2 -> v1_0 was omitted!\n\
                 let v1_val = v1_0::types::FloatFree {\n\
                     id: 9,\n\
                     k: witforge_deps::example_shared::common::Key { ns: \"b\".into(), id: 3 },\n\
                     fixed: [5, 6, 7, 8],\n\
                     labels: BTreeMap::new(),\n\
                     letter: 'z',\n\
                     st: v1_0::types::Status::Closed,\n\
                     p: v1_0::types::Perms { read: false, write: true, admin: true },\n\
                     note: Some(\"hi\".into()),\n\
                     required_v1: 42,\n\
                 };\n\
                 let down: v0_2::types::FloatFree = v1_val.try_into().unwrap();\n\
                 assert_eq!(down.id, 9);\n\
                 assert_eq!(down.note.as_deref(), Some(\"hi\"));\n\
             }\n\
         }\n",
    )
    .unwrap();

    for args in [
        &["test", "--offline"][..],
        &[
            "clippy",
            "--offline",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ][..],
    ] {
        let out = Command::new("cargo")
            .current_dir(&crate_std)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "cargo {args:?} failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }

    // Also verify `#![no_std]` + `alloc` + `eq_hash` + `try_from` compiles and formats cleanly!
    let nostd_opts = RustOptions::new()
        .with_no_std(true)
        .with_eq_hash(true)
        .with_try_from(true);
    let out_nostd = root.join("crate-nostd/src/generated");
    let nostd_files = witforge::emit::rust::plan_with_options(&wit, None, &nostd_opts).unwrap();
    nostd_files.write(&out_nostd).unwrap();
    let nostd_rs: Vec<PathBuf> = nostd_files
        .files()
        .keys()
        .map(|rel| out_nostd.join(rel))
        .collect();
    check_rustfmt_both_editions(&nostd_rs);

    let crate_nostd = root.join("crate-nostd");
    fs::write(
        crate_nostd.join("Cargo.toml"),
        "[package]\nname = \"ergo-nostd-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        crate_nostd.join("src/lib.rs"),
        "#![no_std]\n\
         #![deny(warnings, missing_docs)]\n\
         //! no_std test crate for W11-045.\n\
         extern crate alloc;\n\
         /// Generated contract bindings.\n\
         pub mod generated;\n\
         \n\
         /// Smoke-check TryFrom and Hash in no_std.\n\
         pub fn check(v: &generated::v0_1::types::Status) -> Result<generated::v0_2::types::Status, generated::convert::ConversionError> {\n\
             fn assert_eq_hash<T: ::core::cmp::Eq + ::core::hash::Hash>(_: &T) {}\n\
             assert_eq_hash(v);\n\
             v.try_into()\n\
         }\n",
    )
    .unwrap();
    let nostd_clippy = Command::new("cargo")
        .current_dir(&crate_nostd)
        .args(["clippy", "--offline", "--lib", "--", "-D", "warnings"])
        .output()
        .unwrap();
    assert!(
        nostd_clippy.status.success(),
        "no_std clippy failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&nostd_clippy.stdout),
        String::from_utf8_lossy(&nostd_clippy.stderr),
    );

    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn eighth_wave_audit_try_from_rustfmt_long_identifiers_and_float_free_diamond_dag() {
    use std::fmt::Write as _;

    let root = scratch_dir("audit-long-dag");
    let wit = root.join("wit");
    fs::create_dir_all(wit.join("v0.1")).unwrap();
    fs::create_dir_all(wit.join("v0.2")).unwrap();

    // 1. Long interface/record identifiers across every `emit_try_from_impl` formatting regime
    // (`single <= 108`, `trait_on_impl <= 100`, `indented_trait <= 100`, `inner_from <= 100`,
    // `inner_from > 100`, and conversion function name `> 76` chars) PLUS a 30-level diamond DAG
    // exercising memoized `is_float_free_def`.
    let mut dag_defs = String::from("  record d0 { a: u32 }\n  record f0 { x: f64 }\n");
    for level in 1..=30 {
        let prev = level - 1;
        let _ = writeln!(
            dag_defs,
            "  record d{level} {{ left: d{prev}, right: d{prev} }}"
        );
        let _ = writeln!(
            dag_defs,
            "  record f{level} {{ left: f{prev}, right: f{prev} }}"
        );
    }

    let make_contract = |ver: &str| {
        format!(
            "package example:longnames@{ver};\n\
             interface m {{\n\
               record a {{ id: u32 }}\n\
               record aaaaa {{ id: u32 }}\n\
               record aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa {{ id: u32 }}\n\
               record aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa {{ id: u32 }}\n\
               record aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa {{ id: u32 }}\n\
               record a-b-c-d-e-f-g-h-i-j-k-l-m-n-o-p-q-r-s-t-u-v-w-x-y-z-a-b-c-d-e-f-g-h-i-j-k-l-m-n {{ id: u32 }}\n\
             }}\n\
             interface payment-processing {{\n\
               record credit-card-billing-statement {{ id: u32 }}\n\
             }}\n\
             interface dag {{\n\
             {dag_defs}\
             }}\n"
        )
    };
    fs::write(wit.join("v0.1/contract.wit"), make_contract("0.1.0")).unwrap();
    fs::write(wit.join("v0.2/contract.wit"), make_contract("0.2.0")).unwrap();
    lock_version(&wit.join("v0.1"), None);
    fs::copy(wit.join("v0.1/wire.lock"), wit.join("v0.2/wire.lock")).unwrap();
    lock_version(&wit.join("v0.2"), Some(&wit.join("v0.1/wire.lock")));

    let started = std::time::Instant::now();
    let opts = RustOptions::new().with_eq_hash(true).with_try_from(true);
    let files = witforge::emit::rust::plan_with_options(&wit, None, &opts).unwrap();
    let elapsed = started.elapsed();
    assert!(
        elapsed.as_secs() < 5,
        "memoized float-free check on 30-level diamond DAG should finish immediately, took {elapsed:?}"
    );

    let v01 = files.files().get("v0_1.rs").unwrap();
    assert!(
        v01.contains("#[derive(Debug, Clone, PartialEq, Eq, Hash)]\n    pub struct D30"),
        "D30 must derive Eq, Hash"
    );
    assert!(
        v01.contains("#[derive(Debug, Clone, PartialEq)]\n    pub struct F30"),
        "F30 must NOT derive Eq, Hash"
    );

    let out_dir = root.join("crate/src/generated");
    files.write(&out_dir).unwrap();
    let rs_files: Vec<PathBuf> = files.files().keys().map(|rel| out_dir.join(rel)).collect();
    check_rustfmt_both_editions(&rs_files);

    // Also compile with rustc to be 100% certain all generated impls typecheck cleanly.
    let crate_dir = root.join("crate");
    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\nname = \"long-tryfrom-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        crate_dir.join("src/lib.rs"),
        "#![deny(warnings, missing_docs)]\n\
         //! Verify long-name TryFrom formatting and compilation.\n\
         /// Generated contract bindings.\n\
         pub mod generated;\n",
    )
    .unwrap();
    let check = Command::new("cargo")
        .current_dir(&crate_dir)
        .args(["check", "--offline", "--lib"])
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "cargo check failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr),
    );

    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn ninth_wave_audit_try_from_value_shadowing_and_scoped_dep_serde_eq_hash_rustfmt() {
    let root = scratch_dir("ninth-wave-value-dep");
    let wit = root.join("wit");
    fs::create_dir_all(wit.join("v0.1/deps/shared")).unwrap();
    fs::create_dir_all(wit.join("v0.2/deps/shared")).unwrap();
    fs::create_dir_all(wit.join("v1.0/deps/shared")).unwrap();

    // Per-major upgraded dependency (indents structs at 12 spaces under `witforge_deps::v0`/`v1`)
    // PLUS nominal record/enum named `value` (would collide with `fn try_from(value: ...)` in
    // `convert.rs` without `self::value(...)`).
    let dep_v0 = "package example:shared@0.1.0;\n\
                  interface common {\n\
                    record key { id: u32 }\n\
                  }\n";
    let dep_v1 = "package example:shared@1.0.0;\n\
                  interface common {\n\
                    record key { id: u64, label: string }\n\
                  }\n";
    fs::write(wit.join("v0.1/deps/shared/common.wit"), dep_v0).unwrap();
    fs::write(wit.join("v0.2/deps/shared/common.wit"), dep_v0).unwrap();
    fs::write(wit.join("v1.0/deps/shared/common.wit"), dep_v1).unwrap();

    fs::write(
        wit.join("v0.1/contract.wit"),
        "package example:valshadow@0.1.0;\n\
         interface a {\n\
           use example:shared/common@0.1.0.{key};\n\
           record value { id: u32, k: key }\n\
         }\n\
         interface b {\n\
           enum value { alpha, beta }\n\
         }\n",
    )
    .unwrap();
    fs::write(
        wit.join("v0.2/contract.wit"),
        "package example:valshadow@0.2.0;\n\
         interface a {\n\
           use example:shared/common@0.1.0.{key};\n\
           record value { id: u32, k: key, note: option<string> }\n\
         }\n\
         interface b {\n\
           enum value { alpha, beta, gamma }\n\
         }\n",
    )
    .unwrap();
    fs::write(
        wit.join("v1.0/contract.wit"),
        "package example:valshadow@1.0.0;\n\
         interface a {\n\
           use example:shared/common@1.0.0.{key};\n\
           record value { id: u32, k: key }\n\
         }\n\
         interface b {\n\
           enum value { alpha, beta }\n\
         }\n",
    )
    .unwrap();

    lock_version(&wit.join("v0.1"), None);
    fs::copy(wit.join("v0.1/wire.lock"), wit.join("v0.2/wire.lock")).unwrap();
    lock_version(&wit.join("v0.2"), Some(&wit.join("v0.1/wire.lock")));
    lock_version(&wit.join("v1.0"), None);

    let opts = RustOptions::new()
        .with_serde(true)
        .with_eq_hash(true)
        .with_try_from(true);
    let files = witforge::emit::rust::plan_with_options(&wit, None, &opts).unwrap();

    let convert_rs = files.files().get("convert.rs").unwrap();
    assert!(
        convert_rs.contains("self::value(value)"),
        "TryFrom impl must qualify `self::value(...)` to avoid parameter shadowing:\n{convert_rs}"
    );

    let crate_dir = root.join("crate");
    let gen_dir = crate_dir.join("src/generated");
    files.write(&gen_dir).unwrap();
    let rs_files: Vec<PathBuf> = files.files().keys().map(|rel| gen_dir.join(rel)).collect();
    check_rustfmt_both_editions(&rs_files);

    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    fs::copy(
        workspace_root.join("Cargo.lock"),
        crate_dir.join("Cargo.lock"),
    )
    .unwrap();
    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\n\
         name = \"valshadow-test\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\
         \n\
         [dependencies]\n\
         serde = { version = \"1\", features = [\"derive\"] }\n\
         \n\
         [workspace]\n",
    )
    .unwrap();
    fs::write(
        crate_dir.join("src/lib.rs"),
        "#![deny(warnings, missing_docs)]\n\
         //! Verify TryFrom for nominal types named `value` and scoped dep serde+eq_hash.\n\
         /// Generated contract bindings.\n\
         pub mod generated;\n\
         \n\
         #[cfg(test)]\n\
         mod tests {\n\
             use super::generated::{v0_1, v0_2, witforge_deps};\n\
             #[test]\n\
             fn try_from_nominal_value_works() {\n\
                 let v1 = v0_1::a::Value {\n\
                     id: 7,\n\
                     k: witforge_deps::v0::example_shared::common::Key { id: 1 },\n\
                 };\n\
                 let v2_ref: v0_2::a::Value = (&v1).try_into().unwrap();\n\
                 let v2_own: v0_2::a::Value = v1.try_into().unwrap();\n\
                 assert_eq!(v2_ref, v2_own);\n\
                 let e1 = v0_1::b::Value::Beta;\n\
                 let e2: v0_2::b::Value = e1.try_into().unwrap();\n\
                 assert_eq!(e2, v0_2::b::Value::Beta);\n\
             }\n\
         }\n",
    )
    .unwrap();
    let test_out = Command::new("cargo")
        .current_dir(&crate_dir)
        .args(["test", "--offline"])
        .output()
        .unwrap();
    assert!(
        test_out.status.success(),
        "cargo test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&test_out.stdout),
        String::from_utf8_lossy(&test_out.stderr),
    );

    fs::remove_dir_all(&root).unwrap();
}
