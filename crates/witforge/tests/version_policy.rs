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

#[test]
fn near_miss_version_directories_and_files_are_refused() {
    let root = temp("near-miss");
    let wit = root.join("wit");

    // Unrelated entries not starting with v/V + digit remain ignored.
    fs::create_dir_all(wit.join("vendor")).unwrap();
    fs::write(wit.join("versions.md"), "# notes\n").unwrap();
    fs::write(wit.join(".hidden"), "").unwrap();
    fs::write(wit.join(".v0.3"), "").unwrap();
    rust::plan(&wit).unwrap();

    let near_misses = [
        "v0.2.0",
        "V0.2",
        "v0.2-rc1",
        "v00.2",
        "v0.02",
        "v1",
        "v1.",
        "v99999999999999999999999999.0",
        "v0.99999999999999999999999999",
    ];
    for name in near_misses {
        // As a directory:
        let dir_path = wit.join(name);
        fs::create_dir_all(&dir_path).unwrap();
        let err = format!("{:#}", rust::plan(&wit).unwrap_err());
        assert!(err.contains(name), "dir `{name}`: {err}");
        fs::remove_dir_all(&dir_path).unwrap();

        // As a regular file:
        let file_path = wit.join(name);
        fs::write(&file_path, "").unwrap();
        let err = format!("{:#}", rust::plan(&wit).unwrap_err());
        assert!(err.contains(name), "file `{name}`: {err}");
        fs::remove_file(&file_path).unwrap();
    }

    // Even a syntactically canonical version name that is a regular file is refused.
    let canonical_file = wit.join("v0.3");
    fs::write(&canonical_file, "").unwrap();
    let err = format!("{:#}", rust::plan(&wit).unwrap_err());
    assert!(
        err.contains("v0.3") && err.contains("not a directory"),
        "{err}"
    );
    fs::remove_file(&canonical_file).unwrap();

    // Missing entry in `released` stays an error.
    fs::write(wit.join("released"), "v0.1\nv0.9\n").unwrap();
    let err = format!("{:#}", rust::plan(&wit).unwrap_err());
    assert!(
        err.contains("v0.9") && err.contains("does not exist"),
        "{err}"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn released_manifest_accepts_bom_crlf_and_whitespace_and_wit_bom_is_handled() {
    let root = temp("bom");
    let wit = root.join("wit");

    // UTF-8 BOM + CRLF + trailing spaces + blank line + comment: accepted.
    fs::write(
        wit.join("released"),
        "\u{feff}# released versions\r\n\r\n  v0.1  \r\n",
    )
    .unwrap();
    let manifest = witforge::ReleasedManifest::load(&wit.join("released")).unwrap();
    assert_eq!(manifest.directories(), ["v0.1"]);
    rust::plan(&wit).unwrap();

    // Duplicate entry after BOM is still refused.
    fs::write(wit.join("released"), "\u{feff}v0.1\r\nv0.1\r\n").unwrap();
    let dup_err = format!(
        "{:#}",
        witforge::ReleasedManifest::load(&wit.join("released")).unwrap_err()
    );
    assert!(dup_err.contains("duplicate"), "{dup_err}");

    // Test what `wit-parser` does with a leading UTF-8 BOM in a `.wit` file: either it accepts
    // the file cleanly or our loader names the byte-order mark in the error.
    let bom_dir = root.join("bom-wit");
    fs::create_dir_all(&bom_dir).unwrap();
    fs::write(
        bom_dir.join("contract.wit"),
        "\u{feff}package ex:bom@0.1.0;\ninterface types { record r { id: u32 } }\n",
    )
    .unwrap();
    if let Err(err) = witforge::load(&bom_dir) {
        let msg = format!("{err:#}");
        assert!(msg.contains("byte-order mark"), "{msg}");
    }

    fs::remove_dir_all(&root).unwrap();
}
