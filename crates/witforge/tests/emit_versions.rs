//! Version-to-version conversion tests.

mod support;

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
    assert_eq!(new_user.scores.len(), 0);

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
    assert!(message.contains("0.1.0 -> 0.2.0"), "{message}");
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
    assert!(message.contains("0.1.0 -> 0.2.0"), "{message}");
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
        generated::convert::ConversionError::MissingMember {
            from: "0.2.0",
            to: "0.1.0",
            path: "types/user.old-field",
            ..
        }
    ));
    let rendered = format!("{error}");
    assert!(rendered.contains("0.2.0 -> 0.1.0"), "{rendered}");
    assert!(rendered.contains("types/user.old-field"), "{rendered}");
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

#[allow(clippy::too_many_lines)]
#[test]
fn convert_hardening_consumer_from_impls_clippy_empty_pair_and_hop_errors() {
    use witforge::lock::LockMode;

    let base = temp("hardening");
    let wit_root = base.join("wit");

    // v0.1 -> v0.2 -> v0.3 form a three-version chain with nested records/lists/options/results,
    // dropped fields/flags, removed required member, and removed variant case; v1.0 has no
    // shared types with v0.3 so v0.3 <-> v1.0 exercises an empty conversion pair.
    let v01 = wit_root.join("v0.1");
    fs::create_dir_all(&v01).unwrap();
    fs::write(
        v01.join("contract.wit"),
        "package ex:chain@0.1.0;\n\
         interface types {\n\
           record leaf { id: u32, label: string }\n\
           enum status { ok, legacy }\n\
           variant event { leaf(leaf), legacy, done }\n\
           flags opts { fast }\n\
           record envelope {\n\
             title: string,\n\
             leaves: list<leaf>,\n\
             maybe-leaf: option<leaf>,\n\
             maybe-leaves: list<option<leaf>>,\n\
             outcome: result<leaf, status>,\n\
             bare: result,\n\
             ev: event,\n\
             opts: opts,\n\
           }\n\
         }\n",
    )
    .unwrap();
    let m01 = model::build(&load(&v01).unwrap()).unwrap();
    let mut l01 = WireLock::default();
    l01.reconcile_with(&m01.members().unwrap(), LockMode::First)
        .unwrap();
    fs::write(v01.join("wire.lock"), l01.render()).unwrap();

    let v02 = wit_root.join("v0.2");
    fs::create_dir_all(&v02).unwrap();
    fs::write(
        v02.join("contract.wit"),
        "package ex:chain@0.2.0;\n\
         interface types {\n\
           record leaf { id: u32, label: string, extra: option<string> }\n\
           enum status { ok, legacy, middle }\n\
           variant event { leaf(leaf), legacy, middle, done }\n\
           flags opts { fast, strict }\n\
           record envelope {\n\
             title: string,\n\
             leaves: list<leaf>,\n\
             maybe-leaf: option<leaf>,\n\
             maybe-leaves: list<option<leaf>>,\n\
             outcome: result<leaf, status>,\n\
             bare: result,\n\
             ev: event,\n\
             opts: opts,\n\
             note: option<string>,\n\
           }\n\
         }\n",
    )
    .unwrap();
    let m02 = model::build(&load(&v02).unwrap()).unwrap();
    let mut l02 = l01.clone();
    l02.reconcile_with(&m02.members().unwrap(), LockMode::Against(&l01))
        .unwrap();
    fs::write(v02.join("wire.lock"), l02.render()).unwrap();

    let v03 = wit_root.join("v0.3");
    fs::create_dir_all(&v03).unwrap();
    fs::write(
        v03.join("contract.wit"),
        "package ex:chain@0.3.0;\n\
         interface types {\n\
           record leaf { id: u32, extra: option<string> }\n\
           enum status { ok, middle }\n\
           variant event { leaf(leaf), middle, done }\n\
           flags opts { fast, strict }\n\
           record envelope {\n\
             title: string,\n\
             leaves: list<leaf>,\n\
             maybe-leaf: option<leaf>,\n\
             maybe-leaves: list<option<leaf>>,\n\
             outcome: result<leaf, status>,\n\
             bare: result,\n\
             ev: event,\n\
             opts: opts,\n\
             note: option<string>,\n\
           }\n\
         }\n",
    )
    .unwrap();
    let m03 = model::build(&load(&v03).unwrap()).unwrap();
    let mut l03 = l02.clone();
    l03.reconcile_with(&m03.members().unwrap(), LockMode::Against(&l02))
        .unwrap();
    fs::write(v03.join("wire.lock"), l03.render()).unwrap();

    let v10 = wit_root.join("v1.0");
    fs::create_dir_all(&v10).unwrap();
    fs::write(
        v10.join("contract.wit"),
        "package ex:chain@1.0.0;\n\
         interface fresh {\n\
           record unrelated { count: u64 }\n\
         }\n",
    )
    .unwrap();
    let m10 = model::build(&load(&v10).unwrap()).unwrap();
    let mut l10 = WireLock::default();
    l10.reconcile_with(&m10.members().unwrap(), LockMode::First)
        .unwrap();
    fs::write(v10.join("wire.lock"), l10.render()).unwrap();

    let crate_dir = base.join("crate");
    let src = crate_dir.join("src");
    let generated_dir = src.join("generated");
    fs::create_dir_all(&src).unwrap();
    rust::generate(&wit_root, &generated_dir).unwrap();

    let convert_rs = fs::read_to_string(generated_dir.join("convert.rs")).unwrap();
    assert!(
        convert_rs.contains("pub mod v0_3_to_v1_0 {}"),
        "empty version pair left unused imports:\n{convert_rs}"
    );
    assert!(
        convert_rs.contains("pub mod v1_0_to_v0_3 {}"),
        "empty reverse version pair left unused imports:\n{convert_rs}"
    );
    assert!(
        convert_rs.contains("dropped without reporting them"),
        "missing drop doc comment on reverse conversions:\n{convert_rs}"
    );
    assert!(
        convert_rs.contains("Inside the including crate itself it has no"),
        "missing #[non_exhaustive] scope doc comment:\n{convert_rs}"
    );
    assert!(
        !convert_rs.contains("Result::Ok(value."),
        "found untyped Result::Ok(value...):\n{convert_rs}"
    );
    assert!(
        !convert_rs.contains("Result::Ok(::core::clone::Clone::clone"),
        "found untyped Result::Ok(Clone::clone...):\n{convert_rs}"
    );
    assert!(
        !convert_rs.contains("Clone::clone(&value.bare)"),
        "Copy `result` field must be copied directly, not cloned:\n{convert_rs}"
    );

    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\n\
         name = \"witforge-convert-hardening\"\n\
         version = \"0.0.0\"\n\
         edition = \"2024\"\n\
         rust-version = \"1.88\"\n\
         publish = false\n\
         \n\
         [workspace]\n\
         resolver = \"3\"\n",
    )
    .unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"
pub mod generated;

use generated::convert::ConversionError;

// Consumer-defined From impls on ConversionError plus From<ConversionError> for a consumer error.
impl From<std::io::Error> for ConversionError {
    fn from(_: std::io::Error) -> Self {
        ConversionError::MissingMember {
            from: "consumer",
            to: "consumer",
            path: "io",
        }
    }
}

impl From<std::fmt::Error> for ConversionError {
    fn from(_: std::fmt::Error) -> Self {
        ConversionError::MissingMember {
            from: "consumer",
            to: "consumer",
            path: "fmt",
        }
    }
}

#[derive(Debug)]
pub enum ConsumerError {
    Convert(ConversionError),
}

impl From<ConversionError> for ConsumerError {
    fn from(err: ConversionError) -> Self {
        Self::Convert(err)
    }
}

pub fn chain_v01_to_v03(
    input: &generated::v0_1::types::Envelope,
) -> Result<generated::v0_3::types::Envelope, ConsumerError> {
    let mid = generated::convert::v0_1_to_v0_2::types::envelope(input)?;
    let out = generated::convert::v0_2_to_v0_3::types::envelope(&mid)?;
    Ok(out)
}

pub fn chain_v03_to_v01(
    input: &generated::v0_3::types::Envelope,
) -> Result<generated::v0_1::types::Envelope, ConsumerError> {
    let mid = generated::convert::v0_3_to_v0_2::types::envelope(input)?;
    let out = generated::convert::v0_2_to_v0_1::types::envelope(&mid)?;
    Ok(out)
}
"#,
    )
    .unwrap();
    fs::write(
        src.join("main.rs"),
        r#"
use witforge_convert_hardening::{
    ConsumerError, chain_v01_to_v03, chain_v03_to_v01, generated,
};
use generated::convert::ConversionError;

fn main() {
    use generated::v0_1::types as v1;
    use generated::v0_3::types as v3;

    // Valid chain v0.1 -> v0.2 -> v0.3 succeeds.
    let ok_v1 = v1::Envelope {
        title: "hello".to_string(),
        leaves: vec![v1::Leaf {
            id: 1,
            label: "a".to_string(),
        }],
        maybe_leaf: Some(v1::Leaf {
            id: 2,
            label: "b".to_string(),
        }),
        maybe_leaves: vec![
            None,
            Some(v1::Leaf {
                id: 3,
                label: "c".to_string(),
            }),
        ],
        outcome: Ok(v1::Leaf {
            id: 4,
            label: "d".to_string(),
        }),
        bare: Ok(()),
        ev: v1::Event::Done,
        opts: v1::Opts { fast: true },
    };
    let ok_v3 = chain_v01_to_v03(&ok_v1).unwrap();
    assert_eq!(ok_v3.leaves[0].id, 1);
    assert_eq!(ok_v3.leaves[0].extra, None);

    // Chained forward failure on the second hop names 0.2.0 -> 0.3.0.
    let legacy_v1 = v1::Envelope {
        ev: v1::Event::Legacy,
        ..ok_v1
    };
    let ConsumerError::Convert(err) = chain_v01_to_v03(&legacy_v1).unwrap_err();
    assert_eq!(
        err,
        ConversionError::UnknownCase {
            from: "0.2.0",
            to: "0.3.0",
            path: "types/event",
            case: "legacy",
        }
    );
    assert!(format!("{err}").contains("0.2.0 -> 0.3.0"), "{err}");

    // Chained reverse failure on the first hop (missing required `label` removed in v0.3)
    // names 0.3.0 -> 0.2.0.
    let v3_val = v3::Envelope {
        title: "hello".to_string(),
        leaves: vec![v3::Leaf { id: 1, extra: Some("dropped".to_string()) }],
        maybe_leaf: None,
        maybe_leaves: vec![],
        outcome: Err(v3::Status::Ok),
        bare: Err(()),
        ev: v3::Event::Done,
        opts: v3::Opts { fast: true, strict: true },
        note: Some("dropped".to_string()),
    };
    let ConsumerError::Convert(err) = chain_v03_to_v01(&v3_val).unwrap_err();
    assert_eq!(
        err,
        ConversionError::MissingMember {
            from: "0.3.0",
            to: "0.2.0",
            path: "types/leaf.label",
        }
    );
    assert!(format!("{err}").contains("0.3.0 -> 0.2.0"), "{err}");

    // New-to-old drops added fields and flags without reporting them when all required fields
    // remain present.
    let v2_val = generated::v0_2::types::Envelope {
        title: "keep".to_string(),
        leaves: vec![],
        maybe_leaf: None,
        maybe_leaves: vec![],
        outcome: Err(generated::v0_2::types::Status::Ok),
        bare: Ok(()),
        ev: generated::v0_2::types::Event::Done,
        opts: generated::v0_2::types::Opts {
            fast: true,
            strict: true,
        },
        note: Some("dropped-without-error".to_string()),
    };
    let back_v1 = generated::convert::v0_2_to_v0_1::types::envelope(&v2_val).unwrap();
    assert_eq!(back_v1.title, "keep");
    assert!(back_v1.opts.fast);
}
"#,
    )
    .unwrap();

    let target = base.join("cargo-target");
    let clippy = Command::new(env!("CARGO"))
        .args(["clippy", "--offline", "--all-targets", "--manifest-path"])
        .arg(crate_dir.join("Cargo.toml"))
        .args(["--", "-D", "warnings"])
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_TERM_COLOR", "never")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(
        clippy.status.success(),
        "clippy failed on generated convert.rs:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&clippy.stdout),
        String::from_utf8_lossy(&clippy.stderr)
    );

    let run = Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--offline", "--manifest-path"])
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_TERM_COLOR", "never")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "convert hardening binary failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    fs::remove_dir_all(&base).unwrap();
}

/// Extract `// - <from> -> <to> <type_path>: <reason>` lines from `convert.rs`'s top comment block.
fn parse_convert_omission_lines(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| line.strip_prefix("// - ").map(str::to_string))
        .collect()
}

fn format_diff_omissions(omissions: &[diff::ConversionOmission]) -> Vec<String> {
    omissions
        .iter()
        .map(|o| {
            format!(
                "{} -> {} {}: {}",
                o.from_version, o.to_version, o.type_path, o.reason
            )
        })
        .collect()
}

fn write_dep(version_dir: &Path, dep_dir: &str, file: &str, body: &str) {
    let dir = version_dir.join("deps").join(dep_dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(file), body).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn major_boundary_reproduction_and_dependencies_compile_and_match_diff() {
    use support::{HarnessBatch, QualityChecks, RustEdition};
    use witforge::emit::{json_schema, proto};

    let base = temp("major-boundary");
    let mut batch = HarnessBatch::new(
        "major-boundary",
        RustEdition::E2024,
        QualityChecks {
            fmt: true,
            clippy: true,
            missing_docs: false,
            rustdoc: false,
            doctest: false,
        },
    );

    // Base v0.1 / v0.2 contracts shared across the reproduction rows (v0.2 adds an optional field).
    let v01_body = "\
interface types {
    record keep { x: u32 }
    record item { id: u32, label: string }
    record outer { child: item, items: list<item>, maybe: option<item>, res: result<item, string> }
    variant wrap { carried(item), empty }
}
";
    let v02_body = "\
interface types {
    record keep { x: u32, note: option<string> }
    record item { id: u32, label: string }
    record outer { child: item, items: list<item>, maybe: option<item>, res: result<item, string> }
    variant wrap { carried(item), empty }
}
";

    let build_pair = |name: &str, v10_body: &str| -> PathBuf {
        let root = base.join(name);
        write_version(&root, "v0.1", "0.1.0", v01_body);
        write_version(&root, "v0.2", "0.2.0", v02_body);
        write_version(&root, "v1.0", "1.0.0", v10_body);
        root
    };

    // Row 1: identical contract across major — zero omissions, both directions generated.
    let row1 = build_pair("row1_identical", v02_body);
    let r1_plan = rust::plan(&row1).unwrap();
    let r1_convert = r1_plan.files().get("convert.rs").unwrap();
    assert!(
        parse_convert_omission_lines(r1_convert).is_empty(),
        "{r1_convert}"
    );
    assert_eq!(
        diff::compare_omissions(&row1.join("v0.2"), &row1.join("v1.0")).unwrap(),
        Vec::new()
    );
    batch.add_wit_root(
        "row1_identical",
        &row1,
        false,
        false,
        r#"
        use crate::row1_identical::plain;
        let old = plain::v0_2::types::Keep { x: 7, note: Some("hi".to_string()) };
        let new = plain::convert::v0_2_to_v1_0::types::keep(&old).unwrap();
        assert_eq!(new.x, 7);
        let back = plain::convert::v1_0_to_v0_2::types::keep(&new).unwrap();
        assert_eq!(back, old);
        "#,
    );

    // Row 2: removed required field — forward drops it, reverse exists & returns MissingMember.
    let row2 = build_pair(
        "row2_removed_req",
        "\
interface types {
    record keep { x: u32, note: option<string> }
    record item { id: u32 }
    record outer { child: item, items: list<item>, maybe: option<item>, res: result<item, string> }
    variant wrap { carried(item), empty }
}
",
    );
    let r2_plan = rust::plan(&row2).unwrap();
    let r2_convert = r2_plan.files().get("convert.rs").unwrap();
    assert!(
        parse_convert_omission_lines(r2_convert).is_empty(),
        "{r2_convert}"
    );
    let r2_analysis = diff::compare_with_impacts(&row2.join("v0.2"), &row2.join("v1.0")).unwrap();
    assert_eq!(r2_analysis.conversion_impacts.len(), 1);
    assert_eq!(
        r2_analysis.conversion_impacts[0].missing_path,
        "types/item.label"
    );
    batch.add_wit_root(
        "row2_removed_req",
        &row2,
        false,
        false,
        r#"
        use crate::row2_removed_req::plain;
        let v02 = plain::v0_2::types::Item { id: 1, label: "a".to_string() };
        let v10 = plain::convert::v0_2_to_v1_0::types::item(&v02).unwrap();
        assert_eq!(v10.id, 1);
        let err = plain::convert::v1_0_to_v0_2::types::item(&v10).unwrap_err();
        assert!(format!("{err}").contains("types/item.label"), "{err}");
        "#,
    );

    // Row 3: retyped field (`u32` -> `string`) — omits `item` and containing types in both
    // directions, while `keep` still converts in both directions.
    let row3 = build_pair(
        "row3_retype",
        "\
interface types {
    record keep { x: u32, note: option<string> }
    record item { id: string, label: string }
    record outer { child: item, items: list<item>, maybe: option<item>, res: result<item, string> }
    variant wrap { carried(item), empty }
}
",
    );
    let r3_convert = rust::plan(&row3)
        .unwrap()
        .files()
        .get("convert.rs")
        .unwrap()
        .clone();
    let r3_diff = diff::compare_omissions(&row3.join("v0.2"), &row3.join("v1.0")).unwrap();
    assert_eq!(
        parse_convert_omission_lines(&r3_convert),
        format_diff_omissions(&r3_diff)
    );
    assert_eq!(r3_diff.len(), 6, "{r3_diff:?}"); // item, outer, wrap x 2 directions
    assert!(r3_convert.contains("pub fn keep("), "{r3_convert}");
    batch.add_wit_root(
        "row3_retype",
        &row3,
        false,
        false,
        r#"
        use crate::row3_retype::plain;
        let old = plain::v0_2::types::Keep { x: 9, note: None };
        let new = plain::convert::v0_2_to_v1_0::types::keep(&old).unwrap();
        assert_eq!(new.x, 9);
        let item = plain::v1_0::types::Item { id: "s".to_string(), label: "l".to_string() };
        assert_eq!(item.id, "s");
        "#,
    );

    // Row 4: kind change (`record` -> `enum`) + variant payload-presence change.
    let row4 = build_pair(
        "row4_kind_change",
        "\
interface types {
    record keep { x: u32, note: option<string> }
    enum item { a, b }
    record outer { child: item, items: list<item>, maybe: option<item>, res: result<item, string> }
    variant wrap { carried, empty }
}
",
    );
    let r4_convert = rust::plan(&row4)
        .unwrap()
        .files()
        .get("convert.rs")
        .unwrap()
        .clone();
    let r4_diff = diff::compare_omissions(&row4.join("v0.2"), &row4.join("v1.0")).unwrap();
    assert_eq!(
        parse_convert_omission_lines(&r4_convert),
        format_diff_omissions(&r4_diff)
    );
    assert!(
        r4_diff
            .iter()
            .any(|o| o.type_path == "types/item" && o.reason.contains("changes kind"))
    );
    assert!(
        r4_diff
            .iter()
            .any(|o| o.type_path == "types/wrap" && o.reason.contains("payload presence"))
    );
    batch.add_wit_root(
        "row4_kind_change",
        &row4,
        false,
        false,
        r"
        use crate::row4_kind_change::plain;
        let old = plain::v0_2::types::Keep { x: 2, note: None };
        let new = plain::convert::v0_2_to_v1_0::types::keep(&old).unwrap();
        assert_eq!(new.x, 2);
        let _ = plain::v1_0::types::Item::A;
        ",
    );

    // Row 5: required addition — omitted ONLY in forward (`0.2.0 -> 1.0.0`, including transitive
    // containers `outer` and `wrap`), while reverse (`1.0.0 -> 0.2.0`) IS generated for ALL types!
    let row5_v10 = "\
interface types {
    record keep { x: u32, note: option<string> }
    record item { id: u32, label: string, required-new: u64 }
    record outer { child: item, items: list<item>, maybe: option<item>, res: result<item, string> }
    variant wrap { carried(item), empty }
}
";
    let row5 = build_pair("row5_req_add", row5_v10);
    let r5_convert = rust::plan(&row5)
        .unwrap()
        .files()
        .get("convert.rs")
        .unwrap()
        .clone();
    let r5_diff = diff::compare_omissions(&row5.join("v0.2"), &row5.join("v1.0")).unwrap();
    assert_eq!(
        parse_convert_omission_lines(&r5_convert),
        format_diff_omissions(&r5_diff)
    );
    // Only 0.2.0 -> 1.0.0 omits item, outer, wrap; 1.0.0 -> 0.2.0 has ZERO omissions!
    assert_eq!(r5_diff.len(), 3, "{r5_diff:?}");
    assert!(
        r5_diff
            .iter()
            .all(|o| o.from_version.major == 0 && o.to_version.major == 1),
        "{r5_diff:?}"
    );
    batch.add_wit_root(
        "row5_req_add",
        &row5,
        false,
        false,
        r#"
        use crate::row5_req_add::plain;
        // Reverse 1.0 -> 0.2 IS generated for item, outer, and wrap (drops required-new)!
        let item_v1 = plain::v1_0::types::Item {
            id: 5,
            label: "ok".to_string(),
            required_new: 99,
        };
        let outer_v1 = plain::v1_0::types::Outer {
            child: item_v1.clone(),
            items: vec![item_v1.clone()],
            maybe: Some(item_v1.clone()),
            res: Ok(item_v1.clone()),
        };
        let outer_v0 = plain::convert::v1_0_to_v0_2::types::outer(&outer_v1).unwrap();
        assert_eq!(outer_v0.child.id, 5);
        assert_eq!(outer_v0.items.len(), 1);
        let wrap_v0 = plain::convert::v1_0_to_v0_2::types::wrap(
            &plain::v1_0::types::Wrap::Carried(item_v1),
        )
        .unwrap();
        assert!(matches!(wrap_v0, plain::v0_2::types::Wrap::Carried(ref i) if i.id == 5));
        "#,
    );

    // Verify the EXACT SAME required addition inside a major (`v0.2` -> `v0.3`) is STILL refused!
    let intra = base.join("intra_major_refused");
    write_version(&intra, "v0.2", "0.2.0", v02_body);
    write_version(&intra, "v0.3", "0.3.0", row5_v10);
    let intra_err = rust::plan(&intra).unwrap_err();
    assert!(is_contract_error(&intra_err), "{intra_err:#}");
    assert!(
        format!("{intra_err:#}").contains("required-new"),
        "{intra_err:#}"
    );

    // Row 6: renamed type used by a record (`item` -> `entry`).
    let row6 = build_pair(
        "row6_rename",
        "\
interface types {
    record keep { x: u32, note: option<string> }
    record entry { id: u32, label: string }
    record outer { child: entry, items: list<entry>, maybe: option<entry>, res: result<entry, string> }
    variant wrap { carried(entry), empty }
}
",
    );
    let r6_convert = rust::plan(&row6)
        .unwrap()
        .files()
        .get("convert.rs")
        .unwrap()
        .clone();
    let r6_diff = diff::compare_omissions(&row6.join("v0.2"), &row6.join("v1.0")).unwrap();
    assert_eq!(
        parse_convert_omission_lines(&r6_convert),
        format_diff_omissions(&r6_diff)
    );
    assert!(r6_diff.iter().any(|o| {
        o.from_version.major == 0
            && o.type_path == "types/item"
            && o.reason.contains("no namesake in 1.0.0")
    }));
    assert!(r6_diff.iter().any(|o| {
        o.from_version.major == 1
            && o.type_path == "types/entry"
            && o.reason.contains("no namesake in 0.2.0")
    }));
    assert!(
        r6_diff
            .iter()
            .any(|o| o.type_path == "types/outer" && o.reason.contains("changes type"))
    );
    batch.add_wit_root(
        "row6_rename",
        &row6,
        false,
        false,
        r#"
        use crate::row6_rename::plain;
        let old = plain::v0_2::types::Keep { x: 4, note: None };
        assert_eq!(plain::convert::v0_2_to_v1_0::types::keep(&old).unwrap().x, 4);
        let _ = plain::v1_0::types::Entry { id: 1, label: "e".to_string() };
        "#,
    );

    // Row 7a: changed in-place dep (same dep major), upgraded dep (0.1.0 -> 1.0.0), unchanged
    // shared dep, and newly added dep in v1.0.
    let row7 = base.join("row7_deps_mixed");
    let v0_stable =
        "package example:stable@0.1.0;\ninterface common { record tag { key: string } }\n";
    let v0_tweaked = "package example:tweaked@0.1.0;\ninterface common { record rec { n: u32 } }\n";
    let v1_tweaked =
        "package example:tweaked@0.1.0;\ninterface common { record rec { n: string } }\n";
    let v0_upgraded =
        "package example:upgraded@0.1.0;\ninterface common { record val { x: u32 } }\n";
    let v1_upgraded =
        "package example:upgraded@1.0.0;\ninterface common { record val { x: u64 } }\n";
    let v1_added =
        "package example:extra@0.1.0;\ninterface common { record bonus { flag: bool } }\n";

    let v02_dep_wit = "\
interface api {
    use example:stable/common@0.1.0.{tag};
    use example:tweaked/common@0.1.0.{rec};
    use example:upgraded/common@0.1.0.{val};
    record stable-holder { t: tag }
    record tweaked-holder { r: rec }
    record upgraded-holder { v: list<val> }
}
";
    let v10_dep_wit = "\
interface api {
    use example:stable/common@0.1.0.{tag};
    use example:tweaked/common@0.1.0.{rec};
    use example:upgraded/common@1.0.0.{val};
    use example:extra/common@0.1.0.{bonus};
    record stable-holder { t: tag }
    record tweaked-holder { r: rec }
    record upgraded-holder { v: list<val> }
    record extra-holder { b: bonus }
}
";
    let r7_v02 = row7.join("v0.2");
    write_dep(&r7_v02, "stable", "c.wit", v0_stable);
    write_dep(&r7_v02, "tweaked", "c.wit", v0_tweaked);
    write_dep(&r7_v02, "upgraded", "c.wit", v0_upgraded);
    write_version(&row7, "v0.2", "0.2.0", v02_dep_wit);

    let r7_v10 = row7.join("v1.0");
    write_dep(&r7_v10, "stable", "c.wit", v0_stable);
    write_dep(&r7_v10, "tweaked", "c.wit", v1_tweaked);
    write_dep(&r7_v10, "upgraded", "c.wit", v1_upgraded);
    write_dep(&r7_v10, "extra", "c.wit", v1_added);
    write_version(&row7, "v1.0", "1.0.0", v10_dep_wit);

    let r7_rust = rust::plan(&row7).unwrap();
    let r7_proto = proto::plan(&row7).unwrap();
    let r7_schema = json_schema::plan(&row7).unwrap();
    let r7_convert = r7_rust.files().get("convert.rs").unwrap();
    let r7_diff = diff::compare_omissions(&r7_v02, &r7_v10).unwrap();
    assert_eq!(
        parse_convert_omission_lines(r7_convert),
        format_diff_omissions(&r7_diff)
    );
    // `stable-holder` converts cleanly across the major; tweaked/upgraded/extra-holder are omitted.
    assert!(r7_convert.contains("pub fn stable_holder("), "{r7_convert}");
    assert!(
        !r7_convert.contains("pub fn tweaked_holder("),
        "{r7_convert}"
    );
    assert!(
        !r7_convert.contains("pub fn upgraded_holder("),
        "{r7_convert}"
    );
    // Proto & schema disambiguate same-dep-major `tweaked`, keep standard names for `upgraded` v0/v1.
    assert!(r7_proto.files().contains_key("v0.example.tweaked.v0.proto"));
    assert!(r7_proto.files().contains_key("v1.example.tweaked.v0.proto"));
    assert!(r7_proto.files().contains_key("example.upgraded.v0.proto"));
    assert!(r7_proto.files().contains_key("example.upgraded.v1.proto"));
    assert!(r7_proto.files().contains_key("example.stable.v0.proto"));
    assert!(r7_proto.files().contains_key("example.extra.v0.proto"));
    assert!(
        r7_schema
            .files()
            .contains_key("v0.example.tweaked.v0.schema.json")
    );
    assert!(
        r7_schema
            .files()
            .contains_key("v1.example.tweaked.v0.schema.json")
    );
    assert!(
        r7_schema
            .files()
            .contains_key("example.upgraded.v0.schema.json")
    );
    assert!(
        r7_schema
            .files()
            .contains_key("example.upgraded.v1.schema.json")
    );

    batch.add_wit_root(
        "row7_deps_mixed",
        &row7,
        true,
        true,
        r#"
        use crate::row7_deps_mixed::{glue, plain, proto};
        use prost::Message as _;
        let h0 = plain::v0_2::api::StableHolder {
            t: plain::witforge_deps::example_stable::common::Tag { key: "k".to_string() },
        };
        let h1 = plain::convert::v0_2_to_v1_0::api::stable_holder(&h0).unwrap();
        assert_eq!(h1.t.key, "k");

        let t0 = plain::v0_2::api::TweakedHolder {
            r: plain::witforge_deps::v0::example_tweaked::common::Rec { n: 42 },
        };
        let p0 = glue::v0::api::tweaked_holder::to_proto(&t0);
        let rt0 = glue::v0::api::tweaked_holder::from_proto(
            proto::ex::t::v0::ApiTweakedHolder::decode(p0.encode_to_vec().as_slice()).unwrap(),
        )
        .unwrap();
        assert_eq!(rt0, t0);

        let t1 = plain::v1_0::api::TweakedHolder {
            r: plain::witforge_deps::v1::example_tweaked::common::Rec { n: "hi".to_string() },
        };
        let p1 = glue::v1::api::tweaked_holder::to_proto(&t1);
        let rt1 = glue::v1::api::tweaked_holder::from_proto(
            proto::ex::t::v1::ApiTweakedHolder::decode(p1.encode_to_vec().as_slice()).unwrap(),
        )
        .unwrap();
        assert_eq!(rt1, t1);
        "#,
    );

    // Row 7b: v0 has NO dependencies and v1 ADDS a dependency (verifies glue proto_base_override).
    let dep_added_root = base.join("row7b_dep_added");
    write_version(
        &dep_added_root,
        "v0.1",
        "0.1.0",
        "interface api { record item { id: u32 } }\n",
    );
    write_dep(&dep_added_root.join("v1.0"), "extra", "c.wit", v1_added);
    write_version(
        &dep_added_root,
        "v1.0",
        "1.0.0",
        "interface api {\n\
           use example:extra/common@0.1.0.{bonus};\n\
           record item { id: u32, extra: option<bonus> }\n\
         }\n",
    );
    batch.add_wit_root(
        "row7b_dep_added",
        &dep_added_root,
        false,
        false,
        r"
        use crate::row7b_dep_added::{glue, plain};
        let v0 = plain::v0_1::api::Item { id: 3 };
        let v1 = plain::convert::v0_1_to_v1_0::api::item(&v0).unwrap();
        assert_eq!(v1.id, 3);
        assert_eq!(v1.extra, None);
        let p0 = glue::v0::api::item::to_proto(&v0);
        assert_eq!(glue::v0::api::item::from_proto(p0).unwrap(), v0);
        ",
    );

    // Row 7c: 3 majors where v0 and v1 share identical dep `0.1.0` and v2 upgrades it to `1.0.0`!
    // `witforge_deps::v1` re-exports `super::v0::example_upgraded` so `v0.1 <-> v1.0` still converts!
    let three_majors_root = base.join("row7c_three_majors");
    for (dir, ver, dep_src, dep_ver) in [
        ("v0.1", "0.1.0", v0_upgraded, "0.1.0"),
        ("v1.0", "1.0.0", v0_upgraded, "0.1.0"),
        ("v2.0", "2.0.0", v1_upgraded, "1.0.0"),
    ] {
        write_dep(&three_majors_root.join(dir), "upgraded", "c.wit", dep_src);
        write_version(
            &three_majors_root,
            dir,
            ver,
            &format!(
                "interface api {{\n\
                   use example:upgraded/common@{dep_ver}.{{val}};\n\
                   record holder {{ v: val }}\n\
                 }}\n"
            ),
        );
    }
    let r7c_deps = rust::plan(&three_majors_root)
        .unwrap()
        .files()
        .get("witforge_deps.rs")
        .unwrap()
        .clone();
    assert!(
        r7c_deps.contains("pub use super::v0::example_upgraded;"),
        "{r7c_deps}"
    );
    assert_eq!(
        diff::compare_omissions(
            &three_majors_root.join("v0.1"),
            &three_majors_root.join("v1.0")
        )
        .unwrap(),
        Vec::new()
    );
    let three_major_omissions = diff::compare_omissions(
        &three_majors_root.join("v1.0"),
        &three_majors_root.join("v2.0"),
    )
    .unwrap();
    // Both `api/holder` and the local `use` alias `api/val` reference the upgraded dependency,
    // so both are omitted in each direction (4 total), matching `convert.rs`.
    let three_majors_convert = rust::plan(&three_majors_root)
        .unwrap()
        .files()
        .get("convert.rs")
        .unwrap()
        .clone();
    assert_eq!(
        parse_convert_omission_lines(&three_majors_convert),
        format_diff_omissions(&three_major_omissions)
    );
    assert_eq!(three_major_omissions.len(), 4, "{three_major_omissions:?}");
    batch.add_wit_root(
        "row7c_three_majors",
        &three_majors_root,
        false,
        false,
        r"
        use crate::row7c_three_majors::plain;
        let v0 = plain::v0_1::api::Holder {
            v: plain::witforge_deps::v0::example_upgraded::common::Val { x: 11 },
        };
        let v1 = plain::convert::v0_1_to_v1_0::api::holder(&v0).unwrap();
        assert_eq!(v1.v.x, 11);
        ",
    );

    // Row 7d: single-major-only dependency `example:wrapper` added in `v1.0` that references
    // a scoped dependency (`example:tweaked` changed between `v0.1` and `v1.0`). Transitive
    // scoping must place `example:wrapper` under `witforge_deps::v1` (and disambiguate its
    // proto/schema files) so Rust, proto, JSON Schema, and glue all compile cleanly!
    let transitive_scoped_root = base.join("row7d_transitive_scoped");
    let v1_wrapper = "\
package example:wrapper@0.1.0;
interface api {
    use example:tweaked/common@0.1.0.{rec};
    record wrapped { inner: rec }
}
";
    write_dep(
        &transitive_scoped_root.join("v0.1"),
        "tweaked",
        "c.wit",
        v0_tweaked,
    );
    write_version(
        &transitive_scoped_root,
        "v0.1",
        "0.1.0",
        "interface api {\n\
           use example:tweaked/common@0.1.0.{rec};\n\
           record holder { r: rec }\n\
         }\n",
    );
    write_dep(
        &transitive_scoped_root.join("v1.0"),
        "tweaked",
        "c.wit",
        v1_tweaked,
    );
    write_dep(
        &transitive_scoped_root.join("v1.0"),
        "wrapper",
        "c.wit",
        v1_wrapper,
    );
    write_version(
        &transitive_scoped_root,
        "v1.0",
        "1.0.0",
        "interface api {\n\
           use example:tweaked/common@0.1.0.{rec};\n\
           use example:wrapper/api@0.1.0.{wrapped};\n\
           record holder { r: rec, w: wrapped }\n\
         }\n",
    );
    let trans_rust = rust::plan(&transitive_scoped_root).unwrap();
    let trans_deps = trans_rust.files().get("witforge_deps.rs").unwrap();
    assert!(
        trans_deps.contains("pub mod v1 {") && trans_deps.contains("pub mod example_wrapper"),
        "{trans_deps}"
    );
    let trans_proto = proto::plan(&transitive_scoped_root).unwrap();
    assert!(
        trans_proto
            .files()
            .contains_key("v1.example.wrapper.v0.proto")
    );
    batch.add_wit_root(
        "row7d_transitive_scoped",
        &transitive_scoped_root,
        true,
        true,
        r#"
        use crate::row7d_transitive_scoped::{glue, plain, proto};
        use prost::Message as _;
        let h1 = plain::v1_0::api::Holder {
            r: plain::witforge_deps::v1::example_tweaked::common::Rec { n: "a".to_string() },
            w: plain::witforge_deps::v1::example_wrapper::api::Wrapped {
                inner: plain::witforge_deps::v1::example_tweaked::common::Rec {
                    n: "b".to_string(),
                },
            },
        };
        let p1 = glue::v1::api::holder::to_proto(&h1);
        let rt1 = glue::v1::api::holder::from_proto(
            proto::ex::t::v1::ApiHolder::decode(p1.encode_to_vec().as_slice()).unwrap(),
        )
        .unwrap();
        assert_eq!(rt1, h1);
        "#,
    );

    batch.verify_and_run("major_boundary");
    fs::remove_dir_all(&base).unwrap();
}
