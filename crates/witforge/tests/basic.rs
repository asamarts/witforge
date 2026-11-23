//! End-to-end tests over the fixture contracts.

use std::fs;
use std::path::Path;
use std::process::Command;

use witforge::lock::{HEADER, WireLock};
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
    assert_eq!(violations.len(), 0, "{violations:#?}");
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
        cases >= 20,
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
    edited.push(model::Member::new(
        "example:plugin/types/span".into(),
        "line".into(),
        model::MemberKind::Field,
        Some("u64".into()),
    ));
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
    assert_eq!(changes.added.len(), 0);
    assert_eq!(changes.reserved.len(), 0);
    assert!(!changes.is_empty(), "{changes:?}");
    assert_eq!(lock.drift(&members).unwrap().reintroduced, expected);
    let end = lock.get("example:plugin/types/span.end").unwrap();
    assert!(end.reserved && end.number == 2);
    assert_eq!(lock, older);
}

#[test]
fn append_only_check_refuses_renumbering_and_deletion() {
    let older = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.y = 2 field u64 reserved\n"
    ))
    .unwrap();
    let renumbered = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 3 field u64\na:b/i/t.y = 2 field u64 reserved\n"
    ))
    .unwrap();
    assert!(renumbered.check_append_only(&older).is_err());
    let deleted = WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 1 field u64\n")).unwrap();
    assert!(deleted.check_append_only(&older).is_err());
    let unreserved = WireLock::parse(&format!(
        "{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.y = 2 field u64\n"
    ))
    .unwrap();
    assert!(unreserved.check_append_only(&older).is_err());
}

#[test]
fn number_assignment_skips_the_protobuf_reserved_range_and_stops_at_the_maximum() {
    // 18999 is the last usable number before protobuf's reserved range.
    let mut lock = WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 18999 field u64\n")).unwrap();
    let member = model::Member::new(
        "a:b/i/t".into(),
        "y".into(),
        model::MemberKind::Field,
        Some("u64".into()),
    );
    let changes = lock.reconcile(&[member]).unwrap();
    assert_eq!(changes.added, vec![("a:b/i/t.y".to_string(), 20_000)]);
    assert_eq!(lock.get("a:b/i/t.y").unwrap().number, 20_000);

    // The last valid field number cannot be followed by another member in the same parent.
    let mut full =
        WireLock::parse(&format!("{HEADER}\na:b/i/t.x = 536870911 field u64\n")).unwrap();
    let member = model::Member::new(
        "a:b/i/t".into(),
        "y".into(),
        model::MemberKind::Field,
        Some("u64".into()),
    );
    let err = full.reconcile(&[member]).unwrap_err();
    assert!(format!("{err:#}").contains("no protobuf field numbers left"));
    assert!(full.get("a:b/i/t.y").is_none());
}

#[test]
fn parse_rejects_bad_lockfiles() {
    for bad in [
        format!("{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.x = 2 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field u64\na:b/i/t.y = 1 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 19500 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 0 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 1 maybe\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field u64 extra\n"),
        format!("{HEADER}\na:b/i/t.x = 01 field u64\n"),
        format!("{HEADER}\na:b/i/t.x = 1 enum-case string\n"),
        format!("{HEADER}\na:b/i/t.x = 1 param\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field list<>\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field char\n"),
        format!("{HEADER}\na:b/i/t.x = 1 field result<u8>\n"),
        "nonsense\n".to_string(),
        "# witforge wire lockfile: append-only. Do not edit numbers by hand.\na:b/i/t.x = 1\n"
            .to_string(),
    ] {
        assert!(WireLock::parse(&bad).is_err(), "{bad:?} should fail");
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn public_data_types_are_extensible_and_constructible_downstream() {
    let root = std::env::temp_dir().join(format!(
        "witforge-public-api-consumer-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    let dependency = env!("CARGO_MANIFEST_DIR")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"witforge-public-api-consumer\"\nversion = \"0.0.0\"\n\
             edition = \"2021\"\n\n[dependencies]\nwitforge = {{ path = \"{dependency}\" }}\n\n[workspace]\n"
        ),
    )
    .unwrap();

    let positive = r#"
use witforge::diagnostic::{Diagnostic, LocatedDiagnostic, Position, RelatedLocation, Severity};
use witforge::lock::{Entry, PackageName};
use witforge::model::{
    Case, Dependency, Field, Flag, Function, Interface, Member, MemberKind, Model, Package, Param,
    QualifiedName, ResultExpr, Stability, TypeDef, TypeDefKind, TypeExpr, Version, World,
    WorldInterface,
};

pub fn construct() {
    let version = Version::new(0, 1, 0);
    let qualified = QualifiedName::new("types".into(), "value".into());
    let result = ResultExpr::new(Some(Box::new(TypeExpr::Named(qualified))), None);
    let field = Field::new("value".into(), String::new(), TypeExpr::U64);
    let case = Case::new("some".into(), String::new(), Some(TypeExpr::U64));
    let flag = Flag::new("enabled".into(), String::new());
    let type_def = TypeDef::new(
        "value".into(),
        String::new(),
        Stability::Unknown,
        TypeDefKind::Record(vec![field]),
    );
    let param = Param::new("input".into(), TypeExpr::Result(result));
    let function = Function::new(
        "call".into(),
        String::new(),
        Stability::Unknown,
        vec![param],
        Some(TypeExpr::U64),
    );
    let interface = Interface::new(
        "types".into(),
        String::new(),
        Stability::Unknown,
        vec![type_def],
        vec![function],
    );
    let package = Package::new("example".into(), "api".into(), version, String::new());
    let world_interface =
        WorldInterface::new("types".into(), None, String::new(), Stability::Unknown);
    let world = World::new(
        "plugin".into(),
        String::new(),
        Stability::Unknown,
        vec![world_interface],
        Vec::new(),
    );
    let dependency = Dependency::new(package.clone(), vec![interface.clone()]);
    let model = Model::new(package, vec![interface], vec![world], vec![dependency]);
    let member = Member::new(
        "example:api/types/value".into(),
        "value".into(),
        MemberKind::Field,
        Some("u64".into()),
    );
    let package_name = PackageName::new("example".into(), "api".into());
    let entry = Entry::new(1, MemberKind::Field, Some("u64".into()), false);
    let position = Position::new(1, 1, 1, 2);
    let related = RelatedLocation::new("contract.wit".into(), Some(position), "declared here".into());
    let diagnostic = Diagnostic::new("WF0000", Severity::Note, "note");
    let located = LocatedDiagnostic::from_diagnostic(diagnostic).with_related(related);
    let _ = (model, member, package_name, entry, case, flag, located);
}
"#;
    fs::write(root.join("src/lib.rs"), positive).unwrap();
    let target = root.join("target");
    let manifest = root.join("Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args(["check", "--offline", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "public constructors did not compile:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let negative = r#"
use witforge::diagnostic::{Position, Severity};
use witforge::lock::{LockMode, PackageName};
use witforge::model::{Stability, Version};

pub fn structs() {
    let _ = Version { major: 0, minor: 1, patch: 0 };
    let _ = PackageName { namespace: "example".into(), name: "api".into() };
    let _ = Position { line: 1, column: 1, end_line: 1, end_column: 2 };
}

pub fn enums(stability: Stability, mode: LockMode<'_>, severity: Severity) {
    match stability {
        Stability::Unknown | Stability::Unstable { .. } | Stability::Stable { .. } => {}
    }
    match mode {
        LockMode::Default | LockMode::First | LockMode::Against(_) => {}
    }
    match severity {
        Severity::Error | Severity::Warning | Severity::Note => {}
    }
}
"#;
    fs::write(root.join("src/lib.rs"), negative).unwrap();
    let output = Command::new(env!("CARGO"))
        .args(["check", "--offline", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "non-exhaustive API unexpectedly compiled"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.matches("error[E0639]").count() >= 3,
        "expected struct-literal refusals:\n{stderr}"
    );
    assert!(
        stderr.matches("error[E0004]").count() >= 3,
        "expected exhaustive-match refusals:\n{stderr}"
    );
    fs::remove_dir_all(root).unwrap();
}

fn temp_wit_dir(name: &str, contract: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("witforge-basic-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("contract.wit"), contract).unwrap();
    dir
}

#[test]
fn named_interface_imports_and_exports_get_their_own_message() {
    let dir = temp_wit_dir(
        "named-vs-inline",
        r"package example:bad@0.1.0;

interface types {
    record item { id: u32 }
}

world plugin {
    import named-in: types;
    export named-out: types;
    import inline-in: interface {
        ping: func();
    }
    import world-fn: func();
}
",
    );
    let contract = load(&dir).unwrap();
    let violations = subset::check(&contract);
    assert_eq!(violations.len(), 4, "{violations:#?}");

    let named_in = violations
        .iter()
        .find(|v| v.location == "example:bad/plugin/named-in")
        .unwrap();
    assert!(
        named_in
            .message
            .contains("named interface imports are not supported"),
        "{named_in}"
    );
    assert!(!named_in.message.contains("inline"), "{named_in}");

    let named_out = violations
        .iter()
        .find(|v| v.location == "example:bad/plugin/named-out")
        .unwrap();
    assert!(
        named_out
            .message
            .contains("named interface exports are not supported"),
        "{named_out}"
    );
    assert!(!named_out.message.contains("inline"), "{named_out}");

    let inline_in = violations
        .iter()
        .find(|v| v.location == "example:bad/plugin/inline-in")
        .unwrap();
    assert!(
        inline_in
            .message
            .contains("inline interfaces are not supported"),
        "{inline_in}"
    );

    let world_fn = violations
        .iter()
        .find(|v| v.location == "example:bad/plugin#world-fn")
        .unwrap();
    assert!(
        world_fn
            .message
            .contains("functions must be defined in an interface"),
        "{world_fn}"
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn unknown_and_unsupported_annotations_are_refused() {
    // Known annotations (`@since`, `@unstable`, `@deprecated`) remain accepted.
    let valid = temp_wit_dir(
        "known-annotations",
        r"package example:ok@0.2.0;

@since(version = 0.1.0)
interface types {
    @since(version = 0.1.0)
    @deprecated(version = 0.2.0)
    record old { id: u32 }

    @unstable(feature = preview)
    ping: func();
}
",
    );
    let violations = subset::check(&load(&valid).unwrap());
    assert!(violations.is_empty(), "{violations:#?}");
    fs::remove_dir_all(&valid).unwrap();

    // `@external-id` is parsed by `wit-parser` on types, uses, functions, and world items, and
    // must be refused by the subset checker.
    let external_id = temp_wit_dir(
        "external-id-all",
        r#"package example:bad@0.1.0;

interface base {
    record shared { id: u32 }
}

interface api {
    @external-id("use-id")
    use base.{shared};

    @external-id("type-id")
    record item { s: shared }

    @external-id("func-id")
    get: func() -> item;
}

world plugin {
    @external-id("world-use-id")
    use base.{shared};

    @external-id("world-export-id")
    export api;
}
"#,
    );
    let diagnostics = subset::check_diagnostics(&load(&external_id).unwrap());
    assert_eq!(diagnostics.len(), 5, "{diagnostics:#?}");
    for expected_path in [
        "example:bad/api/shared",
        "example:bad/api/item",
        "example:bad/api#get",
        "example:bad/plugin/shared",
        "example:bad/plugin/api",
    ] {
        assert!(
            diagnostics.iter().any(|d| {
                d.code == "WF1014"
                    && d.path.as_deref() == Some(expected_path)
                    && d.message.contains("@external-id")
            }),
            "missing @external-id diagnostic for {expected_path}: {diagnostics:#?}"
        );
    }
    fs::remove_dir_all(&external_id).unwrap();

    // Unknown annotations on an interface, a type, a function, and a world item are refused.
    for (case, body) in [
        (
            "iface",
            "package example:bad@0.1.0;\n@custom(x = 1)\ninterface types { record r { a: u32 } }\n",
        ),
        (
            "type",
            "package example:bad@0.1.0;\ninterface types { @custom(x = 1)\nrecord r { a: u32 } }\n",
        ),
        (
            "func",
            "package example:bad@0.1.0;\ninterface types { @custom(x = 1)\nping: func(); }\n",
        ),
        (
            "world-item",
            "package example:bad@0.1.0;\ninterface types { record r { a: u32 } }\nworld w { @custom(x = 1)\nexport types; }\n",
        ),
        (
            "iface-external-id",
            "package example:bad@0.1.0;\n@external-id(\"x\")\ninterface types { record r { a: u32 } }\n",
        ),
    ] {
        let dir = temp_wit_dir(&format!("unknown-attr-{case}"), body);
        assert!(load(&dir).is_err(), "{case} should be refused");
        fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
fn case_insensitive_scope_collisions_are_refused() {
    // Two interfaces in one package differing only in ASCII letter case pass `wit-parser` and
    // must be refused by `subset::check` with `WF1003`.
    let dup_ifaces = temp_wit_dir(
        "case-ifaces",
        r"package example:bad@0.1.0;

interface api {
    record a { id: u32 }
}

interface API {
    record b { id: u32 }
}
",
    );
    let diagnostics = subset::check_diagnostics(&load(&dup_ifaces).unwrap());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].code, "WF1003");
    assert_eq!(diagnostics[0].path.as_deref(), Some("example:bad/API"));
    assert!(
        diagnostics[0].message.contains("letter case"),
        "{diagnostics:#?}"
    );
    fs::remove_dir_all(&dup_ifaces).unwrap();

    // Two worlds in one package differing only in ASCII letter case are refused with `WF1003`.
    let dup_worlds = temp_wit_dir(
        "case-worlds",
        r"package example:bad@0.1.0;

interface api {
    ping: func();
}

world plugin {
    export api;
}

world PLUGIN {
    export api;
}
",
    );
    let diagnostics = subset::check_diagnostics(&load(&dup_worlds).unwrap());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].code, "WF1003");
    assert_eq!(diagnostics[0].path.as_deref(), Some("example:bad/PLUGIN"));
    fs::remove_dir_all(&dup_worlds).unwrap();

    // Two live members, two types, two functions, or two parameters in one scope that differ only
    // in ASCII letter case are refused at load/check time.
    for (case, body) in [
        (
            "fields",
            "package example:bad@0.1.0;\ninterface api { record r { foo-bar: u32, FOO-bar: u32 } }\n",
        ),
        (
            "types",
            "package example:bad@0.1.0;\ninterface api { record foo { x: u32 } record FOO { y: u32 } }\n",
        ),
        (
            "functions",
            "package example:bad@0.1.0;\ninterface api { foo-bar: func(); FOO-bar: func(); }\n",
        ),
        (
            "params",
            "package example:bad@0.1.0;\ninterface api { f: func(foo-bar: u32, FOO-bar: u32); }\n",
        ),
    ] {
        let dir = temp_wit_dir(&format!("case-live-{case}"), body);
        let outcome = load(&dir).map(|contract| subset::check(&contract));
        assert!(
            outcome.is_err() || !outcome.unwrap().is_empty(),
            "{case} should be refused"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    // Names differing in case in DIFFERENT scopes do not collide.
    let distinct_scopes = temp_wit_dir(
        "case-distinct-scopes",
        r"package example:ok@0.1.0;

interface alpha {
    record item { foo-bar: u32 }
    record other { FOO-bar: u32 }
    f: func(foo-bar: u32);
    g: func(FOO-bar: u32);
}

interface beta {
    record ITEM { id: u32 }
    F: func();
}
",
    );
    let diagnostics = subset::check_diagnostics(&load(&distinct_scopes).unwrap());
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    fs::remove_dir_all(&distinct_scopes).unwrap();
}
