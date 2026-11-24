//! Typed lockfile v2 parsing, rendering, keys, and entry-matrix tests.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use witforge::lock::{HEADER, LockMode, MemberKey, ORDER_HEADER, PackageName, WireLock};
use witforge::model::{self, Member, MemberKind};
use witforge::{ReleasedManifest, ensure_not_released, load, load_released_manifest};

fn member(parent: &str, name: &str, kind: MemberKind, ty: Option<&str>) -> Member {
    Member::new(
        parent.to_string(),
        name.to_string(),
        kind,
        ty.map(ToString::to_string),
    )
}

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

fn order_fingerprint(names: &[&str]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for name in names {
        for byte in name.bytes().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    hash
}

#[test]
fn order_fingerprints_render_and_round_trip_per_parent() {
    let members = fixture_members();
    let mut lock = WireLock::default();
    lock.reconcile(&members).unwrap();
    let text = lock.render();
    assert!(text.contains(ORDER_HEADER), "{text}");
    assert!(text.contains("# order example:plugin/types/span"), "{text}");
    assert!(text.contains("# order example:plugin/analyzer "), "{text}");
    assert!(
        text.contains("# order example:plugin/analyzer#analyze"),
        "{text}"
    );
    assert_eq!(WireLock::parse(&text).unwrap(), lock);
    assert!(lock.is_canonical(&text));
}

#[test]
fn against_catches_reorders_but_ignores_additions_and_removals() {
    let members = fixture_members();
    let mut baseline = WireLock::default();
    baseline.reconcile(&members).unwrap();

    // Reorder two fields of one type.
    let mut reordered = members.clone();
    let parent = reordered[0].parent.clone();
    let first = reordered
        .iter()
        .position(|member| member.parent == parent)
        .unwrap();
    let second = reordered
        .iter()
        .enumerate()
        .skip(first + 1)
        .find(|(_, member)| member.parent == parent)
        .map(|(index, _)| index)
        .unwrap();
    reordered.swap(first, second);
    let report = baseline
        .drift_report_with(&reordered, LockMode::Against(&baseline))
        .unwrap();
    let message = report
        .discrepancies
        .iter()
        .map(|change| change.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(message.contains("declaration order"), "{message}");
    assert!(message.contains(&parent), "{message}");
    let report = baseline.drift_report(&reordered).unwrap();
    assert!(
        report
            .discrepancies
            .iter()
            .any(|change| change.message.contains("declaration order")),
        "{report:?}"
    );

    // Adding a member to the same parent is not a reorder of common members.
    let mut added = members.clone();
    added.push(member(&parent, "extra", MemberKind::Field, Some("u8")));
    baseline
        .drift_with(&added, LockMode::Against(&baseline))
        .unwrap();

    // Removing a common member is also not a reorder.
    let removed: Vec<Member> = members
        .iter()
        .filter(|member| member.key() != members[first].key())
        .cloned()
        .collect();
    baseline
        .drift_with(&removed, LockMode::Against(&baseline))
        .unwrap();
}

#[test]
fn append_only_check_refuses_a_reordered_order_section() {
    let members = fixture_members();
    let mut baseline = WireLock::default();
    baseline.reconcile(&members).unwrap();
    let text = baseline.render();

    for (parent, old, new) in [
        (
            "example:plugin/types/span",
            ["start", "end"],
            ["end", "start"],
        ),
        (
            "example:plugin/analyzer#analyze",
            ["path", "bytes"],
            ["bytes", "path"],
        ),
    ] {
        let old_line = format!(
            "# order {parent} {:016x} = {}",
            order_fingerprint(&old),
            old.join(" ")
        );
        let new_line = format!(
            "# order {parent} {:016x} = {}",
            order_fingerprint(&new),
            new.join(" ")
        );
        assert!(text.contains(&old_line), "{parent}: {text}");
        let reordered = WireLock::parse(&text.replace(&old_line, &new_line)).unwrap();
        let error = reordered.check_append_only(&baseline).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("declaration order"), "{message}");
        assert!(message.contains(parent), "{message}");
    }
}

#[test]
fn order_section_rejects_malformed_and_duplicate_fingerprints() {
    let members = fixture_members();
    let mut lock = WireLock::default();
    lock.reconcile(&members).unwrap();
    let text = lock.render();
    let line = text
        .lines()
        .find(|line| line.starts_with("# order example:plugin/types/span"))
        .unwrap();
    let bad = "# order example:plugin/types/span 0000000000000000 = start end";
    let error = WireLock::parse(&text.replace(line, bad)).unwrap_err();
    assert!(format!("{error:#}").contains("does not match"), "{error:#}");

    let doubled = format!("{text}{line}\n");
    let error = WireLock::parse(&doubled).unwrap_err();
    assert!(
        format!("{error:#}").contains("duplicate order fingerprint"),
        "{error:#}"
    );
}

#[test]
fn order_sections_are_checked_against_the_contract_package() {
    let text = format!(
        "{HEADER}\n{ORDER_HEADER}\n# order other:pkg/types/t {:016x} = x\n",
        order_fingerprint(&["x"])
    );
    let lock = WireLock::parse(&text).unwrap();
    let package = PackageName::new("a".to_string(), "b".to_string());
    let error = lock.check_package(&package).unwrap_err();
    assert!(
        format!("{error:#}").contains("order fingerprint"),
        "{error:#}"
    );
}

#[test]
fn a_v2_lockfile_without_fingerprints_migrates_on_the_next_lock() {
    let members = fixture_members();
    let mut current = WireLock::default();
    current.reconcile(&members).unwrap();
    let with_orders = current.render();
    let without_orders = with_orders
        .split_once(ORDER_HEADER)
        .map_or(with_orders.as_str(), |(before, _)| before);
    let old = WireLock::parse(without_orders).unwrap();
    assert!(!old.render().contains(ORDER_HEADER));
    assert!(old.is_canonical(without_orders));
    assert_eq!(
        old.drift(&members).unwrap(),
        witforge::lock::Changes::default()
    );

    let mut migrated = old;
    migrated.reconcile(&members).unwrap();
    assert!(migrated.render().contains(ORDER_HEADER));
    assert_eq!(migrated, current);
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
        format!("{HEADER}\na:b/i/t.x = 0 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 19000 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 536870912 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 4294967296 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.x = 2 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.y = 1 field u64\n"),
        format!("{HEADER}\nnonsense\n"),
        // A parameter key's interface is a WIT identifier like any other key's.
        format!("{HEADER}\na:b/x/y.z#f.p = 1 param u32\n"),
        format!("{HEADER}\na:b/Not An Ident!#f.p = 1 param u32\n"),
        format!("{HEADER}\na:b/#f.p = 1 param u32\n"),
    ] {
        assert!(WireLock::parse(&bad).is_err(), "{bad:?} should fail");
    }

    let error = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.y = 1 field u64\n"
    ))
    .unwrap_err();
    assert!(format!("{error:#}").contains("line 3"), "{error:#}");
}

#[test]
fn entry_kinds_must_match_their_key_shape() {
    let keys = [
        (
            "a:b/i/t.x",
            &[
                MemberKind::Field,
                MemberKind::VariantCase,
                MemberKind::EnumCase,
                MemberKind::Flag,
            ][..],
        ),
        ("a:b/i.f", &[MemberKind::Method][..]),
        ("a:b/i#f.p", &[MemberKind::Param][..]),
    ];
    let kinds = [
        MemberKind::Field,
        MemberKind::VariantCase,
        MemberKind::EnumCase,
        MemberKind::Flag,
        MemberKind::Method,
        MemberKind::Param,
    ];
    for (key, allowed) in keys {
        for kind in kinds {
            let ty = match kind {
                MemberKind::Field | MemberKind::Param => " u8",
                _ => "",
            };
            let text = format!("{HEADER}\n{key} = 1 {}{ty}\n", kind.as_str());
            let parsed = WireLock::parse(&text);
            assert_eq!(
                parsed.is_ok(),
                allowed.contains(&kind),
                "`{key}` with kind `{kind}` should {} parse",
                if allowed.contains(&kind) { "" } else { "not" }
            );
            if let Err(error) = parsed {
                let message = format!("{error:#}");
                assert!(message.contains("line 2"), "{message}");
            }
        }
    }
}

#[test]
fn the_audit_reproduction_lines_are_rejected() {
    for line in [
        "ex:t/api#get.old = 7 enum-case reserved",
        "ex:t/api.gone = 5 field u32 reserved",
        "ex:t/api/r.zz = 9 method reserved",
    ] {
        let text = format!("{HEADER}\n{line}\n");
        let error = WireLock::parse(&text).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("line 2"), "{message}");
        assert!(message.contains("not allowed under key"), "{message}");
    }
}

#[test]
fn only_ascii_spaces_and_tabs_separate_entry_fields() {
    let good = format!("{HEADER}\n\ta:b/i/t.x\t=\t1\tfield\tu64\t\n");
    let lock = WireLock::parse(&good).unwrap();
    assert_eq!(lock.get("a:b/i/t.x").unwrap().ty.as_deref(), Some("u64"));

    let accepted = format!(
        "{HEADER}\r\n# comment\r\n\r\n  a:b/i/t.x = 1 field u64  \r\n\
         a:b/i/t.y = 2 field u64"
    );
    let lock = WireLock::parse(&accepted).unwrap();
    assert_eq!(lock.entries().count(), 2);

    for separator in ['\u{a0}', '\u{2003}', '\u{000b}', '\u{000c}'] {
        for text in [
            format!("{HEADER}\na:b/i/t.x = 1{separator}field u64\n"),
            format!("{HEADER}\na:b/i/t.x = 1 field{separator}u64\n"),
        ] {
            assert!(
                WireLock::parse(&text).is_err(),
                "U+{:04X} must not separate fields: {text:?}",
                separator as u32
            );
        }
    }

    let bom = format!("\u{feff}{HEADER}\n");
    let error = WireLock::parse(&bom).unwrap_err();
    assert!(format!("{error:#}").contains("line 1"), "{error:#}");
}

#[test]
fn order_lines_use_the_key_grammar_and_exact_fingerprints() {
    let fingerprint = format!("{:016x}", order_fingerprint(&["x"]));
    for parent in ["a:b/i/t", "a:b/i", "a:b/i#f"] {
        let text = format!("{HEADER}\n{ORDER_HEADER}\n# order {parent} {fingerprint} = x\n");
        WireLock::parse(&text).unwrap();
    }
    let tabbed = format!("{HEADER}\n{ORDER_HEADER}\n# order a:b/i/t\t{fingerprint} = x\n");
    assert!(WireLock::parse(&tabbed).is_err());
    let bad_member = format!("{HEADER}\n{ORDER_HEADER}\n# order a:b/i/t {fingerprint} = x_y\n");
    let error = WireLock::parse(&bad_member).unwrap_err();
    assert!(format!("{error:#}").contains("line 3"), "{error:#}");
    for parent in [
        "a:b/i/t.x",
        "a:b/i#f.p",
        "a:b/i/t#f",
        "a:b/i/t/u",
        "a:b",
        "a:b/i/t.",
    ] {
        let text = format!("{HEADER}\n{ORDER_HEADER}\n# order {parent} {fingerprint} = x\n");
        let error = WireLock::parse(&text).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("line 3"), "{message}");
        assert!(message.contains("invalid order parent"), "{message}");
    }
    let bad_fingerprints = [
        format!("+{fingerprint}"),
        format!("A{}", &fingerprint[1..]),
        fingerprint[..15].to_string(),
        format!("{fingerprint}0"),
        "g000000000000000".to_string(),
    ];
    for token in bad_fingerprints {
        let text = format!("{HEADER}\n{ORDER_HEADER}\n# order a:b/i/t {token} = x\n");
        let error = WireLock::parse(&text).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("line 3"), "{message}");
        assert!(message.contains("sixteen lowercase"), "{message}");
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
    for bad in [
        "types/SpAn",
        "types/span_x",
        "types/span-",
        "types//span",
        "types/span/extra",
        "u8:foo/bar",
        "ex:shared/types",
        "ex:shared/types/item/extra",
        "ex:shared//item",
        "ex:/types/item",
        "list",
        "option",
        "result",
        "reserved",
        "list<>",
        "list<",
        "list<u8",
        "list<u8>>",
        "option<u8",
        "result<u8>",
        "result<,>",
        "result<u8,>",
        "result<_,_,_>",
        "list< u8>",
    ] {
        let text = format!("{HEADER}\na:b/i/t.x = 1 field {bad}\n");
        assert!(WireLock::parse(&text).is_err(), "{bad:?} should fail");
    }
}

#[test]
fn reconcile_records_and_checks_kind_and_type() {
    let members = [
        member("a:b/i/t", "x", MemberKind::Field, Some("u64")),
        member("a:b/i", "f", MemberKind::Method, Some("result<_,_>")),
        member("a:b/i#f", "p", MemberKind::Param, Some("types/span")),
    ];
    let mut lock = WireLock::default();
    let changes = lock.reconcile(&members).unwrap();
    assert_eq!(changes.added.len(), 3);
    let entry = lock.get("a:b/i/t.x").unwrap();
    assert_eq!(entry.kind, MemberKind::Field);
    assert_eq!(entry.ty.as_deref(), Some("u64"));

    let edited = [member("a:b/i/t", "x", MemberKind::Field, Some("u16"))];
    let mut edited_lock = lock.clone();
    let report = edited_lock.reconcile_report(&edited).unwrap();
    assert!(
        report
            .discrepancies
            .iter()
            .any(|change| change.key == "a:b/i/t.x" && change.message.contains("kind or type")),
        "{report:?}"
    );
    assert_eq!(
        edited_lock, lock,
        "a refused reconciliation must not mutate"
    );

    lock.reconcile(&members[1..]).unwrap();
    let reserved = lock.get("a:b/i/t.x").unwrap();
    assert!(reserved.reserved);
    assert_eq!(reserved.kind, MemberKind::Field);
    assert_eq!(reserved.ty.as_deref(), Some("u64"));
    let before_reintroduction = lock.clone();
    let reintroduced = lock.reconcile(&members).unwrap();
    assert_eq!(
        reintroduced.reintroduced,
        vec![("a:b/i/t.x".to_string(), 1)]
    );
    assert_eq!(
        lock, before_reintroduction,
        "a reintroduction must not mutate the lockfile"
    );
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn fixture_members() -> Vec<Member> {
    let model = model::build(&load(&fixture("ok")).unwrap()).unwrap();
    model.members().unwrap()
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
    let members = [member("a:b/i/t", "x", MemberKind::Field, Some("u64"))];
    let mut lock = WireLock::default();
    lock.reconcile(&members).unwrap();
    let before = lock.clone();
    let retyped_members = [member("a:b/i/t", "x", MemberKind::Field, Some("u16"))];
    let report = lock.reconcile_report(&retyped_members).unwrap();
    assert!(!report.discrepancies.is_empty(), "{report:?}");
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
        member("a:b/i/t", "x", MemberKind::Field, Some("u16")),
        member("a:b/i/t", "y", MemberKind::Field, Some("string")),
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
        member("a:b/i/t", "x", MemberKind::Field, Some("u64")),
        member("a:b/i/t", "y", MemberKind::Field, Some("string")),
        member("a:b/i/t", "z", MemberKind::Field, Some("u64")),
        member("a:b/i/t", "w", MemberKind::Field, Some("u8")),
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
    let changed_members = [member("a:b/i/t", "x", MemberKind::Field, Some("u16"))];
    let report = retyped
        .reconcile_report_with(&changed_members, LockMode::Against(&previous))
        .unwrap();
    assert!(
        report
            .discrepancies
            .iter()
            .any(|change| change.key == "a:b/i/t.x" && change.message.contains("kind or type")),
        "{report:?}"
    );
    assert_eq!(retyped, previous);

    let reintroduced_previous =
        WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64 reserved\n")).unwrap();
    let mut reintroduced = reintroduced_previous.clone();
    let changes = reintroduced
        .reconcile_with(
            &[member("a:b/i/t", "x", MemberKind::Field, Some("u64"))],
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
    let report = renumbered
        .reconcile_report_with(&[], LockMode::Against(&previous))
        .unwrap();
    assert!(
        report
            .discrepancies
            .iter()
            .any(|change| change.key == "a:b/i/t.x" && change.message.contains("renumbered")),
        "{report:?}"
    );
    let unreserved = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.y = 2 field u64\n"
    ))
    .unwrap();
    let mut unreserved = unreserved;
    let report = unreserved
        .reconcile_report_with(&[], LockMode::Against(&previous))
        .unwrap();
    assert!(
        report
            .discrepancies
            .iter()
            .any(|change| change.key == "a:b/i/t.y" && change.message.contains("un-reserved")),
        "{report:?}"
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
    let report = current
        .reconcile_report_with(&[], LockMode::Against(&previous))
        .unwrap();
    assert!(
        report
            .discrepancies
            .iter()
            .any(|change| change.key == "a:b/i/t.x" && change.message.contains("missing")),
        "{report:?}"
    );
}

#[test]
fn first_mode_readds_a_reserved_draft() {
    let mut lock =
        WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64 reserved\n")).unwrap();
    let members = [member("a:b/i/t", "x", MemberKind::Field, Some("u64"))];
    let changes = lock.reconcile_with(&members, LockMode::First).unwrap();
    assert_eq!(changes.dropped, vec![("a:b/i/t.x".to_string(), 1)]);
    assert_eq!(changes.added, vec![("a:b/i/t.x".to_string(), 1)]);
    assert!(!lock.get("a:b/i/t.x").unwrap().reserved);
}

#[test]
fn order_section_must_name_every_live_member_of_every_parent() {
    let a = member("a:b/i/t", "a", MemberKind::Field, Some("u32"));
    let b = member("a:b/i/t", "b", MemberKind::Field, Some("u32"));
    let lock_for = |members: &[Member]| {
        let mut lock = WireLock::default();
        lock.reconcile(members).unwrap();
        lock
    };
    let exact = lock_for(&[a.clone(), b.clone()]);
    exact
        .validate_order_section(&[a.clone(), b.clone()], false)
        .unwrap();

    // Deleting a line but keeping the marker is a missing line, not a legacy section.
    let text: String = exact
        .render()
        .lines()
        .filter(|line| !line.starts_with("# order "))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let deleted_line = WireLock::parse(&text).unwrap();
    assert!(
        deleted_line
            .validate_order_section(&[a.clone(), b.clone()], false)
            .is_err()
    );

    let subset = lock_for(std::slice::from_ref(&a));
    let error = subset
        .validate_order_section(&[a.clone(), b.clone()], false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("names `a`"), "{error}");
    assert!(error.contains("expected `a b`"), "{error}");

    let extra = lock_for(&[a.clone(), b.clone()]);
    assert!(
        extra
            .validate_order_section(std::slice::from_ref(&a), false)
            .is_err()
    );

    let other = member("a:b/i/other", "a", MemberKind::Field, Some("u32"));
    let unknown = lock_for(&[other]);
    let error = unknown
        .validate_order_section(std::slice::from_ref(&a), false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("no live members"), "{error}");

    let missing = WireLock::parse(&format!("{HEADER}\na:b/i/t.a = 1 field u32\n")).unwrap();
    assert!(
        missing
            .validate_order_section(std::slice::from_ref(&a), false)
            .is_err()
    );
    // A released lockfile written before the section existed stays accepted.
    assert!(
        missing
            .validate_order_section(std::slice::from_ref(&a), true)
            .is_ok()
    );
}

#[test]
fn a_parent_that_mixes_kinds_is_refused_by_itself_and_by_drift() {
    // What a pre-0.6.0 `lock --against` left after a released enum became a record: the old
    // cases stay reserved and the new fields are numbered after them. Neither number nor type
    // changed, so the old released-entry checks alone would accept it.
    let mixed = WireLock::parse(&format!(
        "{HEADER}\n\
a:b/i/t.old = 1 enum-case reserved\n\
a:b/i/t.new = 2 field u64\n"
    ))
    .unwrap();
    let members = [member("a:b/i/t", "new", MemberKind::Field, Some("u64"))];
    let report = mixed.drift_report(&members).unwrap();
    let message = report
        .discrepancies
        .iter()
        .map(|change| change.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(message.contains("`a:b/i/t`"), "{message}");
    assert!(message.contains("enum-case"), "{message}");
    assert!(message.contains("field"), "{message}");

    let mut lock = mixed.clone();
    let report = lock.reconcile_report(&members).unwrap();
    assert!(!report.discrepancies.is_empty(), "{report:?}");
    assert_eq!(lock, mixed, "a refused reconciliation must not mutate");

    // Removing a type in one version and adding another kind under the same name in a later
    // version leaves only reserved entries, but they still mix and are still refused.
    let all_reserved = WireLock::parse(&format!(
        "{HEADER}\n\
a:b/i/t.old = 1 enum-case reserved\n\
a:b/i/t.older = 2 field u64 reserved\n"
    ))
    .unwrap();
    assert!(
        !all_reserved
            .drift_report(&[])
            .unwrap()
            .discrepancies
            .is_empty(),
        "a mixed parent must be reported"
    );
}

#[test]
fn first_mode_drops_a_draft_kind_change_instead_of_mixing_it() {
    let mut lock = WireLock::parse(&format!(
        "{HEADER}\n\
a:b/i/t.one = 1 enum-case\n\
a:b/i/t.two = 2 enum-case\n"
    ))
    .unwrap();
    let members = [member("a:b/i/t", "one", MemberKind::Field, Some("u32"))];
    let changes = lock.reconcile_with(&members, LockMode::First).unwrap();
    assert_eq!(changes.dropped, vec![("a:b/i/t.two".to_string(), 2)]);
    assert_eq!(
        changes.updated,
        vec![(
            "a:b/i/t.one".to_string(),
            MemberKind::Field,
            Some("u32".to_string())
        )]
    );
    assert_eq!(changes.added.len(), 0);
    assert_eq!(lock.entries().count(), 1);
    assert!(lock.get("a:b/i/t.two").is_none());
}

#[test]
fn against_mode_rejects_a_released_kind_change() {
    let previous = WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64\n")).unwrap();
    let mut current =
        WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 variant-case u64\n")).unwrap();
    let report = current
        .reconcile_report_with(&[], LockMode::Against(&previous))
        .unwrap();
    assert!(
        report
            .discrepancies
            .iter()
            .any(|change| change.key == "a:b/i/t.x" && change.message.contains("kind or type")),
        "{report:?}"
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

#[test]
fn released_manifest_entries_must_be_plain_directory_names() {
    let root = std::env::temp_dir().join(format!("witforge-released-names-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let manifest_path = root.join("released");
    let make = |name: &str| {
        let directory = root.join(name);
        if fs::create_dir_all(&directory).is_ok() {
            let _ = fs::write(directory.join("wire.lock"), format!("{HEADER}\n"));
        }
    };

    // Each name exists as a directory with a lockfile where the platform allows it, so only the
    // name rule `^[A-Za-z0-9][A-Za-z0-9._-]*$` can refuse it.
    let mut refused = vec!["v0 1", "vé", ".hidden", "-v0.1", "_v0.1", "v0.1~", "v0+1"];
    if cfg!(unix) {
        refused.extend(["...", "v0\\1", "v0:1"]);
    }
    for name in &refused {
        make(name);
    }
    refused.extend([".", "..", "../v0.1", "a/b", "/abs"]);
    for name in refused {
        fs::write(&manifest_path, format!("{name}\n")).unwrap();
        let error = ReleasedManifest::load(&manifest_path).unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("plain directory name"),
            "`{name}`: {message}"
        );
    }

    for name in ["v0.1", "V1_2-rc", "0release"] {
        make(name);
        fs::write(&manifest_path, format!("  {name}\t\n")).unwrap();
        let manifest = ReleasedManifest::load(&manifest_path).unwrap();
        assert_eq!(manifest.directories(), [name]);
    }
    fs::remove_dir_all(&root).unwrap();
}

fn generated_members(indices: &[(u8, u8)]) -> Vec<Member> {
    let mut seen = BTreeSet::new();
    let mut members = Vec::new();
    for (parent, field) in indices {
        let parent = format!("a:b/i/t{parent}");
        let name = format!("x{field}");
        if seen.insert((parent.clone(), name.clone())) {
            members.push(member(&parent, &name, MemberKind::Field, Some("u64")));
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

#[test]
fn every_mode_rejects_an_entry_from_another_package() {
    let root = fixture("versions/wit");
    let members = model::members(&load(&root.join("v0.2")).unwrap()).unwrap();
    let text = fs::read_to_string(root.join("v0.2/wire.lock")).unwrap();
    let foreign = format!("{text}other:pkg/types/user.x = 1 field u8\n");
    let previous =
        WireLock::parse(&fs::read_to_string(root.join("v0.1/wire.lock")).unwrap()).unwrap();
    for mode in [
        LockMode::Default,
        LockMode::First,
        LockMode::Against(&previous),
    ] {
        let mut lock = WireLock::parse(&foreign).unwrap();
        let error = lock.reconcile_with(&members, mode).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("other:pkg"), "{message}");
        assert!(lock.drift_with(&members, mode).is_err());
    }
}

#[test]
fn an_unchanged_lockfile_is_not_rewritten() {
    let dir = std::env::temp_dir().join(format!("witforge-lock-unchanged-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("wire.lock");
    let lock = WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64\n")).unwrap();
    lock.write_atomic(&path).unwrap();
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    // Make a rewrite observable on file systems with coarse timestamps.
    std::thread::sleep(std::time::Duration::from_millis(20));
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt as _;
        fs::metadata(&path).unwrap().ino()
    };
    lock.write_atomic(&path).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    }
    // A different rendering is written.
    let changed = WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u32\n")).unwrap();
    changed.write_atomic(&path).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), changed.render());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn lockfile_type_tokens_are_bounded_in_depth_and_length() {
    // 32 levels of container nesting is the documented maximum and must both lock and parse.
    let at_limit = format!("{}u8{}", "list<".repeat(32), ">".repeat(32));
    let contract_source =
        format!("package a:b@0.1.0;\ninterface i {{\n  record t {{ x: {at_limit} }}\n}}\n");
    let contract = witforge::load_sources(vec![witforge::SourceFile::new(
        "contract.wit".to_string(),
        contract_source,
    )])
    .unwrap();
    let built = model::build(&contract).unwrap();
    let members = built.members().unwrap();
    let mut lock = WireLock::default();
    lock.reconcile(&members).unwrap();
    let rendered = lock.render();
    let reparsed = WireLock::parse(&rendered).expect("32-level nested type must parse back");
    assert_eq!(
        reparsed.get("a:b/i/t.x").unwrap().ty.as_deref(),
        Some(at_limit.as_str())
    );

    // 33 levels fails consistently in both model building and lockfile parsing.
    let over_limit = format!("{}u8{}", "list<".repeat(33), ">".repeat(33));
    let over_contract = witforge::load_sources(vec![witforge::SourceFile::new(
        "contract.wit".to_string(),
        format!("package a:b@0.1.0;\ninterface i {{\n  record t {{ x: {over_limit} }}\n}}\n"),
    )])
    .unwrap();
    assert!(model::build(&over_contract).is_err());
    let error = format!(
        "{:#}",
        WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field {over_limit}\n")).unwrap_err()
    );
    assert!(error.contains("nesting"), "{error}");

    let long = format!("{}u8{}", "list<".repeat(300), ">".repeat(300));
    let error = format!(
        "{:#}",
        WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field {long}\n")).unwrap_err()
    );
    assert!(error.contains("byte limit"), "{error}");
}

#[test]
fn every_repository_lockfile_uses_the_tightened_grammar() {
    fn collect(directory: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                collect(&path, found);
            } else if entry.file_name() == "wire.lock" {
                found.push(path);
            }
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found = Vec::new();
    for directory in ["crates/witforge/tests/fixtures", "examples"] {
        collect(&root.join(directory), &mut found);
    }
    assert!(!found.is_empty(), "no repository lockfiles found");
    for path in found {
        let text = fs::read_to_string(&path).unwrap();
        WireLock::parse(&text).unwrap_or_else(|error| panic!("{}: {error:#}", path.display()));
    }
}

#[test]
fn number_assignment_is_linear_in_a_wide_contract() {
    let members: Vec<Member> = (0..16_000)
        .map(|index| {
            member(
                "a:b/i/t",
                &format!("m{index:05}"),
                MemberKind::Field,
                Some("u32"),
            )
        })
        .collect();
    let mut lock = WireLock::default();
    let changes = lock.reconcile(&members).unwrap();
    assert_eq!(changes.added.len(), 16_000);
    assert_eq!(lock.entries().count(), 16_000);
    assert_eq!(lock.get("a:b/i/t.m15999").unwrap().number, 16_000);
}

#[test]
fn every_released_discrepancy_is_collected_in_key_order() {
    let previous = WireLock::parse(&format!(
        "{HEADER}\n\
a:b/i/t.a = 1 field string\n\
a:b/i/t.b = 2 field string\n\
a:b/i/t.c = 3 field string\n\
a:b/i/t.d = 4 field string\n\
a:b/i/t.n1 = 5 field u8\n\
a:b/i/t.n2 = 6 field u8\n"
    ))
    .unwrap();
    let members = ["a", "b", "c", "d", "n2", "new1", "new2"]
        .into_iter()
        .map(|name| {
            member(
                "a:b/i/t",
                name,
                MemberKind::Field,
                Some(if matches!(name, "a" | "b" | "c" | "d") {
                    "u32"
                } else {
                    "u8"
                }),
            )
        })
        .collect::<Vec<_>>();
    let mut current = previous.clone();
    let report = current.reconcile_report(&members).unwrap();
    assert_eq!(
        report
            .discrepancies
            .iter()
            .map(|change| change.key.as_str())
            .collect::<Vec<_>>(),
        ["a:b/i/t.a", "a:b/i/t.b", "a:b/i/t.c", "a:b/i/t.d"]
    );
    assert_eq!(report.changes.reserved, vec![("a:b/i/t.n1".to_string(), 5)]);
    assert_eq!(
        report.changes.added,
        vec![
            ("a:b/i/t.new1".to_string(), 7),
            ("a:b/i/t.new2".to_string(), 8)
        ]
    );
}

#[test]
fn draft_removal_of_a_released_member_can_be_restored() {
    let previous = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.a = 1 field u64\na:b/i/t.b = 2 field u64\n"
    ))
    .unwrap();
    let members_a = [member("a:b/i/t", "a", MemberKind::Field, Some("u64"))];
    let members_a_c = [
        members_a[0].clone(),
        member("a:b/i/t", "c", MemberKind::Field, Some("u64")),
    ];
    let members_a_b_c = [
        members_a[0].clone(),
        member("a:b/i/t", "b", MemberKind::Field, Some("u64")),
        members_a_c[1].clone(),
    ];
    let mut current = previous.clone();
    current
        .reconcile_report_with(&members_a, LockMode::Against(&previous))
        .unwrap();
    assert!(current.get("a:b/i/t.b").unwrap().reserved);
    current
        .reconcile_report_with(&members_a_c, LockMode::Against(&previous))
        .unwrap();
    assert_eq!(current.get("a:b/i/t.c").unwrap().number, 3);

    let report = current
        .reconcile_report_with(&members_a_b_c, LockMode::Against(&previous))
        .unwrap();
    assert_eq!(report.restored, vec![("a:b/i/t.b".to_string(), 2)]);
    assert!(report.changes.is_empty(), "{report:?}");
    assert!(report.discrepancies.is_empty(), "{report:?}");
    assert!(!current.get("a:b/i/t.b").unwrap().reserved);
    assert_eq!(current.get("a:b/i/t.b").unwrap().number, 2);
    assert_eq!(current.get("a:b/i/t.c").unwrap().number, 3);

    let mut never_removed = previous.clone();
    never_removed
        .reconcile_report_with(&members_a_b_c, LockMode::Against(&previous))
        .unwrap();
    assert_eq!(current, never_removed);
}

#[test]
fn draft_removal_can_be_restored_for_every_member_kind() {
    let previous = WireLock::parse(&format!(
        "{HEADER}\n\
a:b/i/t.x = 1 field u64\n\
a:b/i/v.c = 1 variant-case u64\n\
a:b/i/e.c = 1 enum-case\n\
a:b/i/f.g = 1 flag\n\
a:b/i.m = 1 method u64\n\
a:b/i#m.p = 1 param u64\n"
    ))
    .unwrap();
    let members = vec![
        member("a:b/i/t", "x", MemberKind::Field, Some("u64")),
        member("a:b/i/v", "c", MemberKind::VariantCase, Some("u64")),
        member("a:b/i/e", "c", MemberKind::EnumCase, None),
        member("a:b/i/f", "g", MemberKind::Flag, None),
        member("a:b/i", "m", MemberKind::Method, Some("u64")),
        member("a:b/i#m", "p", MemberKind::Param, Some("u64")),
    ];
    let mut current = previous.clone();
    let report = current
        .reconcile_report_with(&[], LockMode::Against(&previous))
        .unwrap();
    assert_eq!(report.changes.reserved.len(), members.len());
    assert!(
        current.entries().all(|(_, entry)| entry.reserved),
        "every released entry should be reserved after the draft removal"
    );

    let report = current
        .reconcile_report_with(&members, LockMode::Against(&previous))
        .unwrap();
    assert_eq!(report.restored.len(), members.len());
    assert!(report.changes.is_empty(), "{report:?}");
    assert!(report.discrepancies.is_empty(), "{report:?}");
    assert!(
        current.entries().all(|(_, entry)| !entry.reserved),
        "every released entry should be restored"
    );
    let mut never_removed = previous.clone();
    never_removed
        .reconcile_report_with(&members, LockMode::Against(&previous))
        .unwrap();
    assert_eq!(current, never_removed);
}

#[test]
fn restoring_a_draft_removal_with_a_different_type_or_kind_is_refused() {
    let previous = WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64\n")).unwrap();
    let retyped = [member("a:b/i/t", "x", MemberKind::Field, Some("u16"))];
    let mut current = previous.clone();
    current
        .reconcile_report_with(&[], LockMode::Against(&previous))
        .unwrap();
    let report = current
        .reconcile_report_with(&retyped, LockMode::Against(&previous))
        .unwrap();
    assert!(
        report
            .discrepancies
            .iter()
            .any(|discrepancy| discrepancy.key == "a:b/i/t.x"
                && discrepancy.message.contains("kind or type")),
        "{report:?}"
    );
    assert!(current.get("a:b/i/t.x").unwrap().reserved);

    let rekinded = [member("a:b/i/t", "x", MemberKind::VariantCase, Some("u64"))];
    let report = current
        .reconcile_report_with(&rekinded, LockMode::Against(&previous))
        .unwrap();
    assert!(!report.discrepancies.is_empty(), "{report:?}");
    assert!(current.get("a:b/i/t.x").unwrap().reserved);
}

#[test]
fn a_member_reserved_in_the_baseline_cannot_be_restored() {
    let previous =
        WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64 reserved\n")).unwrap();
    let members = [member("a:b/i/t", "x", MemberKind::Field, Some("u64"))];
    let mut current = previous.clone();
    let report = current
        .reconcile_report_with(&members, LockMode::Against(&previous))
        .unwrap();
    assert_eq!(
        report.changes.reintroduced,
        vec![("a:b/i/t.x".to_string(), 1)]
    );
    assert!(current.get("a:b/i/t.x").unwrap().reserved);
}

#[test]
fn a_draft_member_removed_and_readded_gets_a_new_number() {
    let previous = WireLock::parse(&format!("{HEADER}\na:b/i/t.a = 1 field u64\n")).unwrap();
    let member = |name: &str| member("a:b/i/t", name, MemberKind::Field, Some("u64"));
    let mut current = previous.clone();
    current
        .reconcile_report_with(&[member("a"), member("c")], LockMode::Against(&previous))
        .unwrap();
    assert_eq!(current.get("a:b/i/t.c").unwrap().number, 2);

    current
        .reconcile_report_with(&[member("a")], LockMode::Against(&previous))
        .unwrap();
    assert!(current.get("a:b/i/t.c").is_none());

    current
        .reconcile_report_with(&[member("a"), member("d")], LockMode::Against(&previous))
        .unwrap();
    assert_eq!(current.get("a:b/i/t.d").unwrap().number, 2);

    current
        .reconcile_report_with(
            &[member("a"), member("c"), member("d")],
            LockMode::Against(&previous),
        )
        .unwrap();
    assert_eq!(current.get("a:b/i/t.c").unwrap().number, 3);
}

#[test]
fn case_variant_of_a_reserved_member_is_refused() {
    let mk = member;
    let cases = [
        ("a:b/i/r", MemberKind::Field, Some("u32")),
        ("a:b/i/v", MemberKind::VariantCase, Some("u32")),
        ("a:b/i/e", MemberKind::EnumCase, None),
        ("a:b/i/f", MemberKind::Flag, None),
        ("a:b/i", MemberKind::Method, Some("u32")),
        ("a:b/i#op", MemberKind::Param, Some("u32")),
    ];

    for (parent, kind, ty) in cases {
        let old_member = mk(parent, "foo-bar", kind, ty);
        let new_member = mk(parent, "FOO-bar", kind, ty);

        let mut baseline = WireLock::default();
        baseline
            .reconcile_report_with(&[old_member], LockMode::First)
            .unwrap();

        // One-step replacement of a released member by a case variant is refused.
        let mut one_step = baseline.clone();
        let report = one_step
            .reconcile_report_with(
                std::slice::from_ref(&new_member),
                LockMode::Against(&baseline),
            )
            .unwrap();
        assert_eq!(report.discrepancies.len(), 1, "{parent}: {report:?}");
        assert_eq!(report.discrepancies[0].key, new_member.key());
        assert!(
            report.discrepancies[0].message.contains("letter case"),
            "{parent}: {report:?}"
        );
        assert!(report.changes.added.is_empty(), "{parent}: {report:?}");
        assert_eq!(one_step, baseline, "refused reconcile must not mutate lock");

        let collisions = baseline
            .case_collisions(
                std::slice::from_ref(&new_member),
                LockMode::Against(&baseline),
            )
            .unwrap();
        assert_eq!(collisions.len(), 1, "{parent}: {collisions:#?}");
        assert_eq!(collisions[0].code, "WF1003");
        assert_eq!(
            collisions[0].path.as_deref(),
            Some(new_member.key().as_str())
        );

        // Two-step replacement: reserve `foo-bar` first, then add `FOO-bar`.
        let mut two_step = baseline.clone();
        two_step
            .reconcile_report_with(&[], LockMode::Against(&baseline))
            .unwrap();
        let before = two_step.clone();
        let report = two_step
            .reconcile_report_with(
                std::slice::from_ref(&new_member),
                LockMode::Against(&baseline),
            )
            .unwrap();
        assert_eq!(report.discrepancies.len(), 1, "{parent}: {report:?}");
        assert!(report.changes.added.is_empty(), "{parent}: {report:?}");
        assert_eq!(two_step, before);
        assert!(two_step.drift(std::slice::from_ref(&new_member)).is_err());
    }

    // A pre-existing lockfile that already holds both `foo-bar` (reserved) and `FOO-bar` (live)
    // is refused by `drift` and `case_collisions`.
    let mixed = WireLock::parse(&format!(
        "{HEADER}\na:b/i/r.FOO-bar = 2 field u32\na:b/i/r.foo-bar = 1 field u32 reserved\n"
    ))
    .unwrap();
    let live = [mk("a:b/i/r", "FOO-bar", MemberKind::Field, Some("u32"))];
    let report = mixed.drift_report(&live).unwrap();
    assert_eq!(report.discrepancies.len(), 1, "{report:?}");
    assert_eq!(report.discrepancies[0].key, "a:b/i/r.FOO-bar");
    let collisions = mixed.case_collisions(&live, LockMode::Default).unwrap();
    assert_eq!(collisions.len(), 1, "{collisions:#?}");
    assert_eq!(collisions[0].code, "WF1003");

    // Renaming an unreleased draft from `foo-bar` to `FOO-bar` drops the draft and succeeds.
    let mut draft = WireLock::default();
    let draft_old = mk("a:b/i/r", "foo-bar", MemberKind::Field, Some("u32"));
    draft
        .reconcile_report_with(&[draft_old], LockMode::First)
        .unwrap();
    let report = draft.reconcile_report_with(&live, LockMode::First).unwrap();
    assert!(report.discrepancies.is_empty(), "{report:?}");
    assert!(draft.get("a:b/i/r.foo-bar").is_none());
    assert!(draft.get("a:b/i/r.FOO-bar").is_some());

    // Members in different parents do not collide.
    let other_parent = [
        mk("a:b/i/r", "foo-bar", MemberKind::Field, Some("u32")),
        mk("a:b/i/s", "FOO-bar", MemberKind::Field, Some("u32")),
    ];
    let mut separate = WireLock::default();
    let report = separate
        .reconcile_report_with(&other_parent, LockMode::First)
        .unwrap();
    assert!(report.discrepancies.is_empty(), "{report:?}");
}

#[test]
fn live_case_collisions_match_between_reconcile_and_case_collisions() {
    let live_collision = [
        member("a:b/i/r", "foo-bar", MemberKind::Field, Some("u32")),
        member("a:b/i/r", "FOO-bar", MemberKind::Field, Some("u32")),
    ];
    let mut fresh = WireLock::default();
    let reconcile_report = fresh
        .reconcile_report_with(&live_collision, LockMode::First)
        .unwrap();
    let direct_collisions = WireLock::default()
        .case_collisions(&live_collision, LockMode::First)
        .unwrap();
    assert_eq!(reconcile_report.discrepancies.len(), 1);
    assert_eq!(direct_collisions.len(), 1);
    assert_eq!(
        direct_collisions[0].path.as_deref(),
        Some(reconcile_report.discrepancies[0].key.as_str())
    );
    assert_eq!(
        direct_collisions[0].message,
        reconcile_report.discrepancies[0].message
    );
}
