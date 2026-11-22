//! Release-asset packaging used by the prebuilt binaries.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn release_asset_script_packages_the_binary_and_writes_a_checksum() {
    let root = std::env::temp_dir().join(format!("witforge-release-asset-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let binary = root.join("witforge");
    fs::write(&binary, b"#!/bin/sh\necho fake witforge\n").unwrap();
    let out = root.join("dist");

    let bash = if cfg!(windows) {
        let git_bash = Path::new(r"C:\Program Files\Git\bin\bash.exe");
        if !git_bash.is_file() {
            return;
        }
        git_bash.to_path_buf()
    } else {
        PathBuf::from("bash")
    };
    let bash_path = |path: &Path| {
        let text = path.display().to_string().replace('\\', "/");
        if cfg!(windows) && text.len() >= 3 && text.as_bytes()[1] == b':' {
            format!("/{}/{}", text[..1].to_ascii_lowercase(), &text[3..])
        } else {
            text
        }
    };
    let output = Command::new(bash)
        .current_dir(repository())
        .arg("ci/release-asset.sh")
        .arg("v0.3.0")
        .arg("x86_64-unknown-linux-gnu")
        .arg(bash_path(&binary))
        .arg(bash_path(&out))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "release asset script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let asset = out.join("witforge-0.3.0-x86_64-unknown-linux-gnu.tar.gz");
    assert!(asset.is_file(), "missing {}", asset.display());
    let checksum_path = out.join("witforge-0.3.0-x86_64-unknown-linux-gnu.tar.gz.sha256");
    let checksum = fs::read_to_string(&checksum_path).unwrap();
    let fields: Vec<_> = checksum.split_whitespace().collect();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].len(), 64);
    assert_eq!(fields[1], "witforge-0.3.0-x86_64-unknown-linux-gnu.tar.gz");

    let listing = Command::new("tar")
        .args(["-tzf"])
        .arg(&asset)
        .output()
        .unwrap();
    assert!(listing.status.success(), "tar -tzf failed");
    let entries: Vec<&str> = std::str::from_utf8(&listing.stdout)
        .unwrap()
        .lines()
        .map(str::trim)
        .collect();
    for required in ["witforge", "LICENSE-APACHE", "LICENSE-MIT", "README.md"] {
        assert!(
            entries.contains(&required),
            "archive missing `{required}`: {entries:?}"
        );
    }

    fs::remove_dir_all(&root).unwrap();
}

fn fenced_blocks(markdown: &str, language: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in markdown.lines() {
        if let Some(info) = line.strip_prefix("```") {
            if let Some(block) = current.take() {
                blocks.push(block);
            } else if info.trim() == language {
                current = Some(String::new());
            }
            continue;
        }
        if let Some(block) = &mut current {
            block.push_str(line);
            block.push('\n');
        }
    }
    blocks
}

fn assert_excerpt(label: &str, path: &Path, snippet: &str) {
    assert!(!snippet.trim().is_empty(), "{label}: empty snippet");
    let source = fs::read_to_string(path).unwrap();
    let source_lines: Vec<&str> = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let mut next = 0;
    for line in snippet
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Some(offset) = source_lines[next..]
            .iter()
            .position(|candidate| *candidate == line)
        else {
            panic!("{label}: `{line}` is not in {}", path.display());
        };
        next += offset + 1;
    }
}

fn run(command: &mut Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn generated_output_guide_snippets_are_current() {
    let repository = repository();
    let guide = fs::read_to_string(repository.join("docs/guide/generated-output.md")).unwrap();

    let rust = fenced_blocks(&guide, "rust");
    assert_eq!(rust.len(), 4, "unexpected Rust snippet count");
    assert_excerpt(
        "plain Rust",
        &repository.join("examples/single/generated/rust/v0_1.rs"),
        &rust[0],
    );
    assert_excerpt(
        "glue conversion",
        &repository.join("examples/single/generated/glue/v0.rs"),
        &rust[1],
    );
    assert_excerpt(
        "glue function helper",
        &repository.join("examples/single/generated/glue/v0.rs"),
        &rust[2],
    );
    assert_excerpt(
        "version adapter",
        &repository.join("examples/evolved/generated/rust/convert.rs"),
        &rust[3],
    );

    let proto = fenced_blocks(&guide, "proto");
    assert_eq!(proto.len(), 1, "unexpected protobuf snippet count");
    assert_excerpt(
        "protobuf",
        &repository.join("examples/single/generated/proto/v0.proto"),
        &proto[0],
    );

    let json = fenced_blocks(&guide, "json");
    assert_eq!(json.len(), 1, "unexpected JSON snippet count");
    assert_excerpt(
        "JSON Schema",
        &repository.join("examples/single/generated/schema/v0_1.schema.json"),
        &json[0],
    );
}

#[test]
fn readme_output_snippets_are_current() {
    let repository = repository();
    let readme = fs::read_to_string(repository.join("README.md")).unwrap();
    let wit = fenced_blocks(&readme, "wit");
    assert_eq!(wit.len(), 1, "unexpected README WIT snippet count");

    let base =
        std::env::temp_dir().join(format!("witforge-readme-snippets-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let wit_root = base.join("wit");
    let out = base.join("generated");
    let version = wit_root.join("v0.1");
    fs::create_dir_all(&version).unwrap();
    fs::write(version.join("contract.wit"), &wit[0]).unwrap();

    let binary = env!("CARGO_BIN_EXE_witforge");
    run(Command::new(binary)
        .arg("lock")
        .arg(&version)
        .arg("--first"));
    run(Command::new(binary)
        .arg("rust")
        .arg(&wit_root)
        .arg("--out")
        .arg(out.join("rust")));
    run(Command::new(binary)
        .arg("proto")
        .arg(&wit_root)
        .arg("--out")
        .arg(out.join("proto")));

    let rust = fenced_blocks(&readme, "rust");
    assert_eq!(rust.len(), 1, "unexpected README Rust snippet count");
    assert_excerpt("README Rust", &out.join("rust/v0_1.rs"), &rust[0]);

    let proto = fenced_blocks(&readme, "proto");
    assert_eq!(proto.len(), 1, "unexpected README protobuf snippet count");
    assert_excerpt("README protobuf", &out.join("proto/v0.proto"), &proto[0]);

    fs::remove_dir_all(&base).unwrap();
}
