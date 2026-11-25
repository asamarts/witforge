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

#[allow(clippy::too_many_lines)]
#[test]
fn workflow_subset_since_history_and_load_sources_audit_regressions() {
    let root = std::env::temp_dir().join(format!("witforge-workflow-audit-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);

    // 1. Zero-member version directory without `wire.lock` fails `check(--first)`, `check_root`,
    //    and `check_release` with `WF1100`.
    let empty_root = root.join("empty-root");
    let empty_v01 = empty_root.join("v0.1");
    fs::create_dir_all(&empty_v01).unwrap();
    fs::write(
        empty_v01.join("contract.wit"),
        "package ex:empty@0.1.0;\ninterface api {}\n",
    )
    .unwrap();
    let single_empty =
        workflow::check(&VersionOptions::new(empty_v01.clone()).with_baseline(Baseline::First))
            .unwrap();
    assert!(!single_empty.is_ok(), "{single_empty:?}");
    assert!(
        single_empty
            .findings
            .iter()
            .filter_map(|f| f.diagnostic.as_ref())
            .any(|d| d.diagnostic.code == codes::LOCKFILE_ERROR),
        "{single_empty:?}"
    );
    let root_empty = workflow::check_root(&empty_root, None).unwrap();
    assert!(!root_empty.is_ok(), "{root_empty:?}");
    let rel_empty = workflow::check_release(&empty_v01, None).unwrap();
    assert!(!rel_empty.is_ok(), "{rel_empty:?}");

    // 2. `check_root` with explicit `--released custom` ignores default `wit/released`, and
    //    collects retirement-order violations into `Report` with the custom manifest path.
    let custom_root = root.join("custom-root");
    let cv01 = custom_root.join("v0.1");
    let cv02 = custom_root.join("v0.2");
    fs::create_dir_all(&cv01).unwrap();
    fs::create_dir_all(&cv02).unwrap();
    fs::write(
        cv01.join("contract.wit"),
        "package ex:cust@0.1.0;\ninterface api { record r { a: u32 } }\n",
    )
    .unwrap();
    workflow::lock(&VersionOptions::new(cv01.clone()).with_baseline(Baseline::First))
        .unwrap()
        .write()
        .unwrap();
    fs::write(
        cv02.join("contract.wit"),
        "package ex:cust@0.2.0;\ninterface api { record r { a: u32, b: option<u32> } }\n",
    )
    .unwrap();
    fs::copy(cv01.join("wire.lock"), cv02.join("wire.lock")).unwrap();
    workflow::lock(
        &VersionOptions::new(cv02.clone()).with_baseline(Baseline::Against(cv01.join("wire.lock"))),
    )
    .unwrap()
    .write()
    .unwrap();
    // Corrupt default `released` (invalid syntax) — explicit custom manifest must NOT read it!
    fs::write(custom_root.join("released"), "not a valid entry !!!\n").unwrap();
    let custom_manifest = custom_root.join("custom-released");
    fs::write(&custom_manifest, "v0.1\nv0.2 retired\n").unwrap();
    let bad_retire = workflow::check_root(&custom_root, Some(&custom_manifest)).unwrap();
    assert!(!bad_retire.is_ok(), "{bad_retire:?}");
    let retire_diag = bad_retire
        .findings
        .iter()
        .filter_map(|f| f.diagnostic.as_ref())
        .find(|d| d.diagnostic.code == codes::LOCKFILE_ERROR)
        .expect("retirement order error");
    assert!(
        retire_diag
            .file
            .as_deref()
            .is_some_and(|f| f.ends_with("custom-released")),
        "{retire_diag:?}"
    );

    // 3. `check_release` reports missing predecessor lockfile as `WF1100` in `Report`, validates
    //    earlier directory version mismatches, and checks `check_append_only` when already listed.
    let cr_root = root.join("check-release");
    let cr1 = cr_root.join("v0.1");
    let cr2 = cr_root.join("v0.2");
    fs::create_dir_all(&cr1).unwrap();
    fs::create_dir_all(&cr2).unwrap();
    fs::write(
        cr1.join("contract.wit"),
        "package ex:cr@0.1.0;\ninterface api { record r { a: u32 } }\n",
    )
    .unwrap();
    fs::write(
        cr2.join("contract.wit"),
        "package ex:cr@0.2.0;\ninterface api { record r { a: u32 } }\n",
    )
    .unwrap();
    // Missing predecessor `v0.1/wire.lock` is reported in `Report` (not operational Err).
    let missing_pred = workflow::check_release(&cr2, None).unwrap();
    assert!(!missing_pred.is_ok(), "{missing_pred:?}");
    assert!(
        missing_pred
            .findings
            .iter()
            .filter_map(|f| f.diagnostic.as_ref())
            .any(|d| d.diagnostic.code == codes::LOCKFILE_ERROR),
        "{missing_pred:?}"
    );
    // Write incompatible locks for v0.1 and already-listed v0.2 (`r.a = 1` vs `r.a = 2`).
    workflow::lock(&VersionOptions::new(cr1.clone()).with_baseline(Baseline::First))
        .unwrap()
        .write()
        .unwrap();
    fs::write(
        cr2.join("wire.lock"),
        format!("{}\nex:cr/api/r.a = 2 field u32\n", witforge::lock::HEADER),
    )
    .unwrap();
    let cr_rel = cr_root.join("released");
    fs::write(&cr_rel, "v0.1\nv0.2\n").unwrap();
    let bad_append = workflow::check_release(&cr2, Some(&cr_rel)).unwrap();
    assert!(!bad_append.is_ok(), "{bad_append:?}");
    assert!(
        bad_append
            .findings
            .iter()
            .filter_map(|f| f.diagnostic.as_ref())
            .any(|d| d.diagnostic.message.contains("does not continue")),
        "{bad_append:?}"
    );

    // 4. `subset` rejects invalid stability annotations on world-owned `use` type items.
    let world_use = witforge::load_sources(vec![witforge::SourceFile::new(
        "contract.wit".to_string(),
        "package ex:wuse@0.2.0;\n\
         interface api { record r { a: u32 } }\n\
         world svc {\n\
             @since(version = 0.1.1)\n\
             use api.{r};\n\
             export api;\n\
         }\n"
        .to_string(),
    )])
    .unwrap();
    let world_diags = witforge::subset::check_located_diagnostics(&world_use);
    assert!(
        world_diags
            .iter()
            .any(|d| d.diagnostic.code == codes::SUBSET_ANNOTATION
                && d.diagnostic.message.contains("0.1.1")),
        "{world_diags:?}"
    );
    let world_use_pre = witforge::load_sources(vec![witforge::SourceFile::new(
        "contract.wit".to_string(),
        "package ex:wuse@0.2.0;\n\
         interface api { record r { a: u32 } }\n\
         world svc {\n\
             @since(version = 0.1.0-alpha)\n\
             use api.{r};\n\
             export api;\n\
         }\n"
        .to_string(),
    )])
    .unwrap();
    let world_pre_diags = witforge::subset::check_located_diagnostics(&world_use_pre);
    assert!(
        world_pre_diags
            .iter()
            .any(|d| d.diagnostic.code == codes::SUBSET_ANNOTATION
                && d.diagnostic.message.contains("0.1.0-alpha")),
        "{world_pre_diags:?}"
    );

    // 5. `SinceHistory` regressions:
    //    (a) Failed `v0.1` does NOT cause cascading WF1014 on `v0.2` declaring `@since(0.1.0)`.
    let sh_fail = root.join("sh-fail");
    fs::create_dir_all(sh_fail.join("v0.1")).unwrap();
    fs::create_dir_all(sh_fail.join("v0.2")).unwrap();
    fs::write(
        sh_fail.join("v0.1/contract.wit"),
        "package ex:shf@0.1.0;\ninterface api { record ok { id: u32 } }\n",
    )
    .unwrap();
    workflow::lock(&VersionOptions::new(sh_fail.join("v0.1")).with_baseline(Baseline::First))
        .unwrap()
        .write()
        .unwrap();
    fs::write(
        sh_fail.join("v0.2/contract.wit"),
        "package ex:shf@0.2.0;\n@since(version = 0.1.0)\ninterface api { record ok { id: u32 } }\n",
    )
    .unwrap();
    fs::copy(
        sh_fail.join("v0.1/wire.lock"),
        sh_fail.join("v0.2/wire.lock"),
    )
    .unwrap();
    // Now introduce a subset violation in `v0.1`: `check_root` must report `v0.1`'s subset error
    // without cascading a bogus WF1014 on `v0.2`!
    fs::write(
        sh_fail.join("v0.1/contract.wit"),
        "package ex:shf@0.1.0;\ninterface api { record bad { c: char } }\n",
    )
    .unwrap();
    let sh_fail_rep = workflow::check_root(&sh_fail, None).unwrap();
    assert!(
        !sh_fail_rep
            .findings
            .iter()
            .filter_map(|f| f.diagnostic.as_ref())
            .any(|d| d.diagnostic.code == codes::SUBSET_ANNOTATION),
        "failed v0.1 must not trigger cascading WF1014 on v0.2: {sh_fail_rep:?}"
    );

    //    (b) Gap root (`v0.1`, `v0.3`) catches `first (0.1.0) < since (0.2.0)` even when v0.2 is absent.
    let sh_gap = root.join("sh-gap");
    fs::create_dir_all(sh_gap.join("v0.1")).unwrap();
    fs::create_dir_all(sh_gap.join("v0.3")).unwrap();
    fs::write(
        sh_gap.join("v0.1/contract.wit"),
        "package ex:shg@0.1.0;\ninterface api { record r { id: u32 } }\n",
    )
    .unwrap();
    workflow::lock(&VersionOptions::new(sh_gap.join("v0.1")).with_baseline(Baseline::First))
        .unwrap()
        .write()
        .unwrap();
    fs::write(
        sh_gap.join("v0.3/contract.wit"),
        "package ex:shg@0.3.0;\n@since(version = 0.2.0)\ninterface api { record r { id: u32 } }\n",
    )
    .unwrap();
    fs::copy(sh_gap.join("v0.1/wire.lock"), sh_gap.join("v0.3/wire.lock")).unwrap();
    let sh_gap_rep = workflow::check_root(&sh_gap, None).unwrap();
    assert!(
        sh_gap_rep
            .findings
            .iter()
            .filter_map(|f| f.diagnostic.as_ref())
            .any(|d| d.diagnostic.code == codes::SUBSET_ANNOTATION
                && d.diagnostic.message.contains("0.2.0")
                && d.diagnostic.message.contains("0.1.0")),
        "{sh_gap_rep:?}"
    );

    //    (c) Demoting a stable item to `@unstable` in v0.2 then claiming `@since(0.3.0)` in v0.3
    //        preserves `first_seen = 0.1.0` and rejects `@since(0.3.0)`.
    let sh_demote = root.join("sh-demote");
    for (dir, body) in [
        (
            "v0.1",
            "package ex:shd@0.1.0;\ninterface api { record r { id: u32 } }\n",
        ),
        (
            "v0.2",
            "package ex:shd@0.2.0;\n@unstable(feature = exp)\ninterface api { record r { id: u32 } }\n",
        ),
        (
            "v0.3",
            "package ex:shd@0.3.0;\n@since(version = 0.3.0)\ninterface api { record r { id: u32 } }\n",
        ),
    ] {
        let d = sh_demote.join(dir);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("contract.wit"), body).unwrap();
    }
    workflow::lock(&VersionOptions::new(sh_demote.join("v0.1")).with_baseline(Baseline::First))
        .unwrap()
        .write()
        .unwrap();
    fs::copy(
        sh_demote.join("v0.1/wire.lock"),
        sh_demote.join("v0.2/wire.lock"),
    )
    .unwrap();
    fs::copy(
        sh_demote.join("v0.1/wire.lock"),
        sh_demote.join("v0.3/wire.lock"),
    )
    .unwrap();
    let sh_demote_rep = workflow::check_root(&sh_demote, None).unwrap();
    assert!(
        sh_demote_rep
            .findings
            .iter()
            .filter_map(|f| f.diagnostic.as_ref())
            .any(|d| d.diagnostic.code == codes::SUBSET_ANNOTATION
                && d.diagnostic.message.contains("0.3.0")
                && d.diagnostic.message.contains("0.1.0")),
        "{sh_demote_rep:?}"
    );

    // 6. `ensure_dependency_identity` same-as-root `ContractError` includes the dep entry file.
    let same_dep_err = witforge::load_sources(vec![
        witforge::SourceFile::new(
            "contract.wit".to_string(),
            "package ex:same@0.1.0;\ninterface api {}\n".to_string(),
        ),
        witforge::SourceFile::new(
            "deps/selfdep.wit".to_string(),
            "package ex:same@0.1.0;\ninterface other {}\n".to_string(),
        ),
    ])
    .unwrap_err();
    let located = witforge::error_located_diagnostics(&same_dep_err).expect("located diag");
    assert_eq!(
        located[0].diagnostic.code,
        codes::SUBSET_DEPENDENCY_IDENTITY
    );
    assert_eq!(located[0].file.as_deref(), Some("deps/selfdep.wit"));

    // 7. `load_sources` + `member_locations` locates `package` across nested block comments,
    //    multiline `package\n`, multi-file empty root packages, and empty in-memory `deps/`.
    let mem_contract = witforge::load_sources(vec![
        witforge::SourceFile::new(
            "a.wit".to_string(),
            "// no package header here\n".to_string(),
        ),
        witforge::SourceFile::new(
            "b.wit".to_string(),
            "/* outer /* package fake:x@0.1.0; */ still comment */\n\
             package\n\
               ex:mem@0.1.0;\n"
                .to_string(),
        ),
        witforge::SourceFile::new(
            "deps/emptydep/a.wit".to_string(),
            "// dep comment\n".to_string(),
        ),
        witforge::SourceFile::new(
            "deps/emptydep/b.wit".to_string(),
            "package ex:emptydep@0.1.0;\n".to_string(),
        ),
    ])
    .unwrap();
    let locs = mem_contract.member_locations();
    let (root_file, root_pos) = locs.get("ex:mem").expect("root package location");
    assert_eq!(root_file, "b.wit");
    assert_eq!(root_pos.line, 2);
    let (dep_file, dep_pos) = locs.get("ex:emptydep").expect("dep package location");
    assert_eq!(dep_file, "deps/emptydep/b.wit");
    assert_eq!(dep_pos.line, 1);

    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn second_wave_audit_freeze_and_check_release_regressions() {
    use std::process::Command;

    let base = std::env::temp_dir().join(format!(
        "witforge-second-wave-workflow-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();

    // 1. `check_release` must report corrupt/missing predecessor `wire.lock` even when the target
    //    version is already listed in `released`.
    let rel_root = base.join("rel-root");
    let v01 = rel_root.join("v0.1");
    let v02 = rel_root.join("v0.2");
    fs::create_dir_all(&v01).unwrap();
    fs::create_dir_all(&v02).unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package ex:rel@0.1.0;\ninterface api { record item { id: u64 } }\n",
    )
    .unwrap();
    fs::write(
        v02.join("contract.wit"),
        "package ex:rel@0.2.0;\ninterface api { record item { id: u64, name: option<string> } }\n",
    )
    .unwrap();
    let r01 =
        workflow::lock(&workflow::VersionOptions::new(v01.clone()).with_baseline(Baseline::First))
            .unwrap();
    assert!(r01.is_ok(), "{:?}", r01.report.findings);
    fs::write(&r01.lockfile, r01.rendered.unwrap()).unwrap();
    fs::copy(v01.join("wire.lock"), v02.join("wire.lock")).unwrap();
    let r02 = workflow::lock(
        &workflow::VersionOptions::new(v02.clone())
            .with_baseline(Baseline::Against(v01.join("wire.lock"))),
    )
    .unwrap();
    assert!(r02.is_ok(), "{:?}", r02.report.findings);
    fs::write(&r02.lockfile, r02.rendered.unwrap()).unwrap();
    fs::write(rel_root.join("released"), "v0.1\nv0.2\n").unwrap();
    assert!(
        workflow::check_release(&v02, None).unwrap().is_ok(),
        "clean already-listed v0.2 must pass check_release"
    );

    // Corrupt predecessor wire.lock must propagate operational PARSE_ERROR (`WF9003`, exit 2),
    // while a missing predecessor wire.lock is reported as `LOCKFILE_ERROR` (`WF1100`) in `Report`.
    fs::write(v01.join("wire.lock"), "not a valid lockfile\n").unwrap();
    let corrupt_pred = workflow::check_release(&v02, None).unwrap_err();
    let corrupt_diags = witforge::error_diagnostics(&corrupt_pred).unwrap();
    assert_eq!(
        corrupt_diags[0].code,
        witforge::diagnostic::codes::PARSE_ERROR,
        "expected PARSE_ERROR on corrupt predecessor lock: {corrupt_diags:?}"
    );

    fs::write(rel_root.join("released"), "v0.2\n").unwrap();
    fs::remove_file(v01.join("wire.lock")).unwrap();
    let missing_pred = workflow::check_release(&v02, None).unwrap();
    assert!(
        !missing_pred.is_ok(),
        "missing predecessor wire.lock must fail check_release on already-listed version"
    );
    assert!(
        missing_pred.findings.iter().any(|f| f
            .diagnostic
            .as_ref()
            .is_some_and(|d| d.diagnostic.code == witforge::diagnostic::codes::LOCKFILE_ERROR)),
        "expected LOCKFILE_ERROR finding: {:?}",
        missing_pred.findings
    );

    // 2. `freeze` case-insensitive manifest reconciliation (`core.ignorecase=true`) must catch
    //    alternate-casing overrides (including after `git rm`) and never dereference a symlinked
    //    `--released` manifest.
    let git_repo = base.join("git-repo");
    let git_wit = git_repo.join("wit");
    let git_v01 = git_wit.join("v0.1");
    fs::create_dir_all(&git_v01).unwrap();
    let run_git = |args: &[&str]| {
        let out = Command::new("git")
            .current_dir(&git_repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run_git(&["init", "-q"]);
    run_git(&["config", "user.name", "Test"]);
    run_git(&["config", "user.email", "test@example.invalid"]);
    run_git(&["config", "commit.gpgsign", "false"]);
    fs::write(
        git_v01.join("contract.wit"),
        "package ex:frz@0.1.0;\ninterface api { record item { id: u64 } }\n",
    )
    .unwrap();
    let rg = workflow::lock(
        &workflow::VersionOptions::new(git_v01.clone()).with_baseline(Baseline::First),
    )
    .unwrap();
    assert!(rg.is_ok(), "{:?}", rg.report.findings);
    fs::write(&rg.lockfile, rg.rendered.unwrap()).unwrap();
    fs::write(git_wit.join("released"), "v0.1\n").unwrap();
    run_git(&["add", "."]);
    run_git(&["commit", "-q", "-m", "initial release"]);

    // Enable core.ignorecase=true, stage removal of `wit/released`, create empty `wit/RELEASED`,
    // and check with `--released wit/RELEASED`.
    run_git(&["config", "core.ignorecase", "true"]);
    run_git(&["rm", "-q", "wit/released"]);
    fs::write(git_wit.join("RELEASED"), "").unwrap();
    let case_report = witforge::freeze::check_root_with_manifest(
        &git_wit,
        Some(&git_wit.join("RELEASED")),
        &witforge::freeze::FreezeOptions::default(),
    )
    .unwrap();
    assert!(
        !case_report.is_ok(),
        "case-insensitive manifest bypass must be caught by freeze: {case_report:?}"
    );
    assert_eq!(case_report.manifest, "wit/released");
    let _ = fs::remove_file(git_wit.join("RELEASED"));
    run_git(&["reset", "--hard", "-q", "HEAD"]);
    run_git(&["config", "core.ignorecase", "false"]);

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        // Dangling symlink manifest refused by `check_release`.
        let dangling = rel_root.join("dangling-released");
        symlink(rel_root.join("does-not-exist"), &dangling).unwrap();
        let err = workflow::check_release(&v02, Some(&dangling)).unwrap_err();
        let diags = witforge::error_diagnostics(&err).unwrap();
        assert_eq!(diags[0].code, witforge::diagnostic::codes::GUARD_REFUSED);

        // Symlinked `--released` pointing to a regular file in the repo must still be flagged as a
        // symlink violation by `freeze::check_root_with_manifest`.
        let link_manifest = git_wit.join("released-link");
        symlink(git_wit.join("released"), &link_manifest).unwrap();
        let sym_report = witforge::freeze::check_root_with_manifest(
            &git_wit,
            Some(&link_manifest),
            &witforge::freeze::FreezeOptions::default(),
        )
        .unwrap();
        assert!(
            !sym_report.is_ok(),
            "symlinked --released must produce a freeze violation: {sym_report:?}"
        );
        assert!(
            sym_report
                .violations
                .iter()
                .any(|v| v.diagnostic.message.contains("symlink")),
            "expected symlink violation: {:?}",
            sym_report.violations
        );
    }

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn third_wave_audit_freeze_manifest_and_package_location_regressions() {
    use std::process::Command;

    let base = std::env::temp_dir().join(format!(
        "witforge-third-wave-workflow-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();

    // 1. Multi-file package where the first interface is in a file without a `package` header
    //    still resolves `package_location` to the file carrying the `package` declaration.
    let mem_contract = witforge::load_sources(vec![
        witforge::SourceFile::new(
            "a.wit".to_string(),
            "interface alpha { record r { id: u32 } }\n".to_string(),
        ),
        witforge::SourceFile::new(
            "b.wit".to_string(),
            "// comment\npackage ex:multifile@0.1.0;\n".to_string(),
        ),
    ])
    .unwrap();
    let locs = mem_contract.member_locations();
    let (pkg_file, pkg_pos) = locs.get("ex:multifile").expect("package location");
    assert_eq!(pkg_file, "b.wit");
    assert_eq!(pkg_pos.line, 2);

    // 2. `freeze` case-insensitive manifest bypasses (staged/committed case-swap, committed `git rm`
    //    + untracked alternate-case file) and repo-root `--released` pathspec refusal.
    let git_repo = base.join("git-repo");
    let git_wit = git_repo.join("wit");
    let git_v01 = git_wit.join("v0.1");
    fs::create_dir_all(&git_v01).unwrap();
    let run_git = |args: &[&str]| {
        let out = Command::new("git")
            .current_dir(&git_repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run_git(&["init", "-q"]);
    run_git(&["config", "user.name", "Test"]);
    run_git(&["config", "user.email", "test@example.invalid"]);
    run_git(&["config", "commit.gpgsign", "false"]);
    fs::write(
        git_v01.join("contract.wit"),
        "package ex:frz3@0.1.0;\ninterface api { record item { id: u64 } }\n",
    )
    .unwrap();
    let rg = workflow::lock(
        &workflow::VersionOptions::new(git_v01.clone()).with_baseline(Baseline::First),
    )
    .unwrap();
    fs::write(&rg.lockfile, rg.rendered.unwrap()).unwrap();
    fs::write(git_wit.join("released"), "v0.1\n").unwrap();
    run_git(&["add", "."]);
    run_git(&["commit", "-q", "-m", "initial release"]);

    // Repo root passed as `--released` returns clean `IO_ERROR` instead of raw git empty-pathspec.
    let root_err = witforge::freeze::check_root_with_manifest(
        &git_wit,
        Some(&git_repo),
        &witforge::freeze::FreezeOptions::default(),
    )
    .unwrap_err();
    assert_eq!(
        witforge::error_diagnostics(&root_err).unwrap()[0].code,
        witforge::diagnostic::codes::IO_ERROR
    );

    // 2a. Staged case-swap (`git rm wit/released && : > wit/RELEASED && git add wit/RELEASED`).
    run_git(&["config", "core.ignorecase", "true"]);
    run_git(&["rm", "-q", "wit/released"]);
    fs::write(git_wit.join("RELEASED"), "").unwrap();
    run_git(&["add", "wit/RELEASED"]);
    let staged_swap = witforge::freeze::check_root_with_manifest(
        &git_wit,
        Some(&git_wit.join("RELEASED")),
        &witforge::freeze::FreezeOptions::default(),
    )
    .unwrap();
    assert!(
        !staged_swap.is_ok(),
        "staged case-swap must fail freeze: {staged_swap:?}"
    );

    // 2b. Committed case-swap with empty `wit/RELEASED`.
    run_git(&["commit", "-q", "-m", "case swap"]);
    let committed_swap = witforge::freeze::check_root_with_manifest(
        &git_wit,
        Some(&git_wit.join("RELEASED")),
        &witforge::freeze::FreezeOptions::default(),
    )
    .unwrap();
    assert!(
        !committed_swap.is_ok(),
        "committed case-swap must fail freeze: {committed_swap:?}"
    );

    // 2c. Committed `git rm wit/RELEASED` + untracked empty `wit/Released`.
    run_git(&["rm", "-q", "wit/RELEASED"]);
    run_git(&["commit", "-q", "-m", "drop manifest"]);
    fs::write(git_wit.join("Released"), "").unwrap();
    let dropped_swap = witforge::freeze::check_root_with_manifest(
        &git_wit,
        Some(&git_wit.join("Released")),
        &witforge::freeze::FreezeOptions::default(),
    )
    .unwrap();
    assert!(
        !dropped_swap.is_ok(),
        "committed git rm + untracked alternate-case manifest must fail freeze: {dropped_swap:?}"
    );

    // 3. Symlinked and dangling-symlinked `released` manifests are refused by `load_root_released_manifest`
    //    and `ensure_not_released`, and `<symlink>/..` in `--released` does not lexically collapse.
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let sym_root = base.join("sym-root");
        let sym_v01 = sym_root.join("v0.1");
        fs::create_dir_all(&sym_v01).unwrap();
        fs::write(
            sym_v01.join("contract.wit"),
            "package ex:sym@0.1.0;\ninterface api { record item { id: u64 } }\n",
        )
        .unwrap();
        let rs = workflow::lock(
            &workflow::VersionOptions::new(sym_v01.clone()).with_baseline(Baseline::First),
        )
        .unwrap();
        fs::write(&rs.lockfile, rs.rendered.unwrap()).unwrap();

        // Dangling symlink default manifest refused by both `load_root_released_manifest` and
        // `ensure_not_released`.
        symlink(sym_root.join("missing-target"), sym_root.join("released")).unwrap();
        let err_load = witforge::load_root_released_manifest(&sym_root, None).unwrap_err();
        assert_eq!(
            witforge::error_diagnostics(&err_load).unwrap()[0].code,
            witforge::diagnostic::codes::GUARD_REFUSED
        );
        let err_lock = witforge::ensure_not_released(&sym_v01, None).unwrap_err();
        assert_eq!(
            witforge::error_diagnostics(&err_lock).unwrap()[0].code,
            witforge::diagnostic::codes::GUARD_REFUSED
        );

        // `<symlink>/..` pointing outside the repo must be refused as outside repo instead of
        // lexically cancelling into the repo.
        let outside = base.join("outside/deep/sub");
        fs::create_dir_all(&outside).unwrap();
        fs::write(base.join("outside/deep/released"), "v0.1\n").unwrap();
        symlink(&outside, git_repo.join("sub_link")).unwrap();
        let escape_err = witforge::freeze::check_root_with_manifest(
            &git_wit,
            Some(&git_repo.join("sub_link/../released")),
            &witforge::freeze::FreezeOptions::default(),
        )
        .unwrap_err();
        assert_eq!(
            witforge::error_diagnostics(&escape_err).unwrap()[0].code,
            witforge::diagnostic::codes::GUARD_REFUSED
        );
    }

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn fourth_wave_audit_freeze_tree_symlink_path_and_package_location_regressions() {
    use std::process::Command;

    let base =
        std::env::temp_dir().join(format!("witforge-w10-125-workflow-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();

    // 1. Historical directory (`040000 tree`) and symlink (`120000`) at `wit/released` in earlier
    //    commits must NOT be parsed as released manifest entries when replaced by a regular file.
    let repo = base.join("hist-repo");
    let wit = repo.join("wit");
    let v01 = wit.join("v0.1");
    fs::create_dir_all(&v01).unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package ex:hist@0.1.0;\ninterface api { record item { id: u64 } }\n",
    )
    .unwrap();
    let locked =
        workflow::lock(&workflow::VersionOptions::new(v01.clone()).with_baseline(Baseline::First))
            .unwrap();
    fs::write(&locked.lockfile, locked.rendered.unwrap()).unwrap();

    let run_git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run_git(&["init", "-q"]);
    run_git(&["config", "user.name", "witforge-test"]);
    run_git(&["config", "user.email", "witforge-test@example.invalid"]);

    // Commit 1: `wit/released` is a directory (`040000 tree`).
    fs::create_dir_all(wit.join("released")).unwrap();
    fs::write(wit.join("released/placeholder.txt"), "not a manifest\n").unwrap();
    run_git(&["add", "."]);
    run_git(&["commit", "-qm", "dir-at-released"]);

    // Commit 2 (Unix): `wit/released` is a symlink (`120000 blob`).
    #[cfg(unix)]
    {
        fs::remove_dir_all(wit.join("released")).unwrap();
        std::os::unix::fs::symlink("elsewhere", wit.join("released")).unwrap();
        run_git(&["add", "-A"]);
        run_git(&["commit", "-qm", "symlink-at-released"]);
        fs::remove_file(wit.join("released")).unwrap();
    }
    #[cfg(not(unix))]
    {
        fs::remove_dir_all(wit.join("released")).unwrap();
    }

    // Commit 3: `wit/released` becomes a regular manifest listing `v0.1`.
    fs::write(wit.join("released"), "v0.1\n").unwrap();
    run_git(&["add", "-A"]);
    run_git(&["commit", "-qm", "regular-manifest"]);

    let report = witforge::freeze::check_root(&wit, &witforge::freeze::FreezeOptions::default())
        .expect("freeze must ignore historical tree/symlink at wit/released");
    assert!(
        report.is_ok(),
        "unexpected freeze violations: {:?}",
        report.violations
    );

    // 2. `canonicalize_best_effort` resolves `--released wit/deleted_dir/../released` cleanly
    //    instead of failing with `"cannot resolve ancestor"`.
    let report_dotdot = witforge::freeze::check_root_with_manifest(
        &wit,
        Some(&wit.join("deleted_dir/../released")),
        &witforge::freeze::FreezeOptions::default(),
    )
    .expect("freeze must handle missing intermediate `..` in --released");
    assert!(report_dotdot.is_ok(), "{:?}", report_dotdot.violations);

    // 3. `ReleasedManifest::load` with trailing slash on Unix reads the normalized path cleanly.
    #[cfg(unix)]
    {
        let trailing = PathBuf::from(format!("{}/", wit.join("released").display()));
        let loaded = witforge::ReleasedManifest::load(&trailing)
            .expect("ReleasedManifest::load should normalize trailing slash consistently");
        assert!(loaded.contains("v0.1"));
    }

    // 4. `package_location` locates nested packages at their own `package` line (not line 1) and
    //    multi-file roots at the file declaring the root package (not an earlier file's nested
    //    package).
    let nested_contract = witforge::load_sources(vec![witforge::SourceFile::new(
        "contract.wit".to_string(),
        "package ex:root@0.1.0;\n\
         interface a {\n\
             record x { id: u32 }\n\
         }\n\
         package nested:dep@0.1.0 {\n\
             interface b {}\n\
         }\n"
        .to_string(),
    )])
    .unwrap();
    let diags = witforge::subset::check_located_diagnostics(&nested_contract);
    let nested_diag = diags
        .iter()
        .find(|d| d.diagnostic.code == witforge::diagnostic::codes::SUBSET_NESTED_PACKAGE)
        .expect("expected SUBSET_NESTED_PACKAGE diagnostic");
    assert_eq!(
        nested_diag.diagnostic.position.map(|p| p.line),
        Some(5),
        "nested package diagnostic must point at line 5, got {nested_diag:?}"
    );

    let multi_contract = witforge::load_sources(vec![
        witforge::SourceFile::new(
            "a.wit".to_string(),
            "package nested:dep@0.1.0 {\n    interface b {}\n}\n".to_string(),
        ),
        witforge::SourceFile::new(
            "z.wit".to_string(),
            "package ex:root;\ninterface a { record x { id: u32 } }\n".to_string(),
        ),
    ])
    .unwrap();
    let multi_diags = witforge::subset::check_located_diagnostics(&multi_contract);
    let ver_diag = multi_diags
        .iter()
        .find(|d| d.diagnostic.code == witforge::diagnostic::codes::SUBSET_PACKAGE_VERSION)
        .expect("expected SUBSET_PACKAGE_VERSION diagnostic");
    assert_eq!(
        ver_diag.file.as_deref(),
        Some("z.wit"),
        "root package diagnostic must point at z.wit, not a.wit: {ver_diag:?}"
    );

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn fifth_wave_audit_cross_root_manifest_symlink_lock_and_header_comment_regressions() {
    use witforge::diagnostic::codes;
    use witforge::emit::{glue, json_schema, proto, rust};
    use witforge::inspect;

    let base =
        std::env::temp_dir().join(format!("witforge-w10-126-workflow-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();

    // 1. Unclosed block comment ending with `{` on a `package` header line must not misclassify
    //    the top-level package header as a nested `package { ... }` block.
    let unclosed_comment_contract = witforge::load_sources(vec![witforge::SourceFile::new(
        "contract.wit".to_string(),
        "package ex:root /* note {\n\
         */;\n\
         interface a {\n\
             record x { id: u32 }\n\
         }\n"
        .to_string(),
    )])
    .unwrap();
    let diags = witforge::subset::check_located_diagnostics(&unclosed_comment_contract);
    let ver_diag = diags
        .iter()
        .find(|d| d.diagnostic.code == codes::SUBSET_PACKAGE_VERSION)
        .expect("expected SUBSET_PACKAGE_VERSION diagnostic");
    assert_eq!(
        ver_diag.diagnostic.position.map(|p| p.line),
        Some(1),
        "unclosed block comment ending in `{{` on line 1 must still resolve line 1: {ver_diag:?}"
    );

    // 2. Cross-root `--released` manifests must be refused with WF9004 across root commands so a
    //    manifest from `root_a` cannot silently retire or mark versions released in `root_b`.
    let make_two_version_root = |root: &Path, pkg: &str| {
        let v01 = root.join("v0.1");
        let v02 = root.join("v0.2");
        fs::create_dir_all(&v01).unwrap();
        fs::create_dir_all(&v02).unwrap();
        fs::write(
            v01.join("contract.wit"),
            format!("package {pkg}@0.1.0;\ninterface api {{ record item {{ id: u64 }} }}\n"),
        )
        .unwrap();
        let l1 = workflow::lock(
            &workflow::VersionOptions::new(v01.clone()).with_baseline(Baseline::First),
        )
        .unwrap();
        fs::write(&l1.lockfile, l1.rendered.unwrap()).unwrap();
        fs::write(
            v02.join("contract.wit"),
            format!("package {pkg}@0.2.0;\ninterface api {{ record item {{ id: u64 }} }}\n"),
        )
        .unwrap();
        fs::copy(v01.join("wire.lock"), v02.join("wire.lock")).unwrap();
        let l2 = workflow::lock(
            &workflow::VersionOptions::new(v02.clone())
                .with_baseline(Baseline::Against(v01.join("wire.lock"))),
        )
        .unwrap();
        fs::write(&l2.lockfile, l2.rendered.unwrap()).unwrap();
    };
    let root_a = base.join("root-a");
    let root_b = base.join("root-b");
    make_two_version_root(&root_a, "ex:alpha");
    make_two_version_root(&root_b, "ex:beta");
    let foreign_manifest = root_a.join("released");
    fs::write(&foreign_manifest, "v0.1 retired\nv0.2\n").unwrap();

    let assert_guard_refused = |label: &str, err: anyhow::Error| {
        let diag = witforge::error_diagnostics(&err)
            .and_then(|ds| ds.first())
            .unwrap_or_else(|| panic!("{label}: expected structured diagnostic, got {err:#}"));
        assert_eq!(
            diag.code,
            codes::GUARD_REFUSED,
            "{label}: expected WF9004, got {diag:?}"
        );
    };
    assert_guard_refused(
        "rust::plan_with_released",
        rust::plan_with_released(&root_b, false, Some(&foreign_manifest)).unwrap_err(),
    );
    assert_guard_refused(
        "proto::plan_with_released",
        proto::plan_with_released(&root_b, Some(&foreign_manifest)).unwrap_err(),
    );
    assert_guard_refused(
        "json_schema::plan_with_released",
        json_schema::plan_with_released(&root_b, Some(&foreign_manifest)).unwrap_err(),
    );
    assert_guard_refused(
        "glue::plan_with_released",
        glue::plan_with_released(
            &root_b,
            "crate::plain",
            "crate::proto",
            Some(&foreign_manifest),
        )
        .unwrap_err(),
    );
    assert_guard_refused(
        "workflow::check_root",
        workflow::check_root(&root_b, Some(&foreign_manifest)).unwrap_err(),
    );
    assert_guard_refused(
        "workflow::check_release",
        workflow::check_release(&root_b.join("v0.1"), Some(&foreign_manifest)).unwrap_err(),
    );
    assert_guard_refused(
        "inspect::inspect_root_json",
        inspect::inspect_root_json(&root_b, Some(&foreign_manifest)).unwrap_err(),
    );

    // 3. Explicit in-root `--released` retirement-order violation in `load_versions_with_released`
    //    must cite the explicit manifest path rather than `<wit_root>/released`.
    let custom_manifest = root_b.join("custom-released");
    fs::write(&custom_manifest, "v0.1\nv0.2 retired\n").unwrap();
    let retire_err = rust::plan_with_released(&root_b, false, Some(&custom_manifest)).unwrap_err();
    let retire_located = witforge::error_located_diagnostics(&retire_err).unwrap();
    assert_eq!(retire_located[0].diagnostic.code, codes::LOCKFILE_ERROR);
    assert!(
        retire_located[0]
            .file
            .as_deref()
            .is_some_and(|f| f.ends_with("custom-released")),
        "expected diagnostic file to name custom-released, got {:?}",
        retire_located[0]
    );
    let inspect_retire_err =
        inspect::inspect_root_json(&root_b, Some(&custom_manifest)).unwrap_err();
    let inspect_located = witforge::error_located_diagnostics(&inspect_retire_err).unwrap();
    assert!(
        inspect_located[0]
            .file
            .as_deref()
            .is_some_and(|f| f.ends_with("custom-released")),
        "expected inspect diagnostic file to name custom-released, got {:?}",
        inspect_located[0]
    );

    // 4. Symlinked and dangling-symlinked `wire.lock` files must be refused with WF9004 across
    //    every reader/writer (`check`, `check_root`, `lock`, generators, `inspect`, `diff`,
    //    `WireLock::write_atomic`, and `ReleasedManifest::load`).
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        use witforge::diff;

        let sym_root = base.join("sym-lock");
        make_two_version_root(&sym_root, "ex:symlock");
        let v01 = sym_root.join("v0.1");
        let real_lock = base.join("external-wire.lock");
        fs::rename(v01.join("wire.lock"), &real_lock).unwrap();
        symlink(&real_lock, v01.join("wire.lock")).unwrap();

        assert_guard_refused(
            "workflow::check(symlink wire.lock)",
            workflow::check(&workflow::VersionOptions::new(v01.clone())).unwrap_err(),
        );
        assert_guard_refused(
            "workflow::check_root(symlink wire.lock)",
            workflow::check_root(&sym_root, None).unwrap_err(),
        );
        assert_guard_refused(
            "workflow::lock(symlink wire.lock)",
            workflow::lock(
                &workflow::VersionOptions::new(v01.clone()).with_baseline(Baseline::First),
            )
            .unwrap_err(),
        );
        assert_guard_refused(
            "rust::plan(symlink wire.lock)",
            rust::plan(&sym_root).unwrap_err(),
        );
        assert_guard_refused(
            "inspect::inspect_root_json(symlink wire.lock)",
            inspect::inspect_root_json(&sym_root, None).unwrap_err(),
        );
        assert_guard_refused(
            "diff::compare(symlink wire.lock)",
            diff::compare(&v01, &v01).unwrap_err(),
        );
        let parsed =
            witforge::lock::WireLock::parse(&fs::read_to_string(&real_lock).unwrap()).unwrap();
        assert_guard_refused(
            "WireLock::write_atomic(symlink wire.lock)",
            parsed.write_atomic(&v01.join("wire.lock")).unwrap_err(),
        );

        // Dangling symlink `wire.lock` must report WF9004 (not WF9002).
        fs::remove_file(v01.join("wire.lock")).unwrap();
        symlink(base.join("nonexistent-wire.lock"), v01.join("wire.lock")).unwrap();
        fs::write(sym_root.join("released"), "v0.1\n").unwrap();
        assert_guard_refused(
            "ReleasedManifest::load(dangling symlink wire.lock)",
            witforge::ReleasedManifest::load(&sym_root.join("released")).unwrap_err(),
        );
        assert_guard_refused(
            "diff::compare(dangling symlink wire.lock)",
            diff::compare(&v01, &v01).unwrap_err(),
        );
        fs::remove_file(sym_root.join("released")).unwrap();
        assert_guard_refused(
            "workflow::check_root(dangling symlink predecessor wire.lock)",
            workflow::check_root(&sym_root, None).unwrap_err(),
        );
    }

    fs::remove_dir_all(&base).unwrap();
}
