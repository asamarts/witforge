//! Configuration-file tests for generator roots and outputs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn witforge() -> Command {
    Command::new(env!("CARGO_BIN_EXE_witforge"))
}

fn source_tree() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../witforge/tests/fixtures/versions/wit")
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

fn project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("witforge-config-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    copy_tree(&source_tree(), &root.join("wit"));
    root
}

fn write_config(root: &Path, text: &str) -> PathBuf {
    let path = root.join("witforge.toml");
    fs::write(&path, text).unwrap();
    path
}

#[test]
fn a_single_project_supplies_root_and_all_emitters() {
    let root = project("single");
    let config = write_config(
        &root,
        r#"
version = 1

[project.demo]
root = "wit"

[project.demo.rust]
out = "generated/rust"

[project.demo.proto]
out = "generated/proto"

[project.demo.schema]
out = "generated/schema"

[project.demo.glue]
out = "generated/glue"
plain-root = "crate::plain"
proto-root = "crate::proto"
"#,
    );

    for (command, expected) in [
        ("rust", "generated/rust/mod.rs"),
        ("proto", "generated/proto/v0.proto"),
        ("schema", "generated/schema/v0_1.schema.json"),
        ("glue", "generated/glue/mod.rs"),
    ] {
        let output = witforge()
            .current_dir(&root)
            .args([command, "--config"])
            .arg(&config)
            .output()
            .unwrap();
        assert!(output.status.success(), "{command}: {output:?}");
        assert!(root.join(expected).is_file(), "{command}: {expected}");
    }
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn command_line_paths_override_the_config() {
    let root = project("override");
    let config = write_config(
        &root,
        r#"
version = 1
[project.demo]
root = "wit"
[project.demo.rust]
out = "from-config"
"#,
    );
    let output = witforge()
        .current_dir(&root)
        .args(["rust", "--config"])
        .arg(&config)
        .arg("--out")
        .arg("from-command")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(root.join("from-command/mod.rs").is_file());
    assert!(!root.join("from-config/mod.rs").exists());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn multiple_projects_require_project_selection() {
    let root = project("multiple");
    let config = write_config(
        &root,
        r#"
version = 1
[project.one]
root = "wit"
[project.one.rust]
out = "out/one"
[project.two]
root = "wit"
[project.two.rust]
out = "out/two"
"#,
    );
    let ambiguous = witforge()
        .current_dir(&root)
        .args(["rust", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert_eq!(ambiguous.status.code(), Some(2), "{ambiguous:?}");
    assert!(
        String::from_utf8_lossy(&ambiguous.stderr).contains("multiple projects"),
        "{ambiguous:?}"
    );

    let selected = witforge()
        .current_dir(&root)
        .args(["rust", "--config"])
        .arg(&config)
        .args(["--project", "two"])
        .output()
        .unwrap();
    assert!(selected.status.success(), "{selected:?}");
    assert!(root.join("out/two/mod.rs").is_file());
    assert!(!root.join("out/one").exists());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn unknown_configuration_keys_and_bad_selections_are_errors() {
    let root = project("unknown");
    let config = write_config(
        &root,
        r#"
version = 1
[project.demo]
root = "wit"
[project.demo.rust]
out = "out"
typo = true
"#,
    );
    let unknown = witforge()
        .current_dir(&root)
        .args(["rust", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(2), "{unknown:?}");
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("unknown field"),
        "{unknown:?}"
    );

    let valid = write_config(
        &root,
        r#"
version = 1
[project.demo]
root = "wit"
[project.demo.rust]
out = "out"
"#,
    );
    let missing = witforge()
        .current_dir(&root)
        .args(["rust", "--config"])
        .arg(&valid)
        .args(["--project", "missing"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("no project"),
        "{missing:?}"
    );

    let no_config = witforge()
        .current_dir(&root)
        .args(["rust"])
        .args(["--project", "demo"])
        .output()
        .unwrap();
    assert_eq!(no_config.status.code(), Some(2), "{no_config:?}");
    assert!(
        String::from_utf8_lossy(&no_config.stderr).contains("requires `--config`"),
        "{no_config:?}"
    );
    fs::remove_dir_all(&root).unwrap();
}
