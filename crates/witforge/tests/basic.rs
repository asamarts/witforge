//! End-to-end tests over the fixture contracts.

use std::path::Path;

use witforge::lock::WireLock;
use witforge::{load, model, subset};

fn fixture(name: &str) -> witforge::Contract {
    load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn valid_contract_passes_the_subset() {
    let violations = subset::check(&fixture("ok"));
    assert!(violations.is_empty(), "{violations:#?}");
}

#[test]
fn invalid_contract_reports_every_construct() {
    let messages: Vec<String> = subset::check(&fixture("bad"))
        .iter()
        .map(ToString::to_string)
        .collect();
    let has = |needle: &str| messages.iter().any(|m| m.contains(needle));
    assert!(has("needs a version"), "{messages:#?}");
    assert!(has("resources and handles"), "{messages:#?}");
    assert!(has("tuples are not supported"), "{messages:#?}");
    assert!(has("`char` is not supported"), "{messages:#?}");
    assert!(has("`map` is not supported"), "{messages:#?}");
    assert!(has("not directly in a world"), "{messages:#?}");
}

#[test]
fn members_are_keyed_without_the_version_and_skip_aliases() {
    let keys: Vec<String> = model::members(&fixture("ok"))
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
    let members = model::members(&fixture("ok"));
    let mut lock = WireLock::default();
    let first = lock.reconcile(&members);
    assert_ne!(
        first.added.len(),
        0,
        "the first reconcile must number every member"
    );
    assert!(
        lock.reconcile(&members).is_empty(),
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
    });
    let older = lock.clone();
    let changes = lock.reconcile(&edited);
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
