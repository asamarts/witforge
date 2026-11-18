//! A small performance budget for representative contracts.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use witforge::emit::{glue, json_schema, proto, rust};
use witforge::lock::WireLock;
use witforge::{diff, load, model};

fn write_contract(root: &Path, minor: u64, interfaces: usize, fields: usize, evolve: bool) {
    let dir = root.join(format!("v0.{minor}"));
    fs::create_dir_all(&dir).unwrap();
    let mut source = format!("package example:bench@0.{minor}.0;\n");
    for interface in 0..interfaces {
        let _ = writeln!(source, "interface i{interface} {{");
        for type_index in 0..4 {
            let _ = writeln!(source, "    record t{type_index} {{");
            for field in 0..fields {
                let _ = writeln!(source, "        f{field}: u64,");
            }
            if evolve {
                source.push_str("        added: option<u64>,\n");
            }
            source.push_str("    }\n");
            let _ = writeln!(source, "    enum e{interface}x{type_index} {{ a, b, c }}");
            let _ = writeln!(
                source,
                "    variant v{interface}x{type_index} {{ a, b(u64) }}"
            );
            let _ = writeln!(source, "    flags fl{interface}x{type_index} {{ a, b }}");
            let _ = writeln!(
                source,
                "    call{interface}x{type_index}: func(x: u64) -> u64;"
            );
        }
        source.push_str("}\n");
    }
    fs::write(dir.join("contract.wit"), source).unwrap();
}

fn contract(root: &Path, interfaces: usize, fields: usize) {
    write_contract(root, 1, interfaces, fields, false);
}

fn write_lock(root: &Path, minor: u64, previous: Option<&WireLock>) {
    let dir = root.join(format!("v0.{minor}"));
    let contract = load(&dir).unwrap();
    let model = model::build(&contract).unwrap();
    let members = model.members().unwrap();
    let mut lock = previous.cloned().unwrap_or_default();
    lock.reconcile(&members).unwrap();
    fs::write(dir.join("wire.lock"), lock.render()).unwrap();
}

fn measure(root: &Path) -> Duration {
    let started = Instant::now();
    let contract = load(&root.join("v0.1")).unwrap();
    let model = model::build(&contract).unwrap();
    let members = model.members().unwrap();
    let mut lock = WireLock::default();
    lock.reconcile(&members).unwrap();
    let rust = rust::emit_version(&model).unwrap();
    let proto = proto::emit_major(&model, &lock).unwrap();
    let schema = json_schema::emit_version(&model).unwrap();
    let glue = glue::emit_version(&model, &lock, "crate::plain", "crate::proto").unwrap();
    assert!(!rust.is_empty() && !proto.is_empty() && !schema.is_empty() && !glue.is_empty());
    started.elapsed()
}

fn workspace(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("witforge-bench-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn small_and_large_contracts_stay_within_benchmark_budget() {
    let small = workspace("small");
    contract(&small, 1, 2);
    let small_elapsed = measure(&small);
    println!("small contract: {small_elapsed:?}");

    let large = workspace("large");
    contract(&large, 24, 8);
    let large_elapsed = measure(&large);
    println!("large contract: {large_elapsed:?}");

    // These are deliberately generous catastrophic-regression ceilings, not microbenchmarks.
    // The release CI job runs this test with `--release`; the normal test matrix still runs it.
    assert!(small_elapsed < Duration::from_secs(5), "{small_elapsed:?}");
    assert!(large_elapsed < Duration::from_secs(15), "{large_elapsed:?}");

    fs::remove_dir_all(&small).unwrap();
    fs::remove_dir_all(&large).unwrap();
}

#[test]
fn two_version_diff_and_conversion_stay_within_budget() {
    let root = workspace("two-version");
    write_contract(&root, 1, 24, 8, false);
    write_lock(&root, 1, None);
    write_contract(&root, 2, 24, 8, true);
    let v1_lock =
        WireLock::parse(&fs::read_to_string(root.join("v0.1/wire.lock")).unwrap()).unwrap();
    write_lock(&root, 2, Some(&v1_lock));

    let diff_started = Instant::now();
    let analysis = diff::compare_with_impacts(&root.join("v0.1"), &root.join("v0.2")).unwrap();
    let diff_elapsed = diff_started.elapsed();
    assert_ne!(analysis.report.changes.len(), 0);
    println!("two-version diff: {diff_elapsed:?}");

    let plan_started = Instant::now();
    let files = rust::plan(&root).unwrap();
    let plan_elapsed = plan_started.elapsed();
    assert!(files.files().keys().any(|name| name == "convert.rs"));
    println!("two-version conversion plan: {plan_elapsed:?}");

    // Catastrophic-regression ceilings; the large diff is normally measured in tens of
    // milliseconds, and conversion planning covers both generated versions and the adapters.
    assert!(diff_elapsed < Duration::from_secs(5), "{diff_elapsed:?}");
    assert!(plan_elapsed < Duration::from_secs(5), "{plan_elapsed:?}");

    fs::remove_dir_all(&root).unwrap();
}
