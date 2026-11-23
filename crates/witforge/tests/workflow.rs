//! Library-owned check and lock orchestration.

use std::fs;
use std::path::PathBuf;

use witforge::diagnostic::codes;
use witforge::workflow::{self, Baseline, CheckTargetKind, VersionOptions};

fn version_dir() -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("witforge-library-workflow-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let version = root.join("v0.1");
    fs::create_dir_all(&version).unwrap();
    fs::write(
        version.join("contract.wit"),
        "package ex:workflow@0.1.0;\n\
         interface api { record item { id: u64 } get: func() -> item; }\n",
    )
    .unwrap();
    version
}

#[test]
fn library_check_and_lock_are_structured_and_side_effect_free() {
    let version = version_dir();
    let root = version.parent().unwrap();
    assert_eq!(
        workflow::classify_check_target(&version).unwrap(),
        CheckTargetKind::VersionDirectory
    );
    assert_eq!(
        workflow::classify_check_target(root).unwrap(),
        CheckTargetKind::WitRoot
    );

    let first = VersionOptions::new(version.clone()).with_baseline(Baseline::First);
    let report = workflow::lock(&first).unwrap();
    assert!(report.is_ok(), "{:?}", report.report.findings);
    assert!(
        report
            .rendered
            .as_deref()
            .is_some_and(|text| text.contains("api/item.id"))
    );
    assert!(
        !version.join("wire.lock").exists(),
        "library lock must not write until LockReport::write"
    );
    report.write().unwrap();
    assert!(version.join("wire.lock").is_file());

    let check = workflow::check(&VersionOptions::new(version.clone())).unwrap();
    assert!(check.is_ok(), "{:?}", check.findings);
    let root_check = workflow::check_root(root, None).unwrap();
    assert!(root_check.is_ok(), "{:?}", root_check.findings);

    fs::write(
        version.join("contract.wit"),
        "package ex:workflow@0.1.0;\n\
         interface api { record item { id: u64, label: string } get: func() -> item; }\n",
    )
    .unwrap();
    let drift = workflow::check(&VersionOptions::new(version.clone())).unwrap();
    assert!(!drift.is_ok());
    let finding = drift
        .findings
        .iter()
        .find(|finding| {
            finding.diagnostic.as_ref().is_some_and(|diagnostic| {
                diagnostic.diagnostic.code == codes::LOCKFILE_MISSING_NUMBER
            })
        })
        .expect("missing-number finding");
    let diagnostic = finding.diagnostic.as_ref().unwrap();
    assert_eq!(
        diagnostic.diagnostic.path.as_deref(),
        Some("ex:workflow/api/item.label")
    );
    assert!(
        diagnostic
            .file
            .as_deref()
            .is_some_and(|file| file.ends_with("contract.wit"))
    );
    fs::remove_dir_all(root).unwrap();
}
