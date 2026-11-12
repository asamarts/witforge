use std::fs;
use std::path::Path;
use std::process::Command;

pub fn run_cargo_program(root: &Path, dependencies: &str, label: &str) {
    let manifest = format!(
        "[package]\nname = \"witforge-generated-test\"\nversion = \"0.0.0\"\nedition = \"2024\"\nrust-version = \"1.88\"\npublish = false\n\n[[bin]]\nname = \"witforge-generated-test\"\npath = \"main.rs\"\n\n[dependencies]\n{dependencies}\n\n[workspace]\nresolver = \"3\"\n"
    );
    let manifest_path = root.join("Cargo.toml");
    fs::write(&manifest_path, manifest).unwrap();
    let target = root.join("cargo-target");
    let output = Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--offline", "--manifest-path"])
        .arg(&manifest_path)
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_TERM_COLOR", "never")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{label} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
