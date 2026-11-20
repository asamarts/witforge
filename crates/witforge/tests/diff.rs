//! Stable diff API coverage for conversion impacts.

use std::fs;
use std::path::{Path, PathBuf};

use witforge::diff::ChangeKind;
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
            matches!(change.kind, ChangeKind::Renumbered { .. }) && change.element == "method id"
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
    assert_eq!(report.summary.wire_breaking, 1);

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
