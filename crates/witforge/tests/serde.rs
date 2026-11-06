//! Opt-in serde derives must compile and round-trip.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::emit::rust;
use witforge::{load, model};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/serde")
}

#[test]
fn serde_is_opt_in_and_roundtrips() {
    let contract = load(&fixture()).unwrap();
    let model = model::build(&contract).unwrap();
    assert_eq!(
        rust::emit_version(&model).unwrap(),
        rust::emit_version_with_serde(&model, false).unwrap()
    );
    let source = rust::emit_version_with_serde(&model, true).unwrap();
    assert!(
        source.contains(
            "#[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]"
        ),
        "{source}"
    );

    let output = std::env::temp_dir().join(format!("witforge-serde-{}", std::process::id()));
    let _ = fs::remove_dir_all(&output);
    fs::create_dir_all(output.join("src")).unwrap();
    fs::write(
        output.join("Cargo.toml"),
        r#"[package]
name = "witforge-serde-check"
version = "0.0.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .unwrap();
    let source = format!(
        "{source}\nfn main() {{\n\
         let item = types::Item {{ id: 7, name: \"row\".to_string(), score: 1.5, note: Some(\"n\".to_string()), tags: vec![\"a\".to_string()], scores: vec![Some(2.5), None] }};\n\
         let json = serde_json::to_string(&item).unwrap();\n\
         let back: types::Item = serde_json::from_str(&json).unwrap();\n\
         assert_eq!(item, back);\n\
         let state = types::State::Ready;\n\
         assert_eq!(serde_json::from_str::<types::State>(&serde_json::to_string(&state).unwrap()).unwrap(), state);\n\
         let event = types::Event::Item(item.clone());\n\
         assert_eq!(serde_json::from_str::<types::Event>(&serde_json::to_string(&event).unwrap()).unwrap(), event);\n\
         let marks = types::Marks {{ trace: true }};\n\
         assert_eq!(serde_json::from_str::<types::Marks>(&serde_json::to_string(&marks).unwrap()).unwrap(), marks);\n\
         let alias: types::AliasItem = item.clone();\n\
         assert_eq!(serde_json::from_str::<types::AliasItem>(&serde_json::to_string(&alias).unwrap()).unwrap(), alias);\n\
         }}\n"
    );
    fs::write(output.join("src/main.rs"), source).unwrap();
    let status = Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--manifest-path"])
        .arg(output.join("Cargo.toml"))
        .status()
        .unwrap();
    assert!(
        status.success(),
        "serde-generated Rust did not compile or round-trip"
    );
    fs::remove_dir_all(&output).unwrap();
}
