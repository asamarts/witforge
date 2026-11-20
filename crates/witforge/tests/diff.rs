//! Stable diff API coverage for conversion impacts.

use std::fs;
use std::path::Path;

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
    // the new record fields are numbered after them.
    let mut mixed = old_lock
        .render()
        .replace(" enum-case\n", " enum-case reserved\n");
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

fn write_version(root: &Path, name: &str, contract: &str, lock: &str) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("contract.wit"), contract).unwrap();
    fs::write(dir.join("wire.lock"), lock).unwrap();
}

#[test]
fn method_ids_are_compared_and_generators_agree() {
    let root = std::env::temp_dir().join(format!("witforge-diff-methods-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    write_version(
        &root,
        "v0.1",
        "package ex:t@0.1.0;\ninterface api { alpha: func() -> u32; beta: func(); }\n",
        "# witforge wire lockfile v2\n\
         ex:t/api.alpha = 1 method u32\n\
         ex:t/api.beta = 2 method\n",
    );
    write_version(
        &root,
        "v0.2",
        "package ex:t@0.2.0;\ninterface api { alpha: func() -> u32; beta: func(); }\n",
        "# witforge wire lockfile v2\n\
         ex:t/api.alpha = 2 method u32\n\
         ex:t/api.beta = 1 method\n",
    );

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
    write_version(
        &root,
        "v0.1",
        "package ex:t@0.1.0;\ninterface types { record r { a: u32 } }\n",
        "# witforge wire lockfile v2\n\
         ex:t/types/r.a = 1 field u32\n\
         ex:t/types/r.old = 2 field u32 reserved\n",
    );
    write_version(
        &root,
        "v0.2",
        "package ex:t@0.2.0;\ninterface types { record r { a: u32, fresh: u64 } }\n",
        "# witforge wire lockfile v2\n\
         ex:t/types/r.a = 1 field u32\n\
         ex:t/types/r.fresh = 2 field u64\n",
    );

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
