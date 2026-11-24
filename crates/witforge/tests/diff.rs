//! Stable diff API coverage for conversion impacts.

use std::fs;
use std::path::{Path, PathBuf};

use witforge::diff::{ChangeElement, ChangeKind};
use witforge::lock::WireLock;
use witforge::{load, model};

#[test]
fn a_kind_change_is_wire_breaking_even_with_every_old_number_reserved() {
    let base = std::env::temp_dir().join(format!("witforge-diff-kind-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let old = base.join("v0.1");
    let new = base.join("v0.2");
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&new).unwrap();
    fs::write(
        old.join("contract.wit"),
        "package ex:t@0.1.0;\n\
interface types {\n\
  enum thing { one, two }\n\
}\n",
    )
    .unwrap();
    fs::write(
        new.join("contract.wit"),
        "package ex:t@0.2.0;\n\
interface types {\n\
  record thing { fresh-one: u32, fresh-two: string }\n\
}\n",
    )
    .unwrap();

    let old_model = model::build(&load(&old).unwrap()).unwrap();
    let old_members = old_model.members().unwrap();
    let mut old_lock = WireLock::default();
    old_lock.reconcile(&old_members).unwrap();
    fs::write(old.join("wire.lock"), old_lock.render()).unwrap();

    // The lockfile a pre-0.6.0 `lock --against` produced: the old enum cases stay reserved and
    // the new record fields are numbered after them. Drop the old order line: a direct `diff`
    // accepts a missing or stale order, but an order line naming the old members is structurally
    // wrong for the new live set.
    let mut mixed = old_lock.render();
    if let Some(index) = mixed.find("# witforge order v1") {
        mixed.truncate(index);
    }
    let mut mixed = mixed.replace(" enum-case\n", " enum-case reserved\n");
    mixed.push_str(
        "ex:t/types/thing.fresh-one = 3 field u32\n\
ex:t/types/thing.fresh-two = 4 field string\n",
    );
    fs::write(new.join("wire.lock"), mixed).unwrap();

    let report = witforge::diff::compare(&old, &new).unwrap();
    let change = report
        .changes
        .iter()
        .find(|change| matches!(change.kind, ChangeKind::KindChanged { .. }))
        .expect("the enum-to-record change is reported");
    assert!(change.wire_breaking, "{change:?}");
    assert_eq!(report.summary.wire_breaking, 1);

    fs::remove_dir_all(&base).unwrap();
}

fn write_contract(root: &Path, name: &str, contract: &str) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("contract.wit"), contract).unwrap();
    dir
}

fn generated_lock(dir: &Path) -> String {
    let model = model::build(&load(dir).unwrap()).unwrap();
    let mut lock = WireLock::default();
    lock.reconcile(&model.members().unwrap()).unwrap();
    lock.render()
}

#[test]
fn stability_changes_are_reported_without_breaking_any_view() {
    let root = std::env::temp_dir().join(format!("witforge-diff-stability-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let body = |version: &str, deprecated: bool| {
        let deprecation = if deprecated {
            format!("@deprecated(version = {version})\n")
        } else {
            String::new()
        };
        format!(
            "package ex:t@{version};\n\
             @since(version = 0.1.0)\n\
             {deprecation}interface api {{\n\
                 @since(version = 0.1.0)\n\
                 {deprecation}record item {{ value: string }}\n\
                 @since(version = 0.1.0)\n\
                 {deprecation}get: func() -> item;\n\
             }}\n\
             @since(version = 0.1.0)\n\
             {deprecation}world plugin {{\n\
                 @since(version = 0.1.0)\n\
                 {deprecation}export api;\n\
             }}\n"
        )
    };
    let old = write_contract(&root, "v0.1", &body("0.1.0", false));
    let deprecated = write_contract(&root, "v0.2", &body("0.2.0", true));
    let restored = write_contract(&root, "v0.3", &body("0.3.0", false));
    let baseline_lock = generated_lock(&old);
    for directory in [&old, &deprecated, &restored] {
        let lock = generated_lock(directory);
        assert_eq!(lock, baseline_lock, "stability changed wire.lock");
        fs::write(directory.join("wire.lock"), lock).unwrap();
    }

    for (from, to) in [(&old, &deprecated), (&deprecated, &restored)] {
        let report = witforge::diff::compare(from, to).unwrap();
        assert_eq!(report.changes.len(), 5, "{report:#?}");
        assert!(report.changes.iter().all(|change| {
            matches!(change.kind, ChangeKind::StabilityChanged { .. })
                && !change.wire_breaking
                && !change.source_breaking
                && !change.api_breaking
        }));
        assert_eq!(report.summary.breaking, 0);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn method_ids_are_compared_and_generators_agree() {
    let root = std::env::temp_dir().join(format!("witforge-diff-methods-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let old = write_contract(
        &root,
        "v0.1",
        "package ex:t@0.1.0;\ninterface api { alpha: func() -> u32; beta: func(); }\n",
    );
    fs::write(old.join("wire.lock"), generated_lock(&old)).unwrap();
    let new = write_contract(
        &root,
        "v0.2",
        "package ex:t@0.2.0;\ninterface api { alpha: func() -> u32; beta: func(); }\n",
    );
    let new_lock = generated_lock(&new).replace(
        "ex:t/api.alpha = 1 method u32\nex:t/api.beta = 2 method",
        "ex:t/api.alpha = 2 method u32\nex:t/api.beta = 1 method",
    );
    fs::write(new.join("wire.lock"), new_lock).unwrap();

    let report = witforge::diff::compare(&root.join("v0.1"), &root.join("v0.2")).unwrap();
    let renumbered: Vec<_> = report
        .changes
        .iter()
        .filter(|change| {
            matches!(change.kind, ChangeKind::Renumbered { .. })
                && change.element == ChangeElement::MethodId
        })
        .collect();
    assert_eq!(renumbered.len(), 2, "{report:?}");
    assert!(renumbered.iter().all(|change| change.wire_breaking));
    assert_eq!(report.summary.wire_breaking, 2);

    let error = witforge::emit::proto::plan(&root).unwrap_err();
    assert!(witforge::is_contract_error(&error), "{error:#}");
    assert!(format!("{error:#}").contains("renumbered"), "{error:#}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn reuse_of_a_reserved_number_is_wire_breaking_and_generators_agree() {
    let root = std::env::temp_dir().join(format!(
        "witforge-diff-reserved-reuse-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let old = write_contract(
        &root,
        "v0.1",
        "package ex:t@0.1.0;\ninterface types { record r { a: u32 } }\n",
    );
    let mut old_lock = generated_lock(&old);
    old_lock.push_str("ex:t/types/r.old = 2 field u32 reserved\n");
    fs::write(old.join("wire.lock"), old_lock).unwrap();
    let new = write_contract(
        &root,
        "v0.2",
        "package ex:t@0.2.0;\ninterface types { record r { a: u32, fresh: u64 } }\n",
    );
    fs::write(new.join("wire.lock"), generated_lock(&new)).unwrap();

    let report = witforge::diff::compare(&root.join("v0.1"), &root.join("v0.2")).unwrap();
    let added = report
        .changes
        .iter()
        .find(|change| change.path == "types/r.fresh")
        .expect("the added field is reported");
    assert!(matches!(added.kind, ChangeKind::Added { .. }), "{added:?}");
    assert!(added.wire_breaking, "{added:?}");
    let dropped_reserved = report
        .changes
        .iter()
        .find(|change| {
            change.path == "types/r.old" && change.element == ChangeElement::ReservedEntry
        })
        .expect("the deleted reserved entry is reported");
    assert_eq!(dropped_reserved.kind, ChangeKind::Removed);
    assert!(dropped_reserved.wire_breaking, "{dropped_reserved:?}");
    assert_eq!(report.summary.wire_breaking, 2);

    let error = witforge::emit::proto::plan(&root).unwrap_err();
    assert!(witforge::is_contract_error(&error), "{error:#}");
    assert!(format!("{error:#}").contains("removed"), "{error:#}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn dependency_alias_shapes_diff_with_their_own_version_and_an_evolution() {
    let root = std::env::temp_dir().join(format!("witforge-diff-dep-alias-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dependency = "package example:shared@0.1.0;\n\
         interface types {\n\
           record item { id: u64 }\n\
           enum state { on, off }\n\
           variant choice { none, some(item) }\n\
           flags flags-t { read, write }\n\
           type alias-item = item;\n\
           type alias-chain = alias-item;\n\
           type alias-state = state;\n\
           type alias-choice = choice;\n\
           type alias-flags = flags-t;\n\
           type list-item = list<item>;\n\
           type maybe-item = option<item>;\n\
           type result-item = result<item, string>;\n\
         }\n";
    for version in ["v0.1", "v0.2"] {
        let shared = root.join(version).join("deps/shared");
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("types.wit"), dependency).unwrap();
    }
    let body = "interface api {\n\
         use example:shared/types@0.1.0.{alias-item, alias-chain, alias-state, alias-choice, \
         alias-flags, list-item, maybe-item, result-item, item as local-item};\n\
         record item { local: u32 }\n\
         record r {\n\
           a: alias-item, b: alias-chain, c: alias-state, d: alias-choice, e: alias-flags,\n\
           f: list-item, g: maybe-item, h: result-item, i: local-item\n\
         }\n\
         get: func(input: alias-item) -> alias-item;\n\
       }\n";
    let old = write_contract(
        &root,
        "v0.1",
        &format!("package example:root@0.1.0;\n{body}"),
    );
    fs::write(old.join("wire.lock"), generated_lock(&old)).unwrap();
    let new = write_contract(
        &root,
        "v0.2",
        &format!("package example:root@0.2.0;\n{body}")
            .replace("i: local-item\n", "i: local-item, added: option<u32>\n"),
    );
    fs::write(new.join("wire.lock"), generated_lock(&new)).unwrap();

    let same = witforge::diff::compare(&old, &old).unwrap();
    assert_eq!(same.summary.changes, 0, "{same:?}");
    let evolved = witforge::diff::compare(&old, &new).unwrap();
    assert!(
        evolved
            .changes
            .iter()
            .any(|change| change.path == "api/r.added"),
        "{evolved:?}"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn diff_reads_the_released_manifest_beside_a_version_directory() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "witforge-diff-released-manifest-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let version = write_contract(
        &root,
        "v0.1",
        "package example:root@0.1.0;\ninterface api { record r { a: u32 } }\n",
    );
    fs::write(version.join("wire.lock"), generated_lock(&version)).unwrap();
    fs::write(root.join("released"), "v0.1\n").unwrap();
    let real = root.join("real.lock");
    fs::rename(version.join("wire.lock"), &real).unwrap();
    symlink(&real, version.join("wire.lock")).unwrap();

    let error = witforge::diff::compare(&version, &version).unwrap_err();
    assert!(format!("{error:#}").contains("symlink"), "{error:#}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn direct_diff_accepts_a_stale_order_but_refuses_a_named_non_member() {
    let root = std::env::temp_dir().join(format!(
        "witforge-diff-order-structure-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let old = write_contract(
        &root,
        "v0.1",
        "package ex:t@0.1.0;\ninterface types { record r { a: u32, b: u32 } }\n",
    );
    let old_lock = generated_lock(&old);
    fs::write(old.join("wire.lock"), &old_lock).unwrap();
    let new = write_contract(
        &root,
        "v0.2",
        "package ex:t@0.2.0;\ninterface types { record r { b: u32, a: u32 } }\n",
    );
    // A stale order still names exactly the live members, so direct diff may report the reorder.
    fs::write(new.join("wire.lock"), &old_lock).unwrap();
    let report = witforge::diff::compare(&old, &new).unwrap();
    assert!(
        report
            .changes
            .iter()
            .any(|change| matches!(change.kind, ChangeKind::Reordered { .. })),
        "{report:?}"
    );

    // A line naming a non-member is structurally wrong, not stale.
    let extra = write_contract(
        &root,
        "extra",
        "package ex:t@0.1.0;\ninterface types { record r { a: u32, b: u32, ghost: u32 } }\n",
    );
    let lock = generated_lock(&extra).replace("ex:t/types/r.ghost = 3 field u32\n", "");
    fs::write(new.join("wire.lock"), lock).unwrap();
    let error = witforge::diff::compare(&old, &new).unwrap_err();
    assert!(format!("{error:#}").contains("order line"), "{error:#}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn compare_with_impacts_matches_compare_on_the_versions_fixture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/versions/wit");
    let old = root.join("v0.1");
    let new = root.join("v0.2");

    let report = witforge::diff::compare(&old, &new).unwrap();
    let analysis = witforge::diff::compare_with_impacts(&old, &new).unwrap();

    assert_eq!(analysis.report, report);
    assert!(analysis.conversion_impacts.iter().any(|impact| {
        impact.type_path == "types/user"
            && impact.missing_member == "old-field"
            && impact.missing_path == "types/user.old-field"
            && impact.from_version == analysis.report.new.version
            && impact.to_version == analysis.report.old.version
    }));
}

#[test]
fn decode_verdicts_cover_retypes_kind_changes_renumbering_and_interfaces() {
    use witforge::diff::DecodeVerdict;

    let root = std::env::temp_dir().join(format!(
        "witforge-diff-decode-matrix-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let old = write_contract(
        &root,
        "v0.1",
        "package ex:dec@0.1.0;\n\
         interface types {\n\
           record r { a: u32, b: u32 }\n\
           enum k { one, two }\n\
           type ids = list<u32>;\n\
           record gone { id: u32 }\n\
         }\n\
         interface old-api { ping: func(); }\n",
    );
    let old_lock = generated_lock(&old);
    fs::write(old.join("wire.lock"), &old_lock).unwrap();

    let new = write_contract(
        &root,
        "v0.2",
        "package ex:dec@0.2.0;\n\
         interface types {\n\
           record r { a: string, b: u32 }\n\
           record k { x: u32 }\n\
           type ids = list<u64>;\n\
         }\n\
         interface new-types { record fresh { id: u32 } }\n\
         interface new-api { call: func(); }\n",
    );
    // Reconcile fresh to trigger retype + kind change + renumber + interface add/remove.
    let mut new_lock = generated_lock(&new);
    new_lock = new_lock.replace(
        "ex:dec/types/r.b = 2 field u32",
        "ex:dec/types/r.b = 9 field u32",
    );
    fs::write(new.join("wire.lock"), new_lock).unwrap();

    let full = witforge::diff::compare_full(&old, &new).unwrap();
    let find = |path: &str, element: ChangeElement| {
        full.analysis
            .report
            .changes
            .iter()
            .zip(full.decode_verdicts.iter())
            .find(|(change, _)| change.path == path && change.element == element)
            .map_or_else(
                || panic!("missing ({path}, {element})"),
                |(_, verdict)| *verdict,
            )
    };

    assert_eq!(
        find("types/r.a", ChangeElement::RecordField),
        DecodeVerdict::INCOMPATIBLE
    );
    assert_eq!(
        find("types/r.b", ChangeElement::RecordField),
        DecodeVerdict::INCOMPATIBLE
    );
    assert_eq!(
        find("types/k", ChangeElement::Type),
        DecodeVerdict::INCOMPATIBLE
    );
    assert_eq!(
        find("types/ids", ChangeElement::Type),
        DecodeVerdict::INCOMPATIBLE
    );
    assert_eq!(
        find("new-types", ChangeElement::Interface),
        DecodeVerdict::COMPATIBLE
    );
    assert_eq!(
        find("new-api", ChangeElement::Interface),
        DecodeVerdict {
            new_reads_old: true,
            old_reads_new: false,
        }
    );
    assert_eq!(
        find("types/gone", ChangeElement::Type),
        DecodeVerdict {
            new_reads_old: false,
            old_reads_new: true,
        }
    );
    assert_eq!(
        find("old-api", ChangeElement::Interface),
        DecodeVerdict {
            new_reads_old: false,
            old_reads_new: true,
        }
    );

    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn live_number_reuse_and_reserved_entry_regressions_are_detected_within_major() {
    use witforge::diff::DecodeVerdict;

    let root = std::env::temp_dir().join(format!(
        "witforge-diff-reuse-and-reserved-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);

    // 1. Live wire-number reuse within the same major:
    //    old has `old: option<u32>` at number 1; new drops it without reserving and puts
    //    `fresh: option<string>` at number 1. Both wire and decode must report BREAKING(both).
    let old_live = write_contract(
        &root,
        "live/v0.1",
        "package ex:reuse@0.1.0;\ninterface types { record r { old: option<u32> } }\n",
    );
    fs::write(old_live.join("wire.lock"), generated_lock(&old_live)).unwrap();
    let new_live = write_contract(
        &root,
        "live/v0.2",
        "package ex:reuse@0.2.0;\ninterface types { record r { fresh: option<string> } }\n",
    );
    fs::write(new_live.join("wire.lock"), generated_lock(&new_live)).unwrap();

    let full = witforge::diff::compare_full(&old_live, &new_live).unwrap();
    let find = |path: &str| {
        full.analysis
            .report
            .changes
            .iter()
            .zip(full.decode_verdicts.iter())
            .find(|(c, _)| c.path == path)
            .expect(path)
    };
    let (added, added_decode) = find("types/r.fresh");
    assert!(added.wire_breaking, "{added:?}");
    assert_eq!(*added_decode, DecodeVerdict::INCOMPATIBLE);
    let (removed, removed_decode) = find("types/r.old");
    assert!(removed.wire_breaking, "{removed:?}");
    assert_eq!(*removed_decode, DecodeVerdict::INCOMPATIBLE);
    assert_eq!(full.decode_summary.breaking, 2);

    // 2. Deleted and altered same-major `reserved` lockfile entries are reported as
    //    `ChangeElement::ReservedEntry` with `wire_breaking: true`.
    let old_res = write_contract(
        &root,
        "res/v0.1",
        "package ex:res@0.1.0;\ninterface types { record r { a: u32 } }\n",
    );
    let mut old_res_lock = generated_lock(&old_res);
    old_res_lock.push_str(
        "ex:res/types/r.gone = 2 field u32 reserved\n\
         ex:res/types/r.altered = 3 field u32 reserved\n",
    );
    fs::write(old_res.join("wire.lock"), &old_res_lock).unwrap();

    let new_res = write_contract(
        &root,
        "res/v0.2",
        "package ex:res@0.2.0;\ninterface types { record r { a: u32 } }\n",
    );
    let mut new_res_lock = generated_lock(&new_res);
    // Delete `gone`, renumber and retype `altered` (3 field u32 -> 4 field string).
    new_res_lock.push_str("ex:res/types/r.altered = 4 field string reserved\n");
    fs::write(new_res.join("wire.lock"), &new_res_lock).unwrap();

    let res_report = witforge::diff::compare(&old_res, &new_res).unwrap();
    assert!(
        res_report.changes.iter().any(|c| {
            c.element == ChangeElement::ReservedEntry
                && c.path == "types/r.gone"
                && c.kind == ChangeKind::Removed
                && c.number == Some(2)
                && c.wire_breaking
                && !c.source_breaking
                && !c.api_breaking
        }),
        "{res_report:#?}"
    );
    assert!(
        res_report.changes.iter().any(|c| {
            c.element == ChangeElement::ReservedEntry
                && c.path == "types/r.altered"
                && c.kind == ChangeKind::Renumbered { from: 3, to: 4 }
                && c.wire_breaking
        }),
        "{res_report:#?}"
    );
    assert!(
        res_report.changes.iter().any(|c| {
            c.element == ChangeElement::ReservedEntry
                && c.path == "types/r.altered"
                && c.kind
                    == ChangeKind::Retyped {
                        from: "field u32".to_string(),
                        to: "field string".to_string(),
                    }
                && c.wire_breaking
        }),
        "{res_report:#?}"
    );
    assert_eq!(res_report.summary.wire_breaking, 3);

    // 3. Cross-major diff treats the new major as an independent wire namespace: dropping prior
    //    reserved entries in v1.0 (--first) does not report reserved-entry regressions.
    let v10 = write_contract(
        &root,
        "res/v1.0",
        "package ex:res@1.0.0;\ninterface types { record r { a: u32 } }\n",
    );
    fs::write(v10.join("wire.lock"), generated_lock(&v10)).unwrap();
    let cross = witforge::diff::compare(&old_res, &v10).unwrap();
    assert_eq!(cross.summary.changes, 0, "{cross:#?}");

    // 4. `preserves` checks `kind` and `ty` on reserved entries, and dropping a reserved entry
    //    whose number is reused by a live member reports `DecodeVerdict::INCOMPATIBLE`.
    let old_p = write_contract(
        &root,
        "pres/v0.1",
        "package ex:pres@0.1.0;\ninterface types { record r { keep: u32, dropme: option<u32> } }\n",
    );
    fs::write(old_p.join("wire.lock"), generated_lock(&old_p)).unwrap();
    let new_p = write_contract(
        &root,
        "pres/v0.2",
        "package ex:pres@0.2.0;\ninterface types { record r { keep: u32 } }\n",
    );
    let mut new_p_lock = generated_lock(&new_p);
    // Tamper the preserved reserved entry's type (`option<u32>` -> `string`).
    new_p_lock.push_str("ex:pres/types/r.dropme = 2 field string reserved\n");
    fs::write(new_p.join("wire.lock"), new_p_lock).unwrap();
    let tampered = witforge::diff::compare(&old_p, &new_p).unwrap();
    let removed_field = tampered
        .changes
        .iter()
        .find(|c| c.path == "types/r.dropme")
        .expect("removed field");
    assert!(
        removed_field.wire_breaking,
        "tampered reserved type must fail preserves(): {removed_field:?}"
    );

    let old_hijack = write_contract(
        &root,
        "hijack/v0.1",
        "package ex:hijack@0.1.0;\ninterface types { record r { keep: u32 } }\n",
    );
    let mut old_hijack_lock = generated_lock(&old_hijack);
    old_hijack_lock.push_str("ex:hijack/types/r.gone = 2 field u32 reserved\n");
    fs::write(old_hijack.join("wire.lock"), old_hijack_lock).unwrap();
    let new_hijack = write_contract(
        &root,
        "hijack/v0.2",
        "package ex:hijack@0.2.0;\ninterface types { record r { keep: u32, hijack: option<string> } }\n",
    );
    // Fresh lock assigns `hijack = 2`, reusing the dropped reserved number 2.
    fs::write(new_hijack.join("wire.lock"), generated_lock(&new_hijack)).unwrap();
    let hijack_full = witforge::diff::compare_full(&old_hijack, &new_hijack).unwrap();
    let (res_change, res_decode) = hijack_full
        .analysis
        .report
        .changes
        .iter()
        .zip(hijack_full.decode_verdicts.iter())
        .find(|(c, _)| c.element == ChangeElement::ReservedEntry && c.path == "types/r.gone")
        .expect("removed reserved entry");
    assert!(res_change.wire_breaking, "{res_change:?}");
    assert_eq!(
        *res_decode,
        DecodeVerdict::INCOMPATIBLE,
        "reusing a dropped reserved wire number must break decode: {hijack_full:#?}"
    );
    assert!(hijack_full.decode_summary.breaking > 0);

    fs::remove_dir_all(&root).unwrap();
}
