//! Version-to-version conversion tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::emit::rust;
use witforge::lock::WireLock;
use witforge::{diff, is_contract_error, load, model};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("witforge-convert-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn write_version(root: &Path, name: &str, package_version: &str, body: &str) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("contract.wit"),
        format!("package ex:t@{package_version};\n{body}"),
    )
    .unwrap();
    let model = model::build(&load(&dir).unwrap()).unwrap();
    let mut lock = WireLock::default();
    lock.reconcile(&model.members().unwrap()).unwrap();
    fs::write(dir.join("wire.lock"), lock.render()).unwrap();
    dir
}

#[test]
fn diff_passing_implies_generation_succeeds_for_alias_evolutions() {
    let cases: &[(&str, &str, &str, bool)] = &[
        (
            "alias-to-target",
            "interface types { type id = u32; record r { a: id } }",
            "interface types { type id = u32; record r { a: u32 } }",
            true,
        ),
        (
            "target-to-alias",
            "interface types { record r { a: u32 } }",
            "interface types { type id = u32; record r { a: id } }",
            true,
        ),
        (
            "alias-to-alias",
            "interface types { type id = u32; type other = u32; record r { a: id } }",
            "interface types { type id = u32; type other = u32; record r { a: other } }",
            true,
        ),
        (
            "named-list-to-expansion",
            "interface types { type ids = list<u32>; record r { a: ids } }",
            "interface types { type ids = list<u32>; record r { a: list<u32> } }",
            true,
        ),
        (
            "nested-positions",
            "interface types {\n\
               type id = u32;\n\
               record r { a: list<id>, b: option<id>, c: result<id, string>, d: list<list<id>> }\n\
               variant v { x(id) }\n\
             }",
            "interface types {\n\
               type id = u32;\n\
               record r { a: list<u32>, b: option<u32>, c: result<u32, string>, d: list<list<u32>> }\n\
               variant v { x(u32) }\n\
             }",
            true,
        ),
        (
            "use-alias",
            "interface types { type id = u32; }\n\
             interface api { use types.{id as local}; record r { a: local } }",
            "interface types { type id = u32; }\n\
             interface api { use types.{id as local}; record r { a: u32 } }",
            true,
        ),
        (
            "changed-alias-target",
            "interface types { type id = u32; record r { a: id } }",
            "interface types { type id = u64; record r { a: id } }",
            false,
        ),
    ];
    for (name, old_contract, new_contract, diff_passes) in cases {
        let root = temp(name);
        write_version(&root, "v0.1", "0.1.0", old_contract);
        write_version(&root, "v0.2", "0.2.0", new_contract);
        let report = diff::compare(&root.join("v0.1"), &root.join("v0.2")).unwrap();
        assert_eq!(
            report.summary.breaking == 0,
            *diff_passes,
            "{name}: {report:?}"
        );
        let generated = rust::plan(&root);
        if *diff_passes {
            assert!(generated.is_ok(), "{name}: {generated:?}");
        } else {
            let error = generated.unwrap_err();
            assert!(is_contract_error(&error), "{name}: {error:#}");
        }
        fs::remove_dir_all(&root).unwrap();
    }
}

#[test]
fn foreign_alias_spellings_share_the_resolved_conversion() {
    let root = temp("foreign-alias");
    for version in ["v0.1", "v0.2"] {
        let shared = root.join(version).join("deps/shared");
        fs::create_dir_all(&shared).unwrap();
        fs::write(
            shared.join("types.wit"),
            "package example:shared@0.1.0;\ninterface types { type id = u32; }\n",
        )
        .unwrap();
    }
    write_version(
        &root,
        "v0.1",
        "0.1.0",
        "interface api {\n\
           use example:shared/types@0.1.0.{id as local};\n\
           record r { a: local }\n\
         }",
    );
    write_version(
        &root,
        "v0.2",
        "0.2.0",
        "interface api {\n\
           use example:shared/types@0.1.0.{id as local};\n\
           use example:shared/types@0.1.0.{id};\n\
           record r { a: id }\n\
         }",
    );
    let report = diff::compare(&root.join("v0.1"), &root.join("v0.2")).unwrap();
    assert_eq!(report.summary.breaking, 0, "{report:?}");
    assert!(rust::plan(&root).is_ok());
    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn generated_conversions_compile_and_apply_across_two_versions() {
    let root = fixture("versions/convert");
    let out = temp("run").join("generated");
    rust::generate(&root, &out).unwrap();
    assert_eq!(rust::check(&root, &out).unwrap(), Vec::<String>::new());
    let source = fs::read_to_string(out.join("convert.rs")).unwrap();
    assert!(source.contains("pub mod v0_1_to_v0_2 {"), "{source}");
    assert!(source.contains("pub mod v0_2_to_v0_1 {"), "{source}");
    assert!(source.contains("pub enum ConversionError"), "{source}");
    assert!(source.contains("pub fn user("), "{source}");
    assert!(source.contains("pub fn maybe_id("), "{source}");
    assert!(!source.contains('\r'), "convert.rs contains CRLF");

    let parent = out.parent().unwrap();
    let check = parent.join("check.rs");
    fs::write(
        &check,
        r#"
#[path = "generated/mod.rs"]
mod generated;

fn main() {
    use generated::v0_1::types as old;
    use generated::v0_2::types as new;

    let old_user = old::User {
        name: "alice".to_string(),
        id: 7,
        names: vec!["a".to_string()],
        maybe_id: Some(9),
        outcome: Ok(11),
        tags: vec!["tag".to_string()],
        nickname: Some("al".to_string()),
    };
    let new_user = generated::convert::v0_1_to_v0_2::types::user(&old_user).unwrap();
    assert_eq!(new_user.name, "alice");
    assert_eq!(new_user.id, 7);
    assert_eq!(new_user.names, vec!["a".to_string()]);
    assert_eq!(new_user.maybe_id, Some(9));
    assert_eq!(new_user.outcome, Ok(11));
    assert_eq!(new_user.email, None);
    assert!(new_user.scores.is_empty());

    let back = generated::convert::v0_2_to_v0_1::types::user(&new_user).unwrap();
    assert_eq!(back.name, old_user.name);
    assert_eq!(back.id, old_user.id);
    assert_eq!(back.names, old_user.names);
    assert_eq!(back.maybe_id, old_user.maybe_id);
    assert_eq!(back.outcome, old_user.outcome);
    assert_eq!(back.tags, old_user.tags);
    assert_eq!(back.nickname, old_user.nickname);

    assert_eq!(generated::convert::v0_1_to_v0_2::types::id(&4).unwrap(), 4);
    assert_eq!(
        generated::convert::v0_1_to_v0_2::types::names(&vec!["x".to_string()]).unwrap(),
        vec!["x".to_string()]
    );
    assert_eq!(
        generated::convert::v0_1_to_v0_2::types::maybe_id(&Some(3)).unwrap(),
        Some(3)
    );
    assert_eq!(
        generated::convert::v0_1_to_v0_2::types::outcome(&Err("no".to_string())).unwrap(),
        Err("no".to_string())
    );

    let event = old::Event::Progress(5);
    let converted = generated::convert::v0_1_to_v0_2::types::event(&event).unwrap();
    assert!(matches!(converted, new::Event::Progress(5)));
    assert!(
        generated::convert::v0_2_to_v0_1::types::event(&new::Event::Finished).is_err()
    );
    let event_back = generated::convert::v0_2_to_v0_1::types::event(&converted).unwrap();
    assert!(matches!(event_back, old::Event::Progress(5)));

    assert!(matches!(
        generated::convert::v0_1_to_v0_2::types::state(&old::State::Idle).unwrap(),
        new::State::Idle
    ));
    assert!(
        generated::convert::v0_2_to_v0_1::types::state(&new::State::Stopped).is_err()
    );

    let old_options = old::Options {
        verbose: true,
        quiet: true,
    };
    let new_options = generated::convert::v0_1_to_v0_2::types::options(&old_options).unwrap();
    assert!(new_options.verbose);
    assert!(!new_options.strict);
    let options_back =
        generated::convert::v0_2_to_v0_1::types::options(&new_options).unwrap();
    assert!(options_back.verbose);
    assert!(!options_back.quiet);

    let error = generated::convert::v0_2_to_v0_1::types::event(&new::Event::Finished)
        .unwrap_err();
    assert!(
        format!("{error}").contains("types/event"),
        "conversion error did not name the path: {error}"
    );
}
"#,
    )
    .unwrap();
    let binary = parent.join(format!("check{}", std::env::consts::EXE_SUFFIX));
    let status = Command::new("rustc")
        .args(["--edition=2024", "-o"])
        .arg(&binary)
        .arg(&check)
        .status()
        .unwrap();
    assert!(status.success(), "generated conversions did not compile");
    let status = Command::new(&binary).status().unwrap();
    assert!(
        status.success(),
        "generated conversions returned the wrong value"
    );
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn a_required_addition_is_refused() {
    let base = temp("required");
    let root = base.join("wit");
    for version in ["v0.1", "v0.2"] {
        fs::create_dir_all(root.join(version)).unwrap();
        for name in ["contract.wit", "wire.lock"] {
            fs::copy(
                fixture("versions/wit").join(version).join(name),
                root.join(version).join(name),
            )
            .unwrap();
        }
    }
    fs::copy(
        fixture("versions/wit").join("released"),
        root.join("released"),
    )
    .unwrap();
    let contract = root.join("v0.2/contract.wit");
    let text = fs::read_to_string(&contract)
        .unwrap()
        .replace("new-field: option<string>,", "new-field: string,");
    fs::write(&contract, text).unwrap();
    let lock = root.join("v0.2/wire.lock");
    let text = fs::read_to_string(&lock).unwrap().replace(
        "user.new-field = 5 field option<string>",
        "user.new-field = 5 field string",
    );
    fs::write(&lock, text).unwrap();

    let error = rust::plan(&root).unwrap_err();
    let message = format!("{error:#}");
    assert!(is_contract_error(&error), "{message}");
    assert!(message.contains("types/user.new-field"), "{message}");
    assert!(message.contains("option<T>"), "{message}");
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn a_retyped_alias_is_refused() {
    let base = temp("retyped");
    let root = base.join("wit");
    for (version, target) in [("v0.1", "u32"), ("v0.2", "string")] {
        let directory = root.join(version);
        fs::create_dir_all(&directory).unwrap();
        let number = version.strip_prefix('v').unwrap();
        fs::write(
            directory.join("contract.wit"),
            format!(
                "package example:retype@{number}.0;\ninterface types {{ type id = {target}; }}\n"
            ),
        )
        .unwrap();
        fs::write(directory.join("wire.lock"), "# witforge wire lockfile v2\n").unwrap();
    }
    let error = rust::plan(&root).unwrap_err();
    let message = format!("{error:#}");
    assert!(is_contract_error(&error), "{message}");
    assert!(message.contains("types/id"), "{message}");
    assert!(message.contains("changes type"), "{message}");
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn a_removed_required_member_is_reported_by_the_reverse_conversion() {
    let root = fixture("versions/wit");
    let out = temp("removed").join("generated");
    rust::generate(&root, &out).unwrap();
    let parent = out.parent().unwrap();
    let check = parent.join("check.rs");
    fs::write(
        &check,
        r#"
#[path = "generated/mod.rs"]
mod generated;

fn main() {
    let old = generated::v0_1::types::User {
        name: "alice".to_string(),
        old_field: 3,
        rename_me: "old".to_string(),
    };
    let new = generated::convert::v0_1_to_v0_2::types::user(&old).unwrap();
    assert_eq!(new.new_field, None);
    assert_eq!(new.renamed, None);
    let error = generated::convert::v0_2_to_v0_1::types::user(&new).unwrap_err();
    assert!(matches!(
        error,
        generated::convert::ConversionError::MissingMember { path }
            if path == "types/user.old-field"
    ));
}
"#,
    )
    .unwrap();
    let binary = parent.join(format!("check{}", std::env::consts::EXE_SUFFIX));
    let status = Command::new("rustc")
        .args(["--edition=2024", "-o"])
        .arg(&binary)
        .arg(&check)
        .status()
        .unwrap();
    assert!(status.success(), "generated conversions did not compile");
    let status = Command::new(&binary).status().unwrap();
    assert!(
        status.success(),
        "reverse conversion did not report the removed field"
    );
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn a_single_version_has_no_conversion_module() {
    let base = temp("single");
    let root = base.join("wit");
    fs::create_dir_all(root.join("v0.1")).unwrap();
    for name in ["contract.wit", "wire.lock"] {
        fs::copy(
            fixture("versions/convert/v0.1").join(name),
            root.join("v0.1").join(name),
        )
        .unwrap();
    }
    let files = rust::plan(&root).unwrap();
    assert!(!files.files().contains_key("convert.rs"));
    assert!(files.files().contains_key("mod.rs"));
    assert!(files.files().contains_key("v0_1.rs"));
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn check_treats_convert_as_shared_generated_output() {
    let root = fixture("versions/convert");
    let out = temp("drift").join("generated");
    rust::generate(&root, &out).unwrap();
    fs::write(
        out.join("convert.rs"),
        format!("{}\nchanged\n", rust::HEADER),
    )
    .unwrap();
    let messages = rust::check(&root, &out).unwrap();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("convert.rs") && message.contains("changed")),
        "{messages:?}"
    );
    fs::remove_file(out.join("convert.rs")).unwrap();
    let messages = rust::check(&root, &out).unwrap();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("convert.rs") && message.contains("missing")),
        "{messages:?}"
    );
    fs::remove_dir_all(out.parent().unwrap()).unwrap();
}

#[test]
fn a_recursive_alias_is_refused_before_conversion_generation() {
    let root = temp("cycle");
    let version = root.join("v0.1");
    fs::create_dir_all(&version).unwrap();
    fs::write(
        version.join("contract.wit"),
        "package example:cycle@0.1.0;\n\ninterface types {\n    type loop = loop;\n}\n",
    )
    .unwrap();
    fs::write(
        version.join("wire.lock"),
        witforge::lock::WireLock::default().render(),
    )
    .unwrap();

    let error = rust::plan(&root).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("depends on itself"), "{message}");
    fs::remove_dir_all(&root).unwrap();
}
