//! End-to-end tests over the fixture contracts.

use std::fs;
use std::path::Path;

use witforge::lock::WireLock;
use witforge::{load, model, subset};

fn fixtures() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

fn fixture(name: &str) -> witforge::Contract {
    load(&fixtures().join(name)).unwrap()
}

#[test]
fn valid_contract_passes_the_subset() {
    let violations = subset::check(&fixture("ok"));
    assert!(violations.is_empty(), "{violations:#?}");
}

/// Each directory under `fixtures/refused` holds one refused construct and either
/// `expected.txt` (the exact violations, one per line) or `parse-error.txt` (text the load error
/// must contain, for constructs `wit-parser` already rejects).
#[test]
fn every_refused_construct_is_reported() {
    let mut cases = 0;
    for entry in fs::read_dir(fixtures().join("refused")).unwrap() {
        let dir = entry.unwrap().path();
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        if let Ok(expected) = fs::read_to_string(dir.join("parse-error.txt")) {
            let err = format!("{:#}", load(&dir).unwrap_err());
            assert!(err.contains(expected.trim()), "{name}: {err}");
        } else {
            let expected = fs::read_to_string(dir.join("expected.txt"))
                .unwrap_or_else(|_| panic!("{name}: expected.txt or parse-error.txt is missing"));
            let actual: Vec<String> = subset::check(&load(&dir).unwrap())
                .iter()
                .map(ToString::to_string)
                .collect();
            assert_eq!(actual, expected.lines().collect::<Vec<_>>(), "{name}");
        }
        cases += 1;
    }
    assert!(
        cases >= 18,
        "only {cases} refused-construct fixtures were found"
    );
}

#[test]
fn members_are_keyed_without_the_version_and_skip_aliases() {
    let keys: Vec<String> = model::members(&fixture("ok"))
        .unwrap()
        .iter()
        .map(model::Member::key)
        .collect();
    assert!(keys.contains(&"example:plugin/types/span.start".to_string()));
    assert!(keys.contains(&"example:plugin/types/edit.delete-file".to_string()));
    assert!(keys.contains(&"example:plugin/types/caps.git".to_string()));
    // `use types.{finding}` in `analyzer` is an alias, numbered only where it is defined.
    assert!(
        !keys
            .iter()
            .any(|k| k.starts_with("example:plugin/analyzer/")),
        "{keys:#?}"
    );
}

#[test]
fn lock_is_append_only_across_changes() {
    let members = model::members(&fixture("ok")).unwrap();
    let mut lock = WireLock::default();
    let first = lock.reconcile(&members).unwrap();
    assert_ne!(
        first.added.len(),
        0,
        "the first reconcile must number every member"
    );
    assert!(
        lock.reconcile(&members).unwrap().is_empty(),
        "second reconcile must be a no-op"
    );
    let start = lock.get("example:plugin/types/span.start").unwrap().number;

    // Remove `span.end`, add `span.line`.
    let mut edited: Vec<_> = members
        .iter()
        .filter(|m| m.key() != "example:plugin/types/span.end")
        .cloned()
        .collect();
    edited.push(model::Member {
        parent: "example:plugin/types/span".into(),
        name: "line".into(),
        kind: model::MemberKind::Field,
        ty: Some("u64".into()),
    });
    let older = lock.clone();
    let changes = lock.reconcile(&edited).unwrap();
    assert_eq!(
        changes.reserved,
        vec![("example:plugin/types/span.end".to_string(), 2)]
    );
    assert_eq!(
        changes.added,
        vec![("example:plugin/types/span.line".to_string(), 3)]
    );
    assert_eq!(
        lock.get("example:plugin/types/span.start").unwrap().number,
        start
    );
    lock.check_append_only(&older).unwrap();

    // The rendered file round-trips.
    assert_eq!(WireLock::parse(&lock.render()).unwrap(), lock);
}

#[test]
fn a_reintroduced_member_stays_reserved_and_is_reported() {
    let members = model::members(&fixture("ok")).unwrap();
    let mut lock = WireLock::default();
    lock.reconcile(&members).unwrap();
    let without_end: Vec<_> = members
        .iter()
        .filter(|m| m.key() != "example:plugin/types/span.end")
        .cloned()
        .collect();
    lock.reconcile(&without_end).unwrap();
    let older = lock.clone();

    // `span.end` comes back under its old name.
    let changes = lock.reconcile(&members).unwrap();
    let expected = vec![("example:plugin/types/span.end".to_string(), 2)];
    assert_eq!(changes.reintroduced, expected);
    assert!(changes.added.is_empty() && changes.reserved.is_empty());
    assert!(!changes.is_empty());
    assert_eq!(lock.drift(&members).unwrap().reintroduced, expected);
    let end = lock.get("example:plugin/types/span.end").unwrap();
    assert!(end.reserved && end.number == 2);
    assert_eq!(lock, older);
}

#[test]
fn append_only_check_refuses_renumbering_and_deletion() {
    let older = WireLock::parse("a:b/i/t.x = 1\na:b/i/t.y = 2 reserved\n").unwrap();
    let renumbered = WireLock::parse("a:b/i/t.x = 3\na:b/i/t.y = 2 reserved\n").unwrap();
    assert!(renumbered.check_append_only(&older).is_err());
    let deleted = WireLock::parse("a:b/i/t.x = 1\n").unwrap();
    assert!(deleted.check_append_only(&older).is_err());
    let unreserved = WireLock::parse("a:b/i/t.x = 1\na:b/i/t.y = 2\n").unwrap();
    assert!(unreserved.check_append_only(&older).is_err());
}

#[test]
fn number_assignment_skips_the_protobuf_reserved_range_and_stops_at_the_maximum() {
    // 18999 is the last usable number before protobuf's reserved range.
    let mut lock = WireLock::parse("a:b/i/t.x = 18999\n").unwrap();
    let member = model::Member {
        parent: "a:b/i/t".into(),
        name: "y".into(),
        kind: model::MemberKind::Field,
        ty: Some("u64".into()),
    };
    let changes = lock.reconcile(&[member]).unwrap();
    assert_eq!(changes.added, vec![("a:b/i/t.y".to_string(), 20_000)]);
    assert_eq!(lock.get("a:b/i/t.y").unwrap().number, 20_000);

    // The last valid field number cannot be followed by another member in the same parent.
    let mut full = WireLock::parse("a:b/i/t.x = 536870911\n").unwrap();
    let member = model::Member {
        parent: "a:b/i/t".into(),
        name: "y".into(),
        kind: model::MemberKind::Field,
        ty: Some("u64".into()),
    };
    let err = full.reconcile(&[member]).unwrap_err();
    assert!(format!("{err:#}").contains("no protobuf field numbers left"));
    assert!(full.get("a:b/i/t.y").is_none());
}

#[test]
fn parse_rejects_bad_lockfiles() {
    for bad in [
        "a:b/i/t.x = 1\na:b/i/t.x = 2\n", // duplicate key
        "a:b/i/t.x = 1\na:b/i/t.y = 1\n", // shared number in one parent
        "a:b/i/t.x = 19500\n",            // protobuf-reserved range
        "a:b/i/t.x = 0\n",                // zero
        "a:b/i/t.x = 1 maybe\n",          // unknown flag
        "nonsense\n",                     // not key = value
    ] {
        assert!(WireLock::parse(bad).is_err(), "{bad:?} should fail");
    }
}
