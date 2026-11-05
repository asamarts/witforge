//! A small performance budget for representative contracts.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use witforge::emit::{glue, json_schema, proto, rust};
use witforge::lock::WireLock;
use witforge::{load, model};

fn contract(root: &Path, interfaces: usize, fields: usize) {
    let dir = root.join("v0.1");
    fs::create_dir_all(&dir).unwrap();
    let mut source = String::from("package example:bench@0.1.0;\n");
    for interface in 0..interfaces {
        let _ = writeln!(source, "interface i{interface} {{");
        for type_index in 0..4 {
            let _ = writeln!(source, "    record t{type_index} {{");
            for field in 0..fields {
                let _ = writeln!(source, "        f{field}: u64,");
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
