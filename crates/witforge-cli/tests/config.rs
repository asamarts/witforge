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

#[test]
#[allow(clippy::too_many_lines)]
fn freeze_honors_custom_released_manifest_and_retirement_order() {
    let base = std::env::temp_dir().join(format!(
        "witforge-config-freeze-custom-released-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&base);
    let wit_root = base.join("wit");
    fs::create_dir_all(wit_root.join("v0.1")).unwrap();
    fs::write(
        wit_root.join("v0.1/contract.wit"),
        "package ex:frz@0.1.0;\ninterface i { record r { a: u32 } }\n",
    )
    .unwrap();
    write_config(
        &base,
        "version = 1\n[project.demo]\nroot = \"wit\"\nreleased = \"wit/released-prod\"\n",
    );

    assert!(
        witforge()
            .args(["lock"])
            .arg(wit_root.join("v0.1"))
            .arg("--first")
            .status()
            .unwrap()
            .success()
    );
    assert!(
        witforge()
            .args(["release"])
            .arg(wit_root.join("v0.1"))
            .args(["--config"])
            .arg(base.join("witforge.toml"))
            .status()
            .unwrap()
            .success()
    );
    assert!(
        witforge()
            .args(["new-version"])
            .arg(&wit_root)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        witforge()
            .args(["lock"])
            .arg(wit_root.join("v0.2"))
            .args(["--against"])
            .arg(wit_root.join("v0.1/wire.lock"))
            .status()
            .unwrap()
            .success()
    );
    assert!(
        witforge()
            .args(["release"])
            .arg(wit_root.join("v0.2"))
            .args(["--config"])
            .arg(base.join("witforge.toml"))
            .status()
            .unwrap()
            .success()
    );

    let run_git = |args: &[&str]| {
        let out = Command::new("git")
            .current_dir(&base)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?} failed: {out:?}");
    };
    run_git(&["init", "-q"]);
    run_git(&["config", "user.name", "test"]);
    run_git(&["config", "user.email", "test@example.invalid"]);
    run_git(&["add", "."]);
    run_git(&["commit", "-q", "-m", "init"]);

    // 1. Untouched custom manifest passes through both `--config` and explicit `--released`.
    let clean = witforge()
        .current_dir(&base)
        .args(["freeze", "--config", "witforge.toml"])
        .output()
        .unwrap();
    assert!(clean.status.success(), "{clean:?}");
    let clean_stdout = String::from_utf8_lossy(&clean.stdout);
    assert!(
        clean_stdout.contains("2 release check(s)"),
        "expected custom manifest checks, got: {clean_stdout}"
    );

    // 2. Missing explicit `--released` exits 2 (operational error).
    let missing = witforge()
        .current_dir(&base)
        .args(["freeze", "wit", "--released", "wit/no-such-manifest"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");

    // 3. Non-prefix retirement (`v0.2 retired` while `v0.1` is live) and retiring all versions fail.
    fs::write(wit_root.join("released-prod"), "v0.1\nv0.2 retired\n").unwrap();
    let non_prefix = witforge()
        .current_dir(&base)
        .args(["freeze", "--config", "witforge.toml"])
        .output()
        .unwrap();
    assert_eq!(non_prefix.status.code(), Some(1), "{non_prefix:?}");
    assert!(
        String::from_utf8_lossy(&non_prefix.stderr).contains("cannot retire `v0.2`"),
        "{non_prefix:?}"
    );

    fs::write(
        wit_root.join("released-prod"),
        "v0.1 retired\nv0.2 retired\n",
    )
    .unwrap();
    let all_retired = witforge()
        .current_dir(&base)
        .args(["freeze", "wit", "--released", "wit/released-prod"])
        .output()
        .unwrap();
    assert_eq!(all_retired.status.code(), Some(1), "{all_retired:?}");
    assert!(
        String::from_utf8_lossy(&all_retired.stderr).contains("every version directory"),
        "{all_retired:?}"
    );

    // Valid oldest-prefix retirement (`v0.1 retired`, `v0.2` live) passes.
    fs::write(wit_root.join("released-prod"), "v0.1 retired\nv0.2\n").unwrap();
    let valid_retire = witforge()
        .current_dir(&base)
        .args(["freeze", "--config", "witforge.toml"])
        .output()
        .unwrap();
    assert!(valid_retire.status.success(), "{valid_retire:?}");

    // 4. Editing a released directory tracked via `project.released` is caught after commit!
    fs::write(
        wit_root.join("v0.1/contract.wit"),
        "package ex:frz@0.1.0;\n// tampered\ninterface i { record r { a: u32 } }\n",
    )
    .unwrap();
    run_git(&["add", "wit/v0.1/contract.wit"]);
    run_git(&["commit", "-q", "-m", "tamper"]);

    let tampered = witforge()
        .current_dir(&base)
        .args(["freeze", "--config", "witforge.toml"])
        .output()
        .unwrap();
    assert_eq!(tampered.status.code(), Some(1), "{tampered:?}");
    assert!(
        String::from_utf8_lossy(&tampered.stderr).contains("changed since its release"),
        "{tampered:?}"
    );

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn schema_draft_option_works_via_cli_and_config_and_rejects_invalid_values() {
    let root = project("schema-draft");
    let config = write_config(
        &root,
        r#"
version = 1
[project.demo]
root = "wit"
[project.demo.schema]
out = "generated/schema-2020"
draft = "2020-12"
"#,
    );

    // 1. `witforge generate` honors `schema.draft = "2020-12"`.
    let generated = witforge()
        .current_dir(&root)
        .args(["generate", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(generated.status.success(), "{generated:?}");
    let doc: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("generated/schema-2020/v0_1.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        doc["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert!(doc["$defs"].is_object());
    assert!(doc.get("definitions").is_none());

    // 2. CLI `--draft 7` overrides config `draft = "2020-12"`.
    let overridden = witforge()
        .current_dir(&root)
        .args(["schema", "--config"])
        .arg(&config)
        .args(["--draft", "7", "--out", "generated/schema-7"])
        .output()
        .unwrap();
    assert!(overridden.status.success(), "{overridden:?}");
    let doc7: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("generated/schema-7/v0_1.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(doc7["$schema"], "http://json-schema.org/draft-07/schema#");
    assert!(doc7["definitions"].is_object());

    // 3. `witforge generate --draft <DRAFT>` overrides configured `schema.draft` and rejects
    //    invalid draft values upfront (`WF9001`, exit 2).
    let gen_override = witforge()
        .current_dir(&root)
        .args(["generate", "--config"])
        .arg(&config)
        .args(["--draft", "7"])
        .output()
        .unwrap();
    assert!(gen_override.status.success(), "{gen_override:?}");
    let gen_doc7: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("generated/schema-2020/v0_1.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        gen_doc7["$schema"],
        "http://json-schema.org/draft-07/schema#"
    );

    let bad_gen_draft = witforge()
        .current_dir(&root)
        .args(["generate", "--config"])
        .arg(&config)
        .args(["--draft", "2019-09", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(bad_gen_draft.status.code(), Some(2), "{bad_gen_draft:?}");
    let bad_gen_json: serde_json::Value = serde_json::from_slice(&bad_gen_draft.stdout).unwrap();
    assert_eq!(bad_gen_json["diagnostics"][0]["code"], "WF9001");

    // 4. Invalid CLI `--draft` and invalid config `draft` exit 2 (WF9001).
    let bad_cli = witforge()
        .current_dir(&root)
        .args([
            "schema",
            "wit",
            "--out",
            "generated/bad",
            "--draft",
            "2019-09",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(bad_cli.status.code(), Some(2), "{bad_cli:?}");
    let bad_cli_json: serde_json::Value = serde_json::from_slice(&bad_cli.stdout).unwrap();
    assert_eq!(bad_cli_json["diagnostics"][0]["code"], "WF9001");

    let bad_config = write_config(
        &root,
        r#"
version = 1
[project.demo]
root = "wit"
[project.demo.schema]
out = "generated/bad"
draft = "draft-07"
"#,
    );
    let bad_cfg_out = witforge()
        .current_dir(&root)
        .args(["generate", "--config"])
        .arg(&bad_config)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert_eq!(bad_cfg_out.status.code(), Some(2), "{bad_cfg_out:?}");
    let bad_cfg_json: serde_json::Value = serde_json::from_slice(&bad_cfg_out.stdout).unwrap();
    assert_eq!(bad_cfg_json["diagnostics"][0]["code"], "WF9001");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn no_std_option_works_via_cli_and_config_and_overrides_cleanly() {
    let root = project("no-std");
    let config = write_config(
        &root,
        r#"
version = 1
[project.demo]
root = "wit"
[project.demo.rust]
out = "generated/rust"
no-std = true
[project.demo.glue]
out = "generated/glue"
plain-root = "crate::plain"
proto-root = "crate::proto"
no-std = true
"#,
    );

    // 1. `witforge generate` honors `no-std = true` for both rust (including convert.rs) and glue.
    let gen_out = witforge()
        .current_dir(&root)
        .args(["generate", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(gen_out.status.success(), "{gen_out:?}");
    for rel in [
        "generated/rust/v0_1.rs",
        "generated/rust/v0_2.rs",
        "generated/rust/convert.rs",
        "generated/glue/mod.rs",
        "generated/glue/v0.rs",
    ] {
        let text = fs::read_to_string(root.join(rel)).unwrap();
        assert!(!text.contains("::std::"), "{rel} leaked ::std:::\n{text}");
    }
    let rust_v01 = fs::read_to_string(root.join("generated/rust/v0_1.rs")).unwrap();
    assert!(rust_v01.contains("::alloc::string::String"), "{rust_v01}");
    let convert_rs = fs::read_to_string(root.join("generated/rust/convert.rs")).unwrap();
    assert!(
        convert_rs.contains("impl ::core::error::Error for ConversionError"),
        "{convert_rs}"
    );
    let glue_mod = fs::read_to_string(root.join("generated/glue/mod.rs")).unwrap();
    assert!(
        glue_mod.contains("impl ::core::error::Error for GlueError"),
        "{glue_mod}"
    );

    // 2. `--std` overrides config `no-std = true` on both `rust` and `glue`.
    let std_rust = witforge()
        .current_dir(&root)
        .args(["rust", "--config"])
        .arg(&config)
        .args(["--std", "--out", "generated/rust-std"])
        .output()
        .unwrap();
    assert!(std_rust.status.success(), "{std_rust:?}");
    let std_v01 = fs::read_to_string(root.join("generated/rust-std/v0_1.rs")).unwrap();
    assert!(std_v01.contains("::std::string::String"), "{std_v01}");
    assert!(!std_v01.contains("::alloc::"), "{std_v01}");

    let std_glue = witforge()
        .current_dir(&root)
        .args(["glue", "--config"])
        .arg(&config)
        .args(["--std", "--out", "generated/glue-std"])
        .output()
        .unwrap();
    assert!(std_glue.status.success(), "{std_glue:?}");
    let std_glue_mod = fs::read_to_string(root.join("generated/glue-std/mod.rs")).unwrap();
    assert!(
        std_glue_mod.contains("impl ::std::error::Error for GlueError"),
        "{std_glue_mod}"
    );

    // 3. `--no-std` works directly on CLI without config.
    let cli_nostd = witforge()
        .current_dir(&root)
        .args(["rust", "wit", "--out", "generated/cli-nostd", "--no-std"])
        .output()
        .unwrap();
    assert!(cli_nostd.status.success(), "{cli_nostd:?}");
    let cli_v01 = fs::read_to_string(root.join("generated/cli-nostd/v0_1.rs")).unwrap();
    assert!(cli_v01.contains("::alloc::string::String"), "{cli_v01}");

    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn freeze_audit_regressions() {
    let base = std::env::temp_dir().join(format!("witforge-freeze-audit-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();

    assert!(
        witforge()
            .args(["init"])
            .arg(&base)
            .args(["--package", "acme:freezeaudit"])
            .status()
            .unwrap()
            .success()
    );
    let wit_root = base.join("wit");
    let manifest_dir = base.join("manifests");
    fs::create_dir_all(&manifest_dir).unwrap();
    let custom_manifest = manifest_dir.join("released");

    assert!(
        witforge()
            .args(["new-version"])
            .arg(&wit_root)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        witforge()
            .args(["lock"])
            .arg(wit_root.join("v0.2"))
            .args(["--against"])
            .arg(wit_root.join("v0.1/wire.lock"))
            .status()
            .unwrap()
            .success()
    );
    fs::write(&custom_manifest, "v0.1 retired\nv0.2\n").unwrap();

    let run_git = |args: &[&str]| {
        let out = Command::new("git")
            .current_dir(&base)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?} failed: {out:?}");
    };
    run_git(&["init", "-q"]);
    run_git(&["config", "user.name", "test"]);
    run_git(&["config", "user.email", "test@example.invalid"]);
    run_git(&["add", "."]);
    run_git(&["commit", "-q", "-m", "init"]);

    // 1. Staged-only index tampering (working tree restored back to HEAD) must be caught.
    let original_v01 = fs::read_to_string(wit_root.join("v0.1/contract.wit")).unwrap();
    fs::write(
        wit_root.join("v0.1/contract.wit"),
        format!("{original_v01}// staged tamper\n"),
    )
    .unwrap();
    run_git(&["add", "wit/v0.1/contract.wit"]);
    fs::write(wit_root.join("v0.1/contract.wit"), &original_v01).unwrap();
    let staged_out = witforge()
        .current_dir(&base)
        .args(["freeze", "wit", "--released", "manifests/released"])
        .output()
        .unwrap();
    assert_eq!(staged_out.status.code(), Some(1), "{staged_out:?}");
    assert!(
        String::from_utf8_lossy(&staged_out.stderr).contains("v0.1/contract.wit"),
        "{staged_out:?}"
    );
    run_git(&["reset", "-q", "HEAD", "--", "wit/v0.1/contract.wit"]);

    // 2. Un-retiring a previously retired entry reports a line position in JSON diagnostics.
    fs::write(&custom_manifest, "v0.1\nv0.2\n").unwrap();
    let unretire_out = witforge()
        .current_dir(&base)
        .args([
            "freeze",
            "wit",
            "--released",
            "manifests/released",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(unretire_out.status.code(), Some(1), "{unretire_out:?}");
    let unretire_json: serde_json::Value = serde_json::from_slice(&unretire_out.stdout).unwrap();
    assert_eq!(unretire_json["diagnostics"][0]["code"], "WF1109");
    assert_eq!(
        unretire_json["diagnostics"][0]["line"], 1,
        "{unretire_json}"
    );

    // 3. A malformed version entry (`v00.3`) does NOT silence non-prefix retirement violations.
    fs::write(&custom_manifest, "v0.1\nv0.2 retired\n").unwrap();
    fs::create_dir_all(wit_root.join("v00.3")).unwrap();
    let combined = witforge()
        .current_dir(&base)
        .args(["freeze", "wit", "--released", "manifests/released"])
        .output()
        .unwrap();
    assert_eq!(combined.status.code(), Some(1), "{combined:?}");
    let combined_stderr = String::from_utf8_lossy(&combined.stderr);
    assert!(combined_stderr.contains("v00.3"), "{combined_stderr}");
    assert!(
        combined_stderr.contains("cannot retire `v0.2`"),
        "{combined_stderr}"
    );
    fs::remove_dir_all(wit_root.join("v00.3")).unwrap();

    // 4. Deleting the explicit manifest's entire parent directory reports `WF1109` (exit 1),
    //    not `IO_ERROR` (exit 2).
    run_git(&["checkout", "-q", "--", "manifests/released"]);
    fs::remove_dir_all(&manifest_dir).unwrap();
    let deleted_parent = witforge()
        .current_dir(&base)
        .args([
            "freeze",
            "wit",
            "--released",
            "manifests/released",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(deleted_parent.status.code(), Some(1), "{deleted_parent:?}");
    let deleted_json: serde_json::Value = serde_json::from_slice(&deleted_parent.stdout).unwrap();
    assert_eq!(
        deleted_json["diagnostics"][0]["code"], "WF1109",
        "{deleted_json}"
    );

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn project_policy_diff_check_and_forbid_unstable_in_release_work_end_to_end() {
    let base = std::env::temp_dir().join(format!("witforge-policy-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();

    // Create a two-project repo with distinct `[project.<name>.policy]` rules.
    let alpha_v01 = base.join("alpha/v0.1");
    let alpha_v02 = base.join("alpha/v0.2");
    fs::create_dir_all(&alpha_v01).unwrap();
    fs::create_dir_all(&alpha_v02).unwrap();
    fs::write(
        alpha_v01.join("contract.wit"),
        "package acme:alpha@0.1.0;\n\
         interface api {\n\
           record item { id: u32 }\n\
           ping: func();\n\
         }\n",
    )
    .unwrap();
    fs::write(
        alpha_v02.join("contract.wit"),
        "package acme:alpha@0.2.0;\n\
         interface api {\n\
           record item { id: u32, note: option<string> }\n\
           ping: func();\n\
         }\n",
    )
    .unwrap();
    assert!(
        witforge()
            .args(["lock"])
            .arg(&alpha_v01)
            .arg("--first")
            .status()
            .unwrap()
            .success()
    );
    fs::copy(alpha_v01.join("wire.lock"), alpha_v02.join("wire.lock")).unwrap();
    assert!(
        witforge()
            .args(["lock"])
            .arg(&alpha_v02)
            .args(["--against"])
            .arg(alpha_v01.join("wire.lock"))
            .status()
            .unwrap()
            .success()
    );

    let beta_v01 = base.join("beta/v0.1");
    let beta_v02 = base.join("beta/v0.2");
    fs::create_dir_all(&beta_v01).unwrap();
    fs::create_dir_all(&beta_v02).unwrap();
    fs::write(
        beta_v01.join("contract.wit"),
        "package acme:beta@0.1.0;\n\
         @since(version = 0.1.0)\n\
         interface api {\n\
           @since(version = 0.1.0)\n\
           record item { id: u32 }\n\
         }\n",
    )
    .unwrap();
    fs::write(
        beta_v02.join("contract.wit"),
        "package acme:beta@0.2.0;\n\
         @since(version = 0.1.0)\n\
         interface api {\n\
           @since(version = 0.1.0)\n\
           record item { id: u32 }\n\
           @unstable(feature = draft-search)\n\
           search: func(query: string) -> list<item>;\n\
         }\n",
    )
    .unwrap();
    assert!(
        witforge()
            .args(["lock"])
            .arg(&beta_v01)
            .arg("--first")
            .status()
            .unwrap()
            .success()
    );
    fs::copy(beta_v01.join("wire.lock"), beta_v02.join("wire.lock")).unwrap();
    assert!(
        witforge()
            .args(["lock"])
            .arg(&beta_v02)
            .args(["--against"])
            .arg(beta_v01.join("wire.lock"))
            .status()
            .unwrap()
            .success()
    );

    let config = write_config(
        &base,
        r#"
version = 1

[project.alpha]
root = "alpha"

[project.alpha.policy]
diff-check = ["wire", "decode"]

[project.beta]
root = "beta"

[project.beta.policy]
diff-check = ["decode"]
forbid-unstable-in-release = true
"#,
    );

    // 1. Bare `witforge diff alpha/v0.1 alpha/v0.2 --check` discovers `[project.alpha.policy]`
    // (`wire, decode`) via root-matching -> PASSES (optional field addition is wire+decode
    // compatible, even though it breaks source/api)!
    let alpha_discovered = witforge()
        .current_dir(&base)
        .args(["diff", "alpha/v0.1", "alpha/v0.2", "--check"])
        .output()
        .unwrap();
    assert!(
        alpha_discovered.status.success(),
        "policy.diff-check=[wire,decode] must pass optional addition: {alpha_discovered:?}"
    );
    assert!(
        String::from_utf8_lossy(&alpha_discovered.stdout).contains("result: COMPATIBLE"),
        "{alpha_discovered:?}"
    );

    // `--no-config` ignores discovered policy and falls back to default `wire,source,api` -> FAILS!
    let alpha_no_cfg = witforge()
        .current_dir(&base)
        .args(["diff", "alpha/v0.1", "alpha/v0.2", "--check", "--no-config"])
        .output()
        .unwrap();
    assert_eq!(alpha_no_cfg.status.code(), Some(1), "{alpha_no_cfg:?}");

    // Explicit `--check=api` overrides `policy.diff-check` -> FAILS!
    let alpha_override = witforge()
        .current_dir(&base)
        .args([
            "diff",
            "alpha/v0.1",
            "alpha/v0.2",
            "--check=api",
            "--config",
            config.to_str().unwrap(),
            "--project",
            "alpha",
        ])
        .output()
        .unwrap();
    assert_eq!(alpha_override.status.code(), Some(1), "{alpha_override:?}");

    // Plain `witforge diff` without `--check` always exits 0.
    let alpha_plain = witforge()
        .current_dir(&base)
        .args([
            "diff",
            "alpha/v0.1",
            "alpha/v0.2",
            "--config",
            config.to_str().unwrap(),
            "--project",
            "alpha",
        ])
        .output()
        .unwrap();
    assert!(alpha_plain.status.success(), "{alpha_plain:?}");

    // Beta adds a new function (`search`), which is decode-breaking (`old<-new`):
    // bare `--check` uses `beta`'s `policy.diff-check = ["decode"]` -> FAILS!
    let beta_check = witforge()
        .current_dir(&base)
        .args(["diff", "beta/v0.1", "beta/v0.2", "--check"])
        .output()
        .unwrap();
    assert_eq!(beta_check.status.code(), Some(1), "{beta_check:?}");
    assert!(
        String::from_utf8_lossy(&beta_check.stderr).contains("gated view(s) (decode)"),
        "{beta_check:?}"
    );

    // Explicit `--config` on multi-project config without `--project` fails with WF9001 (exit 2).
    let multi_err = witforge()
        .current_dir(&base)
        .args([
            "diff",
            "alpha/v0.1",
            "alpha/v0.2",
            "--check",
            "--config",
            config.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(multi_err.status.code(), Some(2), "{multi_err:?}");

    // 2. `forbid-unstable-in-release = true`:
    // First release beta/v0.1 (no `@unstable` items) -> succeeds!
    let rel_v01 = witforge()
        .current_dir(&base)
        .args(["release", "beta/v0.1"])
        .output()
        .unwrap();
    assert!(rel_v01.status.success(), "{rel_v01:?}");
    assert_eq!(
        fs::read_to_string(base.join("beta/released")).unwrap(),
        "v0.1\n"
    );

    // Releasing beta/v0.2 fails via discovered `[project.beta.policy]` with `WF1014` and leaves
    // `beta/released` untouched!
    let rel_v02_fail = witforge()
        .current_dir(&base)
        .args(["release", "beta/v0.2", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(rel_v02_fail.status.code(), Some(1), "{rel_v02_fail:?}");
    let rel_json: serde_json::Value = serde_json::from_slice(&rel_v02_fail.stdout).unwrap();
    assert_eq!(rel_json["diagnostics"][0]["code"], "WF1014", "{rel_json}");
    assert_eq!(
        rel_json["diagnostics"][0]["path"], "api.search",
        "{rel_json}"
    );
    assert!(
        rel_json["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("draft-search"),
        "{rel_json}"
    );
    assert!(
        rel_json["diagnostics"][0]["line"].as_u64().is_some(),
        "diagnostic should carry source line: {rel_json}"
    );
    assert_eq!(
        fs::read_to_string(base.join("beta/released")).unwrap(),
        "v0.1\n",
        "released manifest must not change when policy refuses release"
    );

    // `--forbid-unstable` on CLI works even with `--no-config`.
    let rel_cli_forbid = witforge()
        .current_dir(&base)
        .args(["release", "beta/v0.2", "--no-config", "--forbid-unstable"])
        .output()
        .unwrap();
    assert_eq!(rel_cli_forbid.status.code(), Some(1), "{rel_cli_forbid:?}");

    // `--allow-unstable` explicitly overrides config `forbid-unstable-in-release = true`.
    let rel_allow = witforge()
        .current_dir(&base)
        .args(["release", "beta/v0.2", "--allow-unstable"])
        .output()
        .unwrap();
    assert!(rel_allow.status.success(), "{rel_allow:?}");
    assert_eq!(
        fs::read_to_string(base.join("beta/released")).unwrap(),
        "v0.1\nv0.2\n"
    );

    // 3. `--git-base` with omitted positional path resolves `project.root` and `policy.diff-check`!
    let run_git = |args: &[&str]| {
        let out = Command::new("git")
            .current_dir(&base)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?} failed: {out:?}");
    };
    run_git(&["init", "-q"]);
    run_git(&["config", "user.name", "test"]);
    run_git(&["config", "user.email", "test@example.invalid"]);
    run_git(&["config", "commit.gpgsign", "false"]);
    run_git(&["add", "alpha/v0.1", "witforge.toml"]);
    run_git(&["commit", "-q", "-m", "v0.1 only"]);
    let git_base_cfg = witforge()
        .current_dir(&base)
        .args([
            "diff",
            "--git-base",
            "HEAD",
            "--project",
            "alpha",
            "--check",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        git_base_cfg.status.success(),
        "git-base + --project alpha + policy.diff-check should pass: {git_base_cfg:?}"
    );
    let gb_json: serde_json::Value = serde_json::from_slice(&git_base_cfg.stdout).unwrap();
    assert_eq!(gb_json["old"]["path"], "HEAD:alpha/v0.1", "{gb_json}");
    assert_eq!(gb_json["new"]["version"], "0.2.0", "{gb_json}");

    // 4. Invalid policy configurations fail with exit 2 (`WF9001` / `WF9003`).
    let bad_empty = base.join("bad-empty.toml");
    fs::write(
        &bad_empty,
        "version = 1\n[project.a]\nroot = \"alpha\"\n[project.a.policy]\ndiff-check = []\n",
    )
    .unwrap();
    let empty_out = witforge()
        .current_dir(&base)
        .args([
            "diff",
            "alpha/v0.1",
            "alpha/v0.2",
            "--config",
            bad_empty.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(empty_out.status.code(), Some(2), "{empty_out:?}");
    let empty_json: serde_json::Value = serde_json::from_slice(&empty_out.stdout).unwrap();
    assert_eq!(empty_json["diagnostics"][0]["code"], "WF9001");

    let bad_view = base.join("bad-view.toml");
    fs::write(
        &bad_view,
        "version = 1\n[project.a]\nroot = \"alpha\"\n[project.a.policy]\ndiff-check = [\"bogus\"]\n",
    )
    .unwrap();
    let view_out = witforge()
        .current_dir(&base)
        .args([
            "diff",
            "alpha/v0.1",
            "alpha/v0.2",
            "--config",
            bad_view.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(view_out.status.code(), Some(2), "{view_out:?}");
    let view_json: serde_json::Value = serde_json::from_slice(&view_out.stdout).unwrap();
    assert_eq!(view_json["diagnostics"][0]["code"], "WF9001");

    let bad_key = base.join("bad-key.toml");
    fs::write(
        &bad_key,
        "version = 1\n[project.a]\nroot = \"alpha\"\n[project.a.policy]\nunknown-key = true\n",
    )
    .unwrap();
    let key_out = witforge()
        .current_dir(&base)
        .args([
            "check",
            "--config",
            bad_key.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(key_out.status.code(), Some(2), "{key_out:?}");
    let key_json: serde_json::Value = serde_json::from_slice(&key_out.stdout).unwrap();
    assert_eq!(key_json["diagnostics"][0]["code"], "WF9003");

    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn eq_hash_and_try_from_config_and_cli_overrides_work() {
    let root = project("eq-hash-try-from");
    let config = write_config(
        &root,
        r#"
version = 1

[project.demo]
root = "wit"

[project.demo.rust]
out = "generated/rust"
eq-hash = true
try-from = true
"#,
    );

    // 1. Config enables both `eq-hash` and `try-from`.
    let gen_ok = witforge()
        .current_dir(&root)
        .args(["generate", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(gen_ok.status.success(), "{gen_ok:?}");
    let v01 = fs::read_to_string(root.join("generated/rust/v0_1.rs")).unwrap();
    assert!(v01.contains("PartialEq, Eq, Hash"), "{v01}");
    let conv = fs::read_to_string(root.join("generated/rust/convert.rs")).unwrap();
    assert!(conv.contains("impl ::core::convert::TryFrom<"), "{conv}");

    // 2. `--no-eq-hash` and `--no-try-from` override config cleanly.
    let no_flags = witforge()
        .current_dir(&root)
        .args(["rust", "--config"])
        .arg(&config)
        .args([
            "--no-eq-hash",
            "--no-try-from",
            "--out",
            "generated/rust-plain",
        ])
        .output()
        .unwrap();
    assert!(no_flags.status.success(), "{no_flags:?}");
    let plain_v01 = fs::read_to_string(root.join("generated/rust-plain/v0_1.rs")).unwrap();
    assert!(!plain_v01.contains("Eq, Hash"), "{plain_v01}");
    let plain_conv = fs::read_to_string(root.join("generated/rust-plain/convert.rs")).unwrap();
    assert!(!plain_conv.contains("TryFrom"), "{plain_conv}");

    // 3. Direct CLI `--eq-hash --try-from` works without config.
    let cli_flags = witforge()
        .current_dir(&root)
        .args([
            "rust",
            "wit",
            "--out",
            "generated/rust-cli",
            "--eq-hash",
            "--try-from",
            "--no-config",
        ])
        .output()
        .unwrap();
    assert!(cli_flags.status.success(), "{cli_flags:?}");
    let cli_v01 = fs::read_to_string(root.join("generated/rust-cli/v0_1.rs")).unwrap();
    assert!(cli_v01.contains("PartialEq, Eq, Hash"), "{cli_v01}");
    let cli_conv = fs::read_to_string(root.join("generated/rust-cli/convert.rs")).unwrap();
    assert!(
        cli_conv.contains("impl ::core::convert::TryFrom<"),
        "{cli_conv}"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doc_emitter_works_via_cli_config_generate_check_and_diff() {
    let root = project("doc-emitter");
    let config = write_config(
        &root,
        r#"
version = 1

[project.demo]
root = "wit"

[project.demo.doc]
out = "generated/docs"
"#,
    );

    // 1. `witforge generate` generates Markdown docs when `[project.<name>.doc]` is configured.
    let gen_out = witforge()
        .current_dir(&root)
        .args(["generate", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(gen_out.status.success(), "{gen_out:?}");
    assert!(root.join("generated/docs/index.md").is_file());
    assert!(root.join("generated/docs/v0_1.md").is_file());
    assert!(root.join("generated/docs/v0_2.md").is_file());

    // 2. `witforge doc --check` passes cleanly.
    let check_ok = witforge()
        .current_dir(&root)
        .args(["doc", "--check", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(check_ok.status.success(), "{check_ok:?}");

    // 3. Tamper -> `witforge doc --check --diff` exits 1 and prints unified diff.
    let mut index = fs::read_to_string(root.join("generated/docs/index.md")).unwrap();
    index.push_str("\n# drift\n");
    fs::write(root.join("generated/docs/index.md"), index).unwrap();
    let check_drift = witforge()
        .current_dir(&root)
        .args(["doc", "--check", "--diff", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert_eq!(check_drift.status.code(), Some(1), "{check_drift:?}");
    assert!(
        String::from_utf8_lossy(&check_drift.stdout).contains("-# drift"),
        "{check_drift:?}"
    );

    // 4. Direct CLI invocation without config works cleanly.
    let direct = witforge()
        .current_dir(&root)
        .args(["doc", "wit", "--out", "direct-docs", "--no-config"])
        .output()
        .unwrap();
    assert!(direct.status.success(), "{direct:?}");
    assert!(root.join("direct-docs/index.md").is_file());

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn eighth_wave_audit_malformed_enclosing_config_fails_closed_for_policy() {
    let base = std::env::temp_dir().join(format!(
        "witforge-policy-fail-closed-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&base);
    let repo = base.join("repo");
    let external = base.join("external");
    fs::create_dir_all(repo.join(".git")).unwrap();
    copy_tree(&source_tree(), &repo.join("wit"));
    copy_tree(&source_tree(), &external.join("wit"));

    // Write a malformed `witforge.toml` enclosing `repo/wit`.
    fs::write(repo.join("witforge.toml"), "version = [malformed\n").unwrap();

    // 1. `witforge diff wit/v0.1 wit/v0.2 --check` inside `repo` MUST fail closed (exit 2, WF9003).
    let diff_fail = witforge()
        .current_dir(&repo)
        .args([
            "diff", "wit/v0.1", "wit/v0.2", "--check", "--format", "json",
        ])
        .output()
        .unwrap();
    assert_eq!(diff_fail.status.code(), Some(2), "{diff_fail:?}");
    let diff_json: serde_json::Value = serde_json::from_slice(&diff_fail.stdout).unwrap();
    assert_eq!(diff_json["diagnostics"][0]["code"], "WF9003");

    // `--no-config` bypasses the malformed config and runs default `--check` (exits 1 on API add).
    let diff_bypass = witforge()
        .current_dir(&repo)
        .args(["diff", "wit/v0.1", "wit/v0.2", "--check", "--no-config"])
        .output()
        .unwrap();
    assert_eq!(diff_bypass.status.code(), Some(1), "{diff_bypass:?}");

    // 2. `witforge release wit/v0.1` inside `repo` MUST fail closed (exit 2, WF9003).
    let rel_fail = witforge()
        .current_dir(&repo)
        .args(["release", "wit/v0.1", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(rel_fail.status.code(), Some(2), "{rel_fail:?}");
    let rel_json: serde_json::Value = serde_json::from_slice(&rel_fail.stdout).unwrap();
    assert_eq!(rel_json["diagnostics"][0]["code"], "WF9003");

    // 3. Diffing an external tree OUTSIDE `repo` from cwd inside `repo` ignores the unrelated
    // malformed `repo/witforge.toml` cleanly.
    let ext_diff = witforge()
        .current_dir(&repo)
        .args(["diff"])
        .arg(external.join("wit/v0.1"))
        .arg(external.join("wit/v0.2"))
        .arg("--check")
        .output()
        .unwrap();
    assert_eq!(ext_diff.status.code(), Some(1), "{ext_diff:?}");

    fs::remove_dir_all(&base).unwrap();
}
