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
        .args(["rust", "--no-config", "--project", "demo"])
        .output()
        .unwrap();
    assert_eq!(no_config.status.code(), Some(2), "{no_config:?}");
    assert!(
        String::from_utf8_lossy(&no_config.stderr).contains("requires `--config`"),
        "{no_config:?}"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn rust_serde_is_opt_in_and_cli_overrides_the_config() {
    let root = project("serde");
    let config = write_config(
        &root,
        r#"
version = 1
[project.demo]
root = "wit"
[project.demo.rust]
out = "generated/serde"
serde = true
"#,
    );
    let enabled = witforge()
        .current_dir(&root)
        .args(["rust", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(enabled.status.success(), "{enabled:?}");
    let generated = fs::read_to_string(root.join("generated/serde/v0_1.rs")).unwrap();
    assert!(
        generated.contains("::serde::Serialize"),
        "serde derives missing: {generated}"
    );

    let disabled = witforge()
        .current_dir(&root)
        .args(["rust", "--config"])
        .arg(&config)
        .arg("--no-serde")
        .arg("--out")
        .arg("generated/plain")
        .output()
        .unwrap();
    assert!(disabled.status.success(), "{disabled:?}");
    let generated = fs::read_to_string(root.join("generated/plain/v0_1.rs")).unwrap();
    assert!(
        !generated.contains("::serde::Serialize"),
        "serde was not disabled: {generated}"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn config_refuses_coinciding_nested_or_symlinked_output_roots() {
    let root = project("overlapping-outs");

    let cases = [
        (
            "equal-in-one-project",
            "version = 1\n[project.demo]\nroot = \"wit\"\n[project.demo.rust]\nout = \"generated/shared\"\n[project.demo.proto]\nout = \"generated/shared\"\n",
            &["rust", "--config"][..],
        ),
        (
            "nested-in-one-project",
            "version = 1\n[project.demo]\nroot = \"wit\"\n[project.demo.rust]\nout = \"generated\"\n[project.demo.proto]\nout = \"generated/proto\"\n",
            &["proto", "--config"][..],
        ),
        (
            "shared-across-two-projects",
            "version = 1\n[project.v1]\nroot = \"wit\"\n[project.v1.rust]\nout = \"generated/rust\"\n[project.v2]\nroot = \"wit\"\n[project.v2.rust]\nout = \"generated/rust\"\n",
            &["rust", "--project", "v1", "--config"][..],
        ),
    ];

    for (label, text, args) in cases {
        let config = write_config(&root, text);
        let output = witforge()
            .current_dir(&root)
            .args(args)
            .arg(&config)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{label}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("overlap"),
            "{label}: {output:?}"
        );
    }

    // CLI `--out` override colliding with another configured emitter is also refused.
    let override_cfg = write_config(
        &root,
        "version = 1\n[project.demo]\nroot = \"wit\"\n[project.demo.rust]\nout = \"generated/rust\"\n[project.demo.proto]\nout = \"generated/proto\"\n",
    );
    let override_err = witforge()
        .current_dir(&root)
        .args(["rust", "--config"])
        .arg(&override_cfg)
        .args(["--out", "generated/proto"])
        .output()
        .unwrap();
    assert_eq!(override_err.status.code(), Some(2), "{override_err:?}");

    #[cfg(unix)]
    {
        fs::create_dir_all(root.join("generated/real")).unwrap();
        std::os::unix::fs::symlink(root.join("generated/real"), root.join("generated/alias"))
            .unwrap();
        let sym_cfg = write_config(
            &root,
            "version = 1\n[project.demo]\nroot = \"wit\"\n[project.demo.rust]\nout = \"generated/real\"\n[project.demo.proto]\nout = \"generated/alias\"\n",
        );
        let sym_err = witforge()
            .current_dir(&root)
            .args(["rust", "--config"])
            .arg(&sym_cfg)
            .output()
            .unwrap();
        assert_eq!(sym_err.status.code(), Some(2), "{sym_err:?}");
        assert!(
            String::from_utf8_lossy(&sym_err.stderr).contains("overlap"),
            "{sym_err:?}"
        );
    }

    assert!(!root.join("generated/shared").exists());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn config_refuses_unexpanded_paths_and_bom_but_accepts_absolute_and_parent_paths() {
    let root = project("unexpanded-paths");

    for (label, value) in [
        ("tilde", "~"),
        ("tilde-slash", "~/gen-out"),
        ("tilde-user", "~alice/gen-out"),
        ("dollar-var", "$HOME/gen-out"),
        ("dollar-braces", "${OUT_DIR}/rust"),
        ("windows-percent", "%USERPROFILE%/gen-out"),
    ] {
        // Test in `out`, `root`, and `released`:
        for (slot, cfg_text) in [
            (
                "rust.out",
                format!(
                    "version = 1\n[project.demo]\nroot = \"wit\"\n[project.demo.rust]\nout = \"{value}\"\n"
                ),
            ),
            (
                "root",
                format!(
                    "version = 1\n[project.demo]\nroot = \"{value}\"\n[project.demo.rust]\nout = \"out\"\n"
                ),
            ),
            (
                "released",
                format!(
                    "version = 1\n[project.demo]\nroot = \"wit\"\nreleased = \"{value}\"\n[project.demo.rust]\nout = \"out\"\n"
                ),
            ),
        ] {
            let config = write_config(&root, &cfg_text);
            let output = witforge()
                .current_dir(&root)
                .args(["rust", "--config"])
                .arg(&config)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(2),
                "{label} in {slot}: {output:?}"
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains("not shell-expanded"),
                "{label} in {slot}: {stderr}"
            );
            assert!(
                !root.join("~").exists(),
                "must never create literal `~` directory"
            );
        }
    }

    // A UTF-8 BOM in `witforge.toml` is refused with a message naming the byte-order mark.
    let bom_cfg = write_config(
        &root,
        "\u{feff}version = 1\n[project.demo]\nroot = \"wit\"\n[project.demo.rust]\nout = \"out\"\n",
    );
    let bom_out = witforge()
        .current_dir(&root)
        .args(["rust", "--config"])
        .arg(&bom_cfg)
        .output()
        .unwrap();
    assert_eq!(bom_out.status.code(), Some(2), "{bom_out:?}");
    let bom_stderr = String::from_utf8_lossy(&bom_out.stderr);
    assert!(bom_stderr.contains("byte-order mark"), "{bom_stderr}");

    // Absolute paths and `..` paths remain accepted in `witforge.toml`, and CLI paths are not
    // subject to the config shell-expansion rejection.
    let sub = root.join("nested");
    fs::create_dir_all(&sub).unwrap();
    let abs_out = root.join("abs-out");
    let ok_cfg = sub.join("witforge.toml");
    let abs_out_toml = abs_out.display().to_string().replace('\\', "\\\\");
    fs::write(
        &ok_cfg,
        format!(
            "version = 1\n[project.demo]\nroot = \"../wit\"\n[project.demo.rust]\nout = \"{abs_out_toml}\"\n"
        ),
    )
    .unwrap();
    let ok = witforge()
        .current_dir(&root)
        .args(["rust", "--config"])
        .arg(&ok_cfg)
        .output()
        .unwrap();
    assert!(ok.status.success(), "{ok:?}");
    assert!(abs_out.join("mod.rs").is_file());

    // CLI `--out` with a literal `$` character stays accepted (shell already ran).
    let dollar_cli_out = root.join("cli-$out");
    let cli_ok = witforge()
        .current_dir(&root)
        .args(["rust", "wit", "--out"])
        .arg(&dollar_cli_out)
        .output()
        .unwrap();
    assert!(cli_ok.status.success(), "{cli_ok:?}");
    assert!(dollar_cli_out.join("mod.rs").is_file());

    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn config_discovery_stops_at_git_respects_no_config_and_ignores_stray_config_when_cli_complete() {
    let base =
        std::env::temp_dir().join(format!("witforge-config-discover-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let outer = base.join("outer");
    let repo = outer.join("repo");
    let nested = repo.join("crates/sub/deep");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir_all(repo.join(".git")).unwrap();
    copy_tree(&source_tree(), &repo.join("wit"));

    // 1. Upward discovery from a nested subdir finds repo/witforge.toml.
    write_config(
        &repo,
        r#"
version = 1
[project.demo]
root = "wit"
[project.demo.rust]
out = "gen/rust"
[project.demo.proto]
out = "gen/proto"
[project.demo.schema]
out = "gen/schema"
[project.demo.glue]
out = "gen/glue"
plain-root = "crate::plain"
proto-root = "crate::proto"
"#,
    );
    let generated = witforge()
        .current_dir(&nested)
        .args(["generate"])
        .output()
        .unwrap();
    assert!(generated.status.success(), "{generated:?}");
    assert!(repo.join("gen/rust/mod.rs").is_file());
    assert!(repo.join("gen/proto/v0.proto").is_file());
    assert!(repo.join("gen/schema/v0_2.schema.json").is_file());
    assert!(repo.join("gen/glue/mod.rs").is_file());

    let checked = witforge()
        .current_dir(&nested)
        .args(["generate", "--check"])
        .output()
        .unwrap();
    assert!(checked.status.success(), "{checked:?}");

    // 2. `--no-config` disables discovery cleanly.
    let disabled = witforge()
        .current_dir(&nested)
        .args(["generate", "--no-config"])
        .output()
        .unwrap();
    assert_eq!(disabled.status.code(), Some(2), "{disabled:?}");

    // 3. Discovery stops at `.git` and does NOT escape into `outer/witforge.toml`.
    fs::remove_file(repo.join("witforge.toml")).unwrap();
    write_config(
        &outer,
        "version = 1\n[project.leaked]\nroot = \"wit\"\n[project.leaked.rust]\nout = \"out\"\n",
    );
    let stopped = witforge()
        .current_dir(&nested)
        .args(["generate"])
        .output()
        .unwrap();
    assert_eq!(stopped.status.code(), Some(2), "{stopped:?}");

    // 4. A malformed or multi-project `witforge.toml` above cwd MUST NOT break explicit CLI calls.
    fs::write(repo.join("witforge.toml"), "version = [malformed toml\n").unwrap();
    let explicit_out = repo.join("explicit-rust");
    let explicit = witforge()
        .current_dir(&nested)
        .args(["rust"])
        .arg(repo.join("wit"))
        .args(["--out"])
        .arg(&explicit_out)
        .output()
        .unwrap();
    assert!(explicit.status.success(), "{explicit:?}");
    assert!(explicit_out.join("mod.rs").is_file());

    // 5. `generate --check` reports EVERY failing generator without stopping at the first.
    write_config(
        &repo,
        r#"
version = 1
[project.demo]
root = "wit"
[project.demo.rust]
out = "gen/rust"
[project.demo.proto]
out = "gen/proto"
"#,
    );
    let mut rust_mod = fs::read_to_string(repo.join("gen/rust/mod.rs")).unwrap();
    rust_mod.push_str("// drift\n");
    fs::write(repo.join("gen/rust/mod.rs"), rust_mod).unwrap();
    let mut proto_v0 = fs::read_to_string(repo.join("gen/proto/v0.proto")).unwrap();
    proto_v0.push_str("// drift\n");
    fs::write(repo.join("gen/proto/v0.proto"), proto_v0).unwrap();
    let both_fail = witforge()
        .current_dir(&repo)
        .args(["generate", "--check", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(both_fail.status.code(), Some(1), "{both_fail:?}");
    let json: serde_json::Value = serde_json::from_slice(&both_fail.stdout).unwrap();
    let diags = json["diagnostics"].as_array().unwrap();
    assert_eq!(
        diags.len(),
        2,
        "generate --check must report drift from BOTH rust and proto: {diags:?}"
    );
    assert!(diags.iter().all(|d| d["code"] == "WF2001"));

    // 6. `generate` validates every selected project up front before writing any files.
    let atomic_out = repo.join("atomic-rust");
    write_config(
        &repo,
        r#"
version = 1
[project.a]
root = "wit"
[project.a.rust]
out = "atomic-rust"
[project.b]
root = "wit"
"#,
    );
    let partial = witforge()
        .current_dir(&repo)
        .args(["generate", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(partial.status.code(), Some(2), "{partial:?}");
    let partial_json: serde_json::Value = serde_json::from_slice(&partial.stdout).unwrap();
    assert_eq!(partial_json["diagnostics"][0]["code"], "WF9001");
    assert!(
        !atomic_out.exists(),
        "generate must not partially write project `a` when project `b` has no generators"
    );

    // 7. Positional WIT_ROOT cannot be combined with `--config` or `--project` on `freeze` or
    // `new-version`.
    for args in [
        vec![
            "freeze",
            "wit",
            "--config",
            "witforge.toml",
            "--format",
            "json",
        ],
        vec!["new-version", "wit", "--project", "a", "--format", "json"],
    ] {
        let out = witforge().current_dir(&repo).args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        let val: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(val["diagnostics"][0]["code"], "WF9001", "{args:?}: {val}");
    }

    fs::remove_dir_all(&base).unwrap();
}
