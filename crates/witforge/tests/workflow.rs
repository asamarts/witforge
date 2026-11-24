//! Library-owned check and lock orchestration.

use std::fs;
use std::path::{Path, PathBuf};

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

#[test]
fn root_check_validates_since_at_the_first_declaration() {
    let root = std::env::temp_dir().join(format!("witforge-since-history-{}", std::process::id()));
    let old = root.join("v0.1");
    let new = root.join("v0.2");
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&new).unwrap();
    fs::write(
        old.join("contract.wit"),
        "package ex:history@0.1.0;\ninterface api { record item { id: u64 } }\n",
    )
    .unwrap();
    fs::write(
        new.join("contract.wit"),
        "package ex:history@0.2.0;\n\
         interface api {\n\
             @since(version = 0.2.0)\n\
             record item { id: u64 }\n\
         }\n",
    )
    .unwrap();
    workflow::lock(&VersionOptions::new(old.clone()).with_baseline(Baseline::First))
        .unwrap()
        .write()
        .unwrap();
    fs::copy(old.join("wire.lock"), new.join("wire.lock")).unwrap();
    workflow::lock(
        &VersionOptions::new(new.clone()).with_baseline(Baseline::Against(old.join("wire.lock"))),
    )
    .unwrap()
    .write()
    .unwrap();
    let single = workflow::check(&VersionOptions::new(new.clone())).unwrap();
    assert!(single.is_ok(), "{:?}", single.findings);

    let report = workflow::check_root(&root, None).unwrap();
    let finding = report
        .findings
        .iter()
        .find(|finding| {
            finding
                .diagnostic
                .as_ref()
                .is_some_and(|diagnostic| diagnostic.diagnostic.code == codes::SUBSET_ANNOTATION)
        })
        .expect("@since history finding");
    let diagnostic = finding.diagnostic.as_ref().unwrap();
    assert_eq!(diagnostic.logical.as_deref(), Some("api/item"));
    assert!(
        diagnostic
            .diagnostic
            .message
            .contains("first declaration in this root is version 0.1.0")
    );
    assert!(
        diagnostic.file.as_deref().is_some_and(|file| {
            Path::new(file).ends_with(Path::new("v0.2").join("contract.wit"))
        })
    );
    let generator_error = witforge::emit::rust::plan(&root).unwrap_err();
    assert!(witforge::is_contract_error(&generator_error));
    assert!(
        format!("{generator_error:#}").contains("first declaration in this root is version 0.1.0")
    );

    for since in ["0.1.0", "0.0.0"] {
        fs::write(
            new.join("contract.wit"),
            format!(
                "package ex:history@0.2.0;\n\
                 interface api {{\n\
                     @since(version = {since})\n\
                     record item {{ id: u64 }}\n\
                 }}\n"
            ),
        )
        .unwrap();
        let valid = workflow::check_root(&root, None).unwrap();
        assert!(valid.is_ok(), "{:?}", valid.findings);
        witforge::emit::rust::plan(&root).unwrap();
    }
    fs::remove_dir_all(root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn since_history_tracks_contiguous_non_unstable_lifetime_and_rejects_metadata() {
    let root =
        std::env::temp_dir().join(format!("witforge-since-lifecycle-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let v01 = root.join("v0.1");
    let v02 = root.join("v0.2");
    let v10 = root.join("v1.0");
    fs::create_dir_all(&v01).unwrap();
    fs::create_dir_all(&v02).unwrap();
    fs::create_dir_all(&v10).unwrap();

    // v0.1: `draft` is @unstable, `recycled` is stable at 0.1.0.
    fs::write(
        v01.join("contract.wit"),
        "package ex:life@0.1.0;\n\
         interface api {\n\
             @unstable(feature = exp)\n\
             record draft { id: u64 }\n\
             @since(version = 0.1.0)\n\
             record recycled { id: u64 }\n\
         }\n",
    )
    .unwrap();
    workflow::lock(&VersionOptions::new(v01.clone()).with_baseline(Baseline::First))
        .unwrap()
        .write()
        .unwrap();

    // v0.2: `draft` stabilizes at 0.2.0; `recycled` is removed.
    fs::write(
        v02.join("contract.wit"),
        "package ex:life@0.2.0;\n\
         interface api {\n\
             @since(version = 0.2.0)\n\
             record draft { id: u64 }\n\
         }\n",
    )
    .unwrap();
    fs::copy(v01.join("wire.lock"), v02.join("wire.lock")).unwrap();
    workflow::lock(
        &VersionOptions::new(v02.clone()).with_baseline(Baseline::Against(v01.join("wire.lock"))),
    )
    .unwrap()
    .write()
    .unwrap();

    // v1.0: `draft` remains @since(0.2.0); `recycled` is re-added in the new major at @since(1.0.0).
    fs::write(
        v10.join("contract.wit"),
        "package ex:life@1.0.0;\n\
         interface api {\n\
             @since(version = 0.2.0)\n\
             record draft { id: u64 }\n\
             @since(version = 1.0.0)\n\
             record recycled { id: u64 }\n\
         }\n",
    )
    .unwrap();
    workflow::lock(&VersionOptions::new(v10.clone()).with_baseline(Baseline::First))
        .unwrap()
        .write()
        .unwrap();

    let report = workflow::check_root(&root, None).unwrap();
    assert!(report.is_ok(), "{:?}", report.findings);
    witforge::emit::rust::plan(&root).unwrap();

    // Claiming @since(0.1.0) on `recycled` after it was absent in v0.2 must fail.
    fs::write(
        v10.join("contract.wit"),
        "package ex:life@1.0.0;\n\
         interface api {\n\
             @since(version = 0.2.0)\n\
             record draft { id: u64 }\n\
             @since(version = 0.1.0)\n\
             record recycled { id: u64 }\n\
         }\n",
    )
    .unwrap();
    let bad = workflow::check_root(&root, None).unwrap();
    assert!(
        bad.findings.iter().any(|finding| {
            finding.diagnostic.as_ref().is_some_and(|diag| {
                diag.diagnostic.code == codes::SUBSET_ANNOTATION
                    && diag.logical.as_deref() == Some("api/recycled")
            })
        }),
        "{:?}",
        bad.findings
    );

    // Pre-release and build metadata on @since and @deprecated must fail with WF1014.
    // Use package version 0.2.0 so `wit-parser` accepts `0.1.0+build.1` (semver orders
    // `0.1.0+build.1 > 0.1.0` and `< 0.2.0`) and hands it to witforge's subset checker.
    for (annotation, source) in [
        (
            "@since pre-release",
            "package ex:meta@0.2.0;\ninterface api { @since(version = 0.1.0-alpha.1) record r { id: u64 } }\n",
        ),
        (
            "@since build",
            "package ex:meta@0.2.0;\ninterface api { @since(version = 0.1.0+build.1) record r { id: u64 } }\n",
        ),
        (
            "@deprecated pre-release",
            "package ex:meta@0.2.0;\ninterface api { @since(version = 0.1.0) @deprecated(version = 0.2.0-rc.1) record r { id: u64 } }\n",
        ),
        (
            "@deprecated build",
            "package ex:meta@0.2.0;\ninterface api { @since(version = 0.1.0) @deprecated(version = 0.1.0+meta) record r { id: u64 } }\n",
        ),
    ] {
        let contract = witforge::load_sources(vec![witforge::SourceFile::new(
            "contract.wit".to_string(),
            source.to_string(),
        )])
        .unwrap();
        let diags = witforge::subset::check_diagnostics(&contract);
        assert!(
            diags
                .iter()
                .any(|diag| diag.code == codes::SUBSET_ANNOTATION),
            "{annotation}: expected WF1014, got {diags:?}"
        );
    }

    // Dependency @since pre-release metadata is also refused with WF1014.
    let dep_contract = witforge::load_sources(vec![
        witforge::SourceFile::new(
            "contract.wit".to_string(),
            "package ex:root@0.1.0;\ninterface api { use ex:dep/shared@0.1.0.{item}; record r { x: item } }\n"
                .to_string(),
        ),
        witforge::SourceFile::new(
            "deps/dep.wit".to_string(),
            "package ex:dep@0.1.0;\ninterface shared { @since(version = 0.1.0-beta.1) record item { id: u64 } }\n"
                .to_string(),
        ),
    ])
    .unwrap();
    let dep_diags = witforge::subset::check_diagnostics(&dep_contract);
    assert!(
        dep_diags
            .iter()
            .any(|diag| diag.code == codes::SUBSET_ANNOTATION),
        "{dep_diags:?}"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn check_against_and_check_root_report_dependency_diffs_in_order_and_without_duplicates() {
    let root = std::env::temp_dir().join(format!(
        "witforge-workflow-dep-order-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let v01 = root.join("v0.1");
    let v02 = root.join("v0.2");
    fs::create_dir_all(v01.join("deps/shared")).unwrap();
    fs::create_dir_all(v02.join("deps/shared")).unwrap();

    fs::write(
        v01.join("deps/shared/types.wit"),
        "package ex:shared@0.1.0;\ninterface common { record tag { id: u32 } }\n",
    )
    .unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package ex:app@0.1.0;\ninterface api { use ex:shared/common@0.1.0.{tag}; record r { t: tag } }\n",
    )
    .unwrap();
    workflow::lock(&VersionOptions::new(v01.clone()).with_baseline(Baseline::First))
        .unwrap()
        .write()
        .unwrap();

    fs::write(
        v02.join("deps/shared/types.wit"),
        "package ex:shared@0.2.0;\ninterface common { record tag { id: u32 } }\n",
    )
    .unwrap();
    fs::write(
        v02.join("contract.wit"),
        "package ex:app@0.2.0;\ninterface api { use ex:shared/common@0.2.0.{tag}; record r { t: tag } }\n",
    )
    .unwrap();
    fs::copy(v01.join("wire.lock"), v02.join("wire.lock")).unwrap();

    // Single-directory `check --against` must report baseline -> current (`0.1.0` -> `0.2.0`).
    let single = workflow::check(
        &VersionOptions::new(v02.clone()).with_baseline(Baseline::Against(v01.join("wire.lock"))),
    )
    .unwrap();
    let dep_diags: Vec<_> = single
        .findings
        .iter()
        .filter_map(|f| f.diagnostic.as_ref())
        .filter(|d| d.diagnostic.code == codes::DEPENDENCY_GRAPH_MISMATCH)
        .collect();
    assert_eq!(dep_diags.len(), 1, "{single:?}");
    let msg = &dep_diags[0].diagnostic.message;
    assert!(
        msg.contains("0.1.0") && msg.contains("0.2.0"),
        "unexpected dependency mismatch message: {msg}"
    );
    assert!(
        msg.find("0.1.0").unwrap() < msg.find("0.2.0").unwrap(),
        "expected baseline (0.1.0) before current (0.2.0): {msg}"
    );

    // `check_root` must emit exactly ONE WF1033 per mismatch (no duplicate contradictory WF1033).
    let root_report = workflow::check_root(&root, None).unwrap();
    let root_dep_diags: Vec<_> = root_report
        .findings
        .iter()
        .filter_map(|f| f.diagnostic.as_ref())
        .filter(|d| d.diagnostic.code == codes::DEPENDENCY_GRAPH_MISMATCH)
        .collect();
    assert_eq!(root_dep_diags.len(), 1, "{root_dep_diags:?}");

    fs::remove_dir_all(&root).unwrap();
}
