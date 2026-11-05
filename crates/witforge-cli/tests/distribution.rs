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

    let bash_path = |path: &Path| path.display().to_string().replace('\\', "/");
    let output = Command::new("bash")
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

    fs::remove_dir_all(&root).unwrap();
}
