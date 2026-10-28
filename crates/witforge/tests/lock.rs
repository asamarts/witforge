//! Typed lockfile v2 parsing, rendering, keys, and entry-matrix tests.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use witforge::lock::{HEADER, LockMode, MemberKey, WireLock};
use witforge::model::{self, Member, MemberKind};
use witforge::{ReleasedManifest, ensure_not_released, load, load_released_manifest};

#[test]
fn v2_round_trip_sorts_and_renders_typed_entries() {
    let text = format!(
        "{HEADER}\n\
a:b/i/t.y = 2 field u64 reserved\n\
a:b/i/t.x = 3 field u64\n\
a:b/i.f = 1 method result<list<types/span>,string>\n\
a:b/i#f.p = 1 param types/span\n\
a:b/i/e.c = 2 enum-case\n\
a:b/i/fl.flag = 1 flag\n\
a:b/i/v.c = 1 variant-case types/span\n\
a:b/i/v.n = 2 variant-case\n"
    );
    let lock = WireLock::parse(&text).unwrap();
    assert_eq!(
        lock.render(),
        format!(
            "{HEADER}\n\
a:b/i#f.p = 1 param types/span\n\
a:b/i.f = 1 method result<list<types/span>,string>\n\
a:b/i/e.c = 2 enum-case\n\
a:b/i/fl.flag = 1 flag\n\
a:b/i/t.x = 3 field u64\n\
a:b/i/t.y = 2 field u64 reserved\n\
a:b/i/v.c = 1 variant-case types/span\n\
a:b/i/v.n = 2 variant-case\n"
        )
    );
    let entry = lock.get("a:b/i.f").unwrap();
    assert_eq!(entry.number, 1);
    assert_eq!(entry.kind, MemberKind::Method);
    assert_eq!(entry.ty.as_deref(), Some("result<list<types/span>,string>"));
    let reserved = lock.get("a:b/i/t.y").unwrap();
    assert!(reserved.reserved);
    assert_eq!(reserved.kind, MemberKind::Field);
    assert_eq!(reserved.ty.as_deref(), Some("u64"));
    assert_eq!(WireLock::parse(&lock.render()).unwrap(), lock);
}

#[test]
fn member_keys_parse_and_render_typed_scopes() {
    let type_key = MemberKey::parse("a:b/i/t.x").unwrap();
    assert_eq!(type_key.key(), "a:b/i/t.x");
    assert_eq!(type_key.parent(), "a:b/i/t");
    let method = MemberKey::parse("a:b/i.f").unwrap();
    assert_eq!(method.key(), "a:b/i.f");
    assert_eq!(method.parent(), "a:b/i");
    let parameter = MemberKey::parse("a:b/i#f.p").unwrap();
    assert_eq!(parameter.key(), "a:b/i#f.p");
    assert_eq!(parameter.parent(), "a:b/i#f");

    for bad in [
        "",
        "a:b/i",
        "a:b/i/",
        "a:b/i/t",
        "a:b/i/t.",
        "a:b/i/.x",
        "a:b/i#f",
        "a:b/i#f.",
        "a:b/i#.p",
        "a:b/i/t.x.y",
        "a:b/i#f.p.extra",
        "a:b/i/t.x/y",
        "a:b/i/t.x#y",
        "a:b:/i/t.x",
        "a:b/i/t.x ",
        "aB:b/i/t.x",
        "a:b/i/T_x.x",
        "a:b/i/t.x_y",
        "a:b/i/t.x-y-",
    ] {
        assert!(MemberKey::parse(bad).is_err(), "{bad:?} should fail");
    }
}

#[test]
fn v2_rejects_v1_and_malformed_entries() {
    for bad in [
        "# witforge wire lockfile: append-only. Do not edit numbers by hand.\na:b/i/t.x = 1\n"
            .to_string(),
        "a:b/i/t.x = 1 field u64\n".to_string(),
        format!("{HEADER}\na:b/i/t.x = 1 field\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field u64 extra\n"),
        format!("{HEADER}\na:b/i/t.x = 1 enum-case u64\n"),
        format!("{HEADER}\na:b/i/t.x = 1 param\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field char\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field list<>\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field result<u8>\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field result<_,_> extra\n"),
        format!("{HEADER}\na:b/i/t.x = 1 unknown u64\n"),
        format!("{HEADER}\na:b/i/t.x = 01 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = +1 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = -1 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 1_0 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 0x1 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.x = 2 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.y = 1 field u64\n"),
        format!("{HEADER}\nnonsense\n"),
    ] {
        assert!(WireLock::parse(&bad).is_err(), "{bad:?} should fail");
    }
}

#[test]
fn canonical_type_tokens_accept_the_supported_grammar() {
    for good in [
        "bool",
        "u8",
        "u16",
        "u32",
        "u64",
        "s8",
        "s16",
        "s32",
        "s64",
        "f32",
        "f64",
        "string",
        "list<u8>",
        "list<option<result<u8,string>>>",
        "option<string>",
        "result<_,_>",
        "result<list<u8>,types/span>",
        "types/span",
    ] {
        let text = format!("{HEADER}\na:b/i/t.x = 1 field {good}\n");
        assert!(WireLock::parse(&text).is_ok(), "{good:?} should parse");
    }
    for bad in ["types/SpAn", "types/span_x", "types/span-", "types//span"] {
        let text = format!("{HEADER}\na:b/i/t.x = 1 field {bad}\n");
        assert!(WireLock::parse(&text).is_err(), "{bad:?} should fail");
    }
}

#[test]
fn reconcile_records_and_checks_kind_and_type() {
    let members = [
        Member {
            parent: "a:b/i/t".into(),
            name: "x".into(),
            kind: MemberKind::Field,
            ty: Some("u64".into()),
        },
        Member {
            parent: "a:b/i".into(),
            name: "f".into(),
            kind: MemberKind::Method,
            ty: Some("result<_,_>".into()),
        },
        Member {
            parent: "a:b/i#f".into(),
            name: "p".into(),
            kind: MemberKind::Param,
            ty: Some("types/span".into()),
        },
    ];
    let mut lock = WireLock::default();
    let changes = lock.reconcile(&members).unwrap();
    assert_eq!(changes.added.len(), 3);
    let entry = lock.get("a:b/i/t.x").unwrap();
    assert_eq!(entry.kind, MemberKind::Field);
    assert_eq!(entry.ty.as_deref(), Some("u64"));

    let edited = [Member {
        parent: "a:b/i/t".into(),
        name: "x".into(),
        kind: MemberKind::Field,
        ty: Some("u16".into()),
    }];
    let mut edited_lock = lock.clone();
    assert!(edited_lock.reconcile(&edited).is_err());

    lock.reconcile(&members[1..]).unwrap();
    let reserved = lock.get("a:b/i/t.x").unwrap();
    assert!(reserved.reserved);
    assert_eq!(reserved.kind, MemberKind::Field);
    assert_eq!(reserved.ty.as_deref(), Some("u64"));
    let reintroduced = lock.reconcile(&members).unwrap();
    assert_eq!(
        reintroduced.reintroduced,
        vec![("a:b/i/t.x".to_string(), 1)]
    );
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn model_members_lock_kinds_canonical_types_and_parameters() {
    let model = model::build(&load(&fixture("model")).unwrap()).unwrap();
    let members = model.members().unwrap();
    let mut lock = WireLock::default();
    lock.reconcile(&members).unwrap();
    for member in &members {
        let entry = lock.get(&member.key()).unwrap();
        assert_eq!(entry.kind, member.kind, "{} kind", member.key());
        assert_eq!(entry.ty, member.ty, "{} type", member.key());
    }

    let no_result = lock.get("example:model/primitives.no-result").unwrap();
    assert_eq!(no_result.kind, MemberKind::Method);
    assert_eq!(no_result.ty, None);
    let result = lock.get("example:model/primitives.all-params").unwrap();
    assert_eq!(
        result.ty.as_deref(),
        Some("result<list<primitives/all>,string>")
    );
    assert_eq!(
        lock.get("example:model/primitives#all-params.a-nested")
            .unwrap()
            .ty
            .as_deref(),
        Some("list<option<result<u8,string>>>")
    );
    assert_eq!(
        lock.get("example:model/primitives/status.ready")
            .unwrap()
            .kind,
        MemberKind::EnumCase
    );
    assert_eq!(
        lock.get("example:model/primitives/choice.with-payload")
            .unwrap()
            .ty
            .as_deref(),
        Some("primitives/all")
    );
}

#[test]
fn append_only_check_rejects_kind_and_type_changes() {
    let older = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.y = 2 variant-case\n"
    ))
    .unwrap();
    let type_changed = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 1 field u16\na:b/i/t.y = 2 variant-case\n"
    ))
    .unwrap();
    assert!(type_changed.check_append_only(&older).is_err());
    let kind_changed = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.y = 2 enum-case\n"
    ))
    .unwrap();
    assert!(kind_changed.check_append_only(&older).is_err());
}

#[test]
fn reconcile_does_not_mutate_on_error() {
    let members = [Member {
        parent: "a:b/i/t".into(),
        name: "x".into(),
        kind: MemberKind::Field,
        ty: Some("u64".into()),
    }];
    let mut lock = WireLock::default();
    lock.reconcile(&members).unwrap();
    let before = lock.clone();
    let changed = [Member {
        parent: "a:b/i/t".into(),
        name: "x".into(),
        kind: MemberKind::Field,
        ty: Some("u16".into()),
    }];
    assert!(lock.reconcile(&changed).is_err());
    assert_eq!(lock, before);
}

#[test]
fn evolution_fixture_records_add_remove_rename_and_reserve() {
    let root = fixture("versions/wit");
    let v01 = WireLock::parse(&fs::read_to_string(root.join("v0.1/wire.lock")).unwrap()).unwrap();
    assert_eq!(
        v01.get("example:evolve/types/user.removed").unwrap().number,
        4
    );
    assert!(
        v01.get("example:evolve/types/user.removed")
            .unwrap()
            .reserved
    );

    let v02 = WireLock::parse(&fs::read_to_string(root.join("v0.2/wire.lock")).unwrap()).unwrap();
    assert_eq!(v02.get("example:evolve/types/user.name").unwrap().number, 1);
    assert_eq!(
        v02.get("example:evolve/types/user.new-field")
            .unwrap()
            .number,
        5
    );
    assert_eq!(
        v02.get("example:evolve/types/user.renamed").unwrap().number,
        6
    );
    for (key, number) in [
        ("example:evolve/types/user.old-field", 2),
        ("example:evolve/types/user.removed", 4),
        ("example:evolve/types/user.rename-me", 3),
    ] {
        let entry = v02.get(key).unwrap();
        assert_eq!(entry.number, number);
        assert!(entry.reserved, "{key}");
    }
}

#[test]
fn atomic_write_replaces_the_lockfile_without_leaving_temporaries() {
    let dir = std::env::temp_dir().join(format!("witforge-lock-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("wire.lock");
    let lock = WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64\n")).unwrap();
    lock.write_atomic(&path).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), lock.render());
    lock.write_atomic(&path).unwrap();
    let names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["wire.lock"]);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn first_mode_updates_drops_and_readds_drafts() {
    let mut lock = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.old = 2 field u64\na:b/i/t.stale = 3 field u64 reserved\n"
    ))
    .unwrap();
    let members = [
        Member {
            parent: "a:b/i/t".into(),
            name: "x".into(),
            kind: MemberKind::Field,
            ty: Some("u16".into()),
        },
        Member {
            parent: "a:b/i/t".into(),
            name: "y".into(),
            kind: MemberKind::Field,
            ty: Some("string".into()),
        },
    ];
    let changes = lock.reconcile_with(&members, LockMode::First).unwrap();
    assert_eq!(
        changes.updated,
        vec![(
            "a:b/i/t.x".to_string(),
            MemberKind::Field,
            Some("u16".to_string())
        )]
    );
    assert_eq!(
        changes
            .dropped
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>(),
        ["a:b/i/t.old", "a:b/i/t.stale"]
    );
    assert_eq!(changes.added, vec![("a:b/i/t.y".to_string(), 2)]);
    assert_eq!(lock.get("a:b/i/t.x").unwrap().ty.as_deref(), Some("u16"));
    assert_eq!(lock.get("a:b/i/t.y").unwrap().number, 2);
    assert!(lock.get("a:b/i/t.old").is_none());
    assert!(lock.get("a:b/i/t.stale").is_none());
}

#[test]
fn against_mode_updates_drafts_but_protects_released_entries() {
    let previous = WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64\n")).unwrap();
    let mut current = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.y = 2 field u64\na:b/i/t.z = 3 field u64 reserved\na:b/i/t.old = 4 field u64\n"
    ))
    .unwrap();
    let members = [
        Member {
            parent: "a:b/i/t".into(),
            name: "x".into(),
            kind: MemberKind::Field,
            ty: Some("u64".into()),
        },
        Member {
            parent: "a:b/i/t".into(),
            name: "y".into(),
            kind: MemberKind::Field,
            ty: Some("string".into()),
        },
        Member {
            parent: "a:b/i/t".into(),
            name: "z".into(),
            kind: MemberKind::Field,
            ty: Some("u64".into()),
        },
        Member {
            parent: "a:b/i/t".into(),
            name: "w".into(),
            kind: MemberKind::Field,
            ty: Some("u8".into()),
        },
    ];
    let changes = current
        .reconcile_with(&members, LockMode::Against(&previous))
        .unwrap();
    assert_eq!(
        changes.updated,
        vec![(
            "a:b/i/t.y".to_string(),
            MemberKind::Field,
            Some("string".to_string())
        )]
    );
    assert_eq!(
        changes
            .dropped
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>(),
        ["a:b/i/t.old", "a:b/i/t.z"]
    );
    assert_eq!(
        changes.added,
        vec![("a:b/i/t.z".to_string(), 3), ("a:b/i/t.w".to_string(), 4)]
    );
    assert_eq!(current.get("a:b/i/t.x").unwrap().ty.as_deref(), Some("u64"));
    assert_eq!(
        current.get("a:b/i/t.y").unwrap().ty.as_deref(),
        Some("string")
    );

    let mut removed = previous.clone();
    let changes = removed
        .reconcile_with(&[], LockMode::Against(&previous))
        .unwrap();
    assert_eq!(changes.reserved, vec![("a:b/i/t.x".to_string(), 1)]);
    assert!(removed.get("a:b/i/t.x").unwrap().reserved);

    let mut retyped = previous.clone();
    let changed_members = [Member {
        parent: "a:b/i/t".into(),
        name: "x".into(),
        kind: MemberKind::Field,
        ty: Some("u16".into()),
    }];
    assert!(
        retyped
            .reconcile_with(&changed_members, LockMode::Against(&previous))
            .is_err()
    );
    assert_eq!(retyped, previous);

    let reintroduced_previous =
        WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64 reserved\n")).unwrap();
    let mut reintroduced = reintroduced_previous.clone();
    let changes = reintroduced
        .reconcile_with(
            &[Member {
                parent: "a:b/i/t".into(),
                name: "x".into(),
                kind: MemberKind::Field,
                ty: Some("u64".into()),
            }],
            LockMode::Against(&reintroduced_previous),
        )
        .unwrap();
    assert_eq!(changes.reintroduced, vec![("a:b/i/t.x".to_string(), 1)]);
}

#[test]
fn against_mode_rejects_released_renumbering_and_un_reserving() {
    let previous = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.y = 2 field u64 reserved\n"
    ))
    .unwrap();
    let renumbered = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 3 field u64\na:b/i/t.y = 2 field u64 reserved\n"
    ))
    .unwrap();
    let mut renumbered = renumbered;
    assert!(
        renumbered
            .reconcile_with(&[], LockMode::Against(&previous))
            .is_err()
    );
    let unreserved = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.y = 2 field u64\n"
    ))
    .unwrap();
    let mut unreserved = unreserved;
    assert!(
        unreserved
            .reconcile_with(&[], LockMode::Against(&previous))
            .is_err()
    );
}

#[test]
fn against_mode_rejects_a_different_package() {
    let previous = WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64\n")).unwrap();
    let mut current = WireLock::parse(&format!("{HEADER}\nc:d/i/t.x = 1 field u64\n")).unwrap();
    assert!(
        current
            .reconcile_with(&[], LockMode::Against(&previous))
            .is_err()
    );
    let mixed = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 1 field u64\nc:d/i/t.y = 2 field u64\n"
    ))
    .unwrap();
    let mut current = WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64\n")).unwrap();
    assert!(
        current
            .reconcile_with(&[], LockMode::Against(&mixed))
            .is_err()
    );
}

#[test]
fn against_mode_rejects_a_missing_released_entry() {
    let previous = WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64\n")).unwrap();
    let mut current = WireLock::default();
    assert!(
        current
            .reconcile_with(&[], LockMode::Against(&previous))
            .is_err()
    );
}

#[test]
fn first_mode_readds_a_reserved_draft() {
    let mut lock =
        WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64 reserved\n")).unwrap();
    let members = [Member {
        parent: "a:b/i/t".into(),
        name: "x".into(),
        kind: MemberKind::Field,
        ty: Some("u64".into()),
    }];
    let changes = lock.reconcile_with(&members, LockMode::First).unwrap();
    assert_eq!(changes.dropped, vec![("a:b/i/t.x".to_string(), 1)]);
    assert_eq!(changes.added, vec![("a:b/i/t.x".to_string(), 1)]);
    assert!(!lock.get("a:b/i/t.x").unwrap().reserved);
}

#[test]
fn against_mode_rejects_a_released_kind_change() {
    let previous = WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64\n")).unwrap();
    let mut current =
        WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 variant-case u64\n")).unwrap();
    assert!(
        current
            .reconcile_with(&[], LockMode::Against(&previous))
            .is_err()
    );
}

#[test]
fn missing_default_released_manifest_means_nothing_is_released() {
    let root = std::env::temp_dir().join(format!("witforge-no-released-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let wit = root.join("wit");
    let v01 = wit.join("v0.1");
    fs::create_dir_all(&v01).unwrap();
    fs::write(v01.join("wire.lock"), format!("{HEADER}\n")).unwrap();
    assert_eq!(
        load_released_manifest(&v01, None)
            .unwrap()
            .directories()
            .len(),
        0
    );
    ensure_not_released(&v01, None).unwrap();
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn released_manifest_validates_entries_and_blocks_locking() {
    let root = std::env::temp_dir().join(format!("witforge-released-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let wit = root.join("wit");
    let v01 = wit.join("v0.1");
    let v02 = wit.join("v0.2");
    fs::create_dir_all(&v01).unwrap();
    fs::create_dir_all(&v02).unwrap();
    fs::write(v01.join("wire.lock"), format!("{HEADER}\n")).unwrap();
    fs::write(v02.join("wire.lock"), format!("{HEADER}\n")).unwrap();
    let manifest_path = wit.join("released");
    fs::write(&manifest_path, "# released\nv0.1\n").unwrap();

    let manifest = ReleasedManifest::load(&manifest_path).unwrap();
    assert_eq!(manifest.directories(), ["v0.1"]);
    assert!(manifest.contains("v0.1"));
    assert!(!manifest.contains("v0.2"));
    assert!(ensure_not_released(&v01, Some(&manifest_path)).is_err());
    ensure_not_released(&v02, Some(&manifest_path)).unwrap();
    assert!(load_released_manifest(&v01, None).unwrap().contains("v0.1"));
    ensure_not_released(&v01, None).unwrap_err();

    fs::write(&manifest_path, "v0.1\nv0.1\n").unwrap();
    assert!(ReleasedManifest::load(&manifest_path).is_err());
    fs::write(&manifest_path, "../v0.1\n").unwrap();
    assert!(ReleasedManifest::load(&manifest_path).is_err());
    fs::write(&manifest_path, "v9.9\n").unwrap();
    assert!(ReleasedManifest::load(&manifest_path).is_err());
    fs::remove_file(v01.join("wire.lock")).unwrap();
    fs::write(&manifest_path, "v0.1\n").unwrap();
    assert!(ReleasedManifest::load(&manifest_path).is_err());
    fs::remove_dir_all(&root).unwrap();
}

fn generated_members(indices: &[(u8, u8)]) -> Vec<Member> {
    let mut seen = BTreeSet::new();
    let mut members = Vec::new();
    for (parent, field) in indices {
        let parent = format!("a:b/i/t{parent}");
        let name = format!("x{field}");
        if seen.insert((parent.clone(), name.clone())) {
            members.push(Member {
                parent,
                name,
                kind: MemberKind::Field,
                ty: Some("u64".into()),
            });
        }
    }
    members
}

proptest! {
    #[test]
    fn reconcile_is_idempotent(indices in prop::collection::vec((0u8..4, 0u8..6), 0..20)) {
        let members = generated_members(&indices);
        let mut lock = WireLock::default();
        lock.reconcile(&members).unwrap();
        let before = lock.clone();
        let changes = lock.reconcile(&members).unwrap();
        prop_assert!(changes.is_empty());
        prop_assert_eq!(lock, before);
    }

    #[test]
    fn reconcile_preserves_numbers_and_append_only(
        indices in prop::collection::vec((0u8..4, 0u8..6), 1..20),
        remove_hint in 0usize..64,
    ) {
        let members = generated_members(&indices);
        prop_assume!(!members.is_empty());
        let mut lock = WireLock::default();
        lock.reconcile(&members).unwrap();
        let before = lock.clone();
        let remove = remove_hint % members.len();
        let remaining: Vec<Member> = members
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != remove)
            .map(|(_, member)| member.clone())
            .collect();
        let changes = lock.reconcile(&remaining).unwrap();
        prop_assert!(changes.reintroduced.is_empty());
        prop_assert!(lock.check_append_only(&before).is_ok());
        let removed = &members[remove];
        let removed_entry = lock.get(&removed.key()).unwrap();
        prop_assert!(removed_entry.reserved);
        prop_assert_eq!(
            removed_entry.number,
            before.get(&removed.key()).unwrap().number
        );
        for member in &remaining {
            prop_assert_eq!(
                lock.get(&member.key()).unwrap().number,
                before.get(&member.key()).unwrap().number
            );
        }
    }
}
