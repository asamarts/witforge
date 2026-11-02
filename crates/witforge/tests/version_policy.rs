//! Version-shape and memberless-definition policy tests.

use std::fs;
use std::path::{Path, PathBuf};

use witforge::emit::rust;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "witforge-version-policy-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    copy_tree(&fixture("versions/wit"), &path.join("wit"));
    path
}

#[test]
fn docs_only_changes_in_a_draft_leave_wire_and_api_shape_alone() {
    let root = temp("docs");
    let contract = root.join("wit/v0.2/contract.wit");
    let text = fs::read_to_string(&contract).unwrap();
    let changed = text.replace(
        "interface types {",
        "/// Documentation added in a draft minor.\ninterface types {",
    );
    assert_ne!(text, changed);
    fs::write(&contract, changed).unwrap();
    let lock_before = fs::read(root.join("wit/v0.2/wire.lock")).unwrap();

    let files = rust::plan(&root.join("wit")).unwrap();
    let version = files.files().get("v0_2.rs").unwrap();
    assert!(
        version.contains("/// Documentation added in a draft minor."),
        "{version}"
    );
    assert_eq!(
        fs::read(root.join("wit/v0.2/wire.lock")).unwrap(),
        lock_before
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_memberless_addition_in_a_new_minor_has_no_wire_entry() {
    let root = temp("memberless");
    let contract = root.join("wit/v0.2/contract.wit");
    let text = fs::read_to_string(&contract).unwrap();
    let changed = text.replacen(
        "interface types {",
        "interface types {\n    /// A memberless alias added in a draft minor.\n    type generated-alias = u32;",
        1,
    );
    assert_ne!(text, changed);
    fs::write(&contract, changed).unwrap();
    let lock_before = fs::read(root.join("wit/v0.2/wire.lock")).unwrap();

    let files = rust::plan(&root.join("wit")).unwrap();
    let version = files.files().get("v0_2.rs").unwrap();
    assert!(
        version.contains("pub type GeneratedAlias = u32;"),
        "{version}"
    );
    assert_eq!(
        fs::read(root.join("wit/v0.2/wire.lock")).unwrap(),
        lock_before
    );
    assert!(
        !String::from_utf8_lossy(&lock_before).contains("generated-alias"),
        "a memberless definition must not acquire a wire entry"
    );
    fs::remove_dir_all(&root).unwrap();
}
