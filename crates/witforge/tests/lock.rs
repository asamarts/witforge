//! Typed lockfile v2 parsing, rendering, keys, and entry-matrix tests.

use std::path::{Path, PathBuf};

use witforge::load;
use witforge::lock::{HEADER, MemberKey, WireLock};
use witforge::model::{self, Member, MemberKind};

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
