//! Shared compile-and-round-trip harness for witforge-generated Rust, protobuf, and glue output.
//!
//! Honours `TMPDIR` (via `std::env::temp_dir()`), reuses a per-rustc build cache under `TMPDIR`
//! with the workspace's `Cargo.lock` so scratch builds run `--offline` without network access,
//! removes each scratch crate on success, and keeps it (printing its path) on failure.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use prost::Message as _;
use witforge::emit::{glue, proto, rust};
use witforge::load;
use witforge::lock::{LockMode, WireLock};
use witforge::model;

static SCRATCH_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Rust edition to compile a scratch crate with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustEdition {
    /// Rust 2021 (`resolver = "2"`).
    E2021,
    /// Rust 2024 (`resolver = "3"`).
    E2024,
}

impl RustEdition {
    /// Edition string for `Cargo.toml` and `rustfmt --edition`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::E2021 => "2021",
            Self::E2024 => "2024",
        }
    }

    /// Cargo workspace resolver matching this edition.
    pub const fn resolver(self) -> &'static str {
        match self {
            Self::E2021 => "2",
            Self::E2024 => "3",
        }
    }
}

/// Quality gates checked on generated Rust and glue modules in a scratch crate.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy)]
pub struct QualityChecks {
    /// Verify generated plain Rust and glue match `rustfmt` / `cargo fmt --check`.
    pub fmt: bool,
    /// Run `cargo clippy --all-targets -- -D warnings` on the scratch crate.
    pub clippy: bool,
    /// Enforce `#[deny(missing_docs)]` on generated plain Rust and glue modules.
    pub missing_docs: bool,
    /// Run `RUSTDOCFLAGS='-D warnings' cargo doc --lib --no-deps`.
    pub rustdoc: bool,
    /// Run `cargo test --doc` to ensure WIT docs never create failing doctests.
    pub doctest: bool,
}

impl QualityChecks {
    /// Full quality suite (rustfmt, Clippy `-D warnings`, `missing_docs`, `cargo doc -D warnings`,
    /// and `cargo test --doc`).
    pub const ALL: Self = Self {
        fmt: true,
        clippy: true,
        missing_docs: true,
        rustdoc: true,
        doctest: true,
    };

    /// Compile and run only (used by targeted glue unit tests that focus on runtime behavior).
    pub const RUN_ONLY: Self = Self {
        fmt: false,
        clippy: false,
        missing_docs: false,
        rustdoc: false,
        doctest: false,
    };
}

/// A scratch directory under `TMPDIR` that is removed on `finish()` and kept (with its path
/// printed to stderr) if dropped early due to a test failure or panic.
pub struct ScratchDir {
    path: PathBuf,
    finished: bool,
}

impl ScratchDir {
    /// Create a fresh scratch directory under `std::env::temp_dir()` with a short Windows-safe
    /// name.
    pub fn new(tag: &str) -> Self {
        let seq = SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("wf-{tag}-{}-{seq}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self {
            path: dir,
            finished: false,
        }
    }

    /// Path to the scratch directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Mark the test as passed and delete the scratch directory.
    pub fn finish(mut self) {
        self.finished = true;
        let _ = fs::remove_dir_all(&self.path);
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        if !self.finished {
            eprintln!(
                "witforge compile harness kept failing scratch directory at: {}",
                self.path.display()
            );
        }
    }
}

/// Locate the workspace root directory containing `Cargo.lock`.
pub fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .expect("crate lives under <workspace>/crates/<crate>");
    assert!(
        root.join("Cargo.lock").is_file(),
        "expected Cargo.lock at {}",
        root.display()
    );
    root.to_path_buf()
}

/// Shared `CARGO_TARGET_DIR` under `TMPDIR`, keyed by rustc version so stable and MSRV 1.88
/// caches do not invalidate each other while keeping paths short on Windows.
pub fn shared_target_dir() -> PathBuf {
    static TARGET_DIR: OnceLock<PathBuf> = OnceLock::new();
    TARGET_DIR
        .get_or_init(|| {
            let output = Command::new("rustc").arg("-V").output().unwrap();
            let text = String::from_utf8_lossy(&output.stdout);
            let version = text
                .split_whitespace()
                .nth(1)
                .unwrap_or("unknown")
                .replace(['.', '-'], "_");
            let dir = std::env::temp_dir().join(format!("wf-t-{version}"));
            fs::create_dir_all(&dir).unwrap();
            dir
        })
        .clone()
}

/// Recursively copy a directory tree.
pub fn copy_dir_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Compile every `.proto` file in `proto_dir` using `protox` + `prost-build` (and optionally
/// `pbjson-build`), writing a self-contained Rust module tree into `out_dir` (`out_dir/mod.rs`
/// plus included files).
///
/// - When `has_deps` is `false`, glue expects `<proto-root>::v<major>::*`, so each `<pkg>.v<M>.rs`
///   is wired under `pub mod v<M> { include!("v<M>.rs"); ... }`.
/// - When `has_deps` is `true`, glue expects `<proto-root>::<ns>::<pkg>::v<M>::*`, matching
///   `prost-build`'s `include_file("mod.rs")` layout directly.
#[allow(clippy::too_many_lines)]
pub fn compile_proto_module(proto_dir: &Path, out_dir: &Path, has_deps: bool, pbjson: bool) {
    let mut proto_paths: Vec<PathBuf> = fs::read_dir(proto_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "proto"))
        .collect();
    proto_paths.sort();
    assert!(
        !proto_paths.is_empty(),
        "no .proto files found in {}",
        proto_dir.display()
    );

    let descriptors = protox::compile(&proto_paths, [proto_dir]).unwrap_or_else(|err| {
        panic!(
            "protox failed to compile {:?} in {}: {err}",
            proto_paths,
            proto_dir.display()
        )
    });
    let encoded_descriptors = descriptors.encode_to_vec();

    // Collect top-level package prefixes (`.foo`) for pbjson if requested.
    let mut pbjson_prefixes: BTreeSet<String> = BTreeSet::new();
    for file in &descriptors.file {
        let pkg = file.package();
        if let Some(first) = pkg.split('.').next()
            && !first.is_empty()
        {
            pbjson_prefixes.insert(format!(".{first}"));
        }
    }

    fs::create_dir_all(out_dir).unwrap();

    if has_deps {
        prost_build::Config::new()
            .out_dir(out_dir)
            .include_file("mod.rs")
            .compile_fds(descriptors)
            .expect("prost-build failed");

        if pbjson {
            let prefix_refs: Vec<&str> = pbjson_prefixes.iter().map(String::as_str).collect();
            pbjson_build::Builder::new()
                .out_dir(out_dir)
                .register_descriptors(&encoded_descriptors)
                .expect("pbjson register_descriptors failed")
                .build(&prefix_refs)
                .expect("pbjson build failed");

            let mut mod_rs = fs::read_to_string(out_dir.join("mod.rs")).unwrap();
            let mut serde_files: Vec<String> = fs::read_dir(out_dir)
                .unwrap()
                .filter_map(|e| {
                    let name = e.ok()?.file_name().to_string_lossy().into_owned();
                    name.ends_with(".serde.rs").then_some(name)
                })
                .collect();
            serde_files.sort();
            for serde_file in serde_files {
                let stem = serde_file.trim_end_matches(".serde.rs");
                let needle = format!("include!(\"{stem}.rs\");");
                let replacement = format!("{needle}\ninclude!(\"{serde_file}\");");
                mod_rs = mod_rs.replace(&needle, &replacement);
            }
            fs::write(out_dir.join("mod.rs"), mod_rs).unwrap();
        }
    } else {
        let work = out_dir.join("_work");
        fs::create_dir_all(&work).unwrap();
        prost_build::Config::new()
            .out_dir(&work)
            .compile_fds(descriptors)
            .expect("prost-build failed");

        if pbjson {
            let prefix_refs: Vec<&str> = pbjson_prefixes.iter().map(String::as_str).collect();
            pbjson_build::Builder::new()
                .out_dir(&work)
                .register_descriptors(&encoded_descriptors)
                .expect("pbjson register_descriptors failed")
                .build(&prefix_refs)
                .expect("pbjson build failed");
        }

        let mut majors: BTreeMap<String, (PathBuf, Option<PathBuf>)> = BTreeMap::new();
        for entry in fs::read_dir(&work).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name.ends_with(".serde.rs")
                || !path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
            {
                continue;
            }
            let stem = name.trim_end_matches(".rs");
            let major = stem.rsplit('.').next().unwrap_or("v0").to_string();
            let serde_path = work.join(format!("{stem}.serde.rs"));
            let serde = serde_path.is_file().then_some(serde_path);
            majors.insert(major, (path, serde));
        }

        let mut mod_rs = String::new();
        for (major, (rs_path, serde_path)) in &majors {
            let dest_rs = out_dir.join(format!("{major}.rs"));
            fs::copy(rs_path, &dest_rs).unwrap();
            let _ = writeln!(mod_rs, "pub mod {major} {{");
            let _ = writeln!(mod_rs, "    include!(\"{major}.rs\");");
            if let Some(serde) = serde_path {
                let dest_serde = out_dir.join(format!("{major}.serde.rs"));
                fs::copy(serde, &dest_serde).unwrap();
                let _ = writeln!(mod_rs, "    include!(\"{major}.serde.rs\");");
            }
            mod_rs.push_str("}\n");
        }
        fs::write(out_dir.join("mod.rs"), mod_rs).unwrap();
        let _ = fs::remove_dir_all(&work);
    }
}

/// Generate a fresh `wire.lock` in `version_dir` if one does not already exist.
pub fn ensure_lock(version_dir: &Path) {
    let lock_path = version_dir.join("wire.lock");
    if lock_path.is_file() {
        return;
    }
    let contract = load(version_dir).unwrap();
    let built = model::build(&contract).unwrap();
    let mut lock = WireLock::default();
    lock.reconcile_with(&built.members().unwrap(), LockMode::First)
        .unwrap();
    fs::write(lock_path, lock.render()).unwrap();
}

/// One module case placed inside a batched scratch crate.
struct BatchModule {
    mod_name: String,
    plain_rel: String,
    proto_rel: String,
    glue_rel: String,
    rust_files: Vec<PathBuf>,
    roundtrip_body: String,
}

/// A batched scratch crate that holds one or more generated contracts as modules so dependencies
/// (`prost`, `pbjson`, `serde`, `serde_json`) are compiled once per batch.
pub struct HarnessBatch {
    scratch: ScratchDir,
    edition: RustEdition,
    checks: QualityChecks,
    use_pbjson: bool,
    use_serde: bool,
    extra_deps: String,
    modules: Vec<BatchModule>,
}

impl HarnessBatch {
    /// Start a new batched scratch crate under `TMPDIR`.
    pub fn new(tag: &str, edition: RustEdition, checks: QualityChecks) -> Self {
        let scratch = ScratchDir::new(tag);
        fs::create_dir_all(scratch.path().join("src/bin")).unwrap();
        Self {
            scratch,
            edition,
            checks,
            use_pbjson: false,
            use_serde: false,
            extra_deps: String::new(),
            modules: Vec::new(),
        }
    }

    /// Enable `pbjson`, `serde`, and `serde_json` dependencies in the scratch crate.
    pub fn enable_pbjson(&mut self) -> &mut Self {
        self.use_pbjson = true;
        self.use_serde = true;
        self
    }

    /// Enable `serde` and `serde_json` dependencies in the scratch crate.
    pub fn enable_serde(&mut self) -> &mut Self {
        self.use_serde = true;
        self
    }

    /// Append extra `[dependencies]` lines to the scratch crate's `Cargo.toml`.
    pub fn add_extra_deps(&mut self, deps: &str) -> &mut Self {
        self.extra_deps.push_str(deps);
        self
    }

    /// Path to the scratch crate root.
    pub fn root(&self) -> &Path {
        self.scratch.path()
    }

    /// Generate plain Rust, protobuf (+ prost / optional pbjson), and glue from a versioned WIT
    /// root (`wit/v0.1`, ...) and mount them under `crate::<mod_name>::{plain,proto,glue}`.
    pub fn add_wit_root(
        &mut self,
        mod_name: &str,
        wit_root: &Path,
        pbjson: bool,
        serde_plain: bool,
        roundtrip_body: impl Into<String>,
    ) -> &mut Self {
        if pbjson {
            self.enable_pbjson();
        }
        if serde_plain {
            self.enable_serde();
        }
        let mod_dir = self.scratch.path().join("src").join(mod_name);
        let plain_dir = mod_dir.join("plain");
        let proto_src_dir = mod_dir.join("_proto_src");
        let proto_dir = mod_dir.join("proto");
        let glue_dir = mod_dir.join("glue");

        let plain_root = format!("crate::{mod_name}::plain");
        let proto_root = format!("crate::{mod_name}::proto");

        let plain_plan = rust::plan_with_serde(wit_root, serde_plain)
            .unwrap_or_else(|e| panic!("{mod_name}: rust::plan failed: {e:#}"));
        let proto_plan = proto::plan(wit_root)
            .unwrap_or_else(|e| panic!("{mod_name}: proto::plan failed: {e:#}"));
        let glue_plan = glue::plan(wit_root, &plain_root, &proto_root)
            .unwrap_or_else(|e| panic!("{mod_name}: glue::plan failed: {e:#}"));

        let has_deps = plain_plan.files().contains_key("witforge_deps.rs");
        plain_plan.write(&plain_dir).unwrap();
        proto_plan.write(&proto_src_dir).unwrap();
        glue_plan.write(&glue_dir).unwrap();

        compile_proto_module(&proto_src_dir, &proto_dir, has_deps, pbjson);

        let mut rust_files = collect_rs_files(&plain_dir);
        rust_files.extend(collect_rs_files(&glue_dir));

        self.modules.push(BatchModule {
            mod_name: mod_name.to_string(),
            plain_rel: "plain".to_string(),
            proto_rel: "proto".to_string(),
            glue_rel: "glue".to_string(),
            rust_files,
            roundtrip_body: roundtrip_body.into(),
        });
        self
    }

    /// Mount pre-generated directories (`plain`, `proto`, `glue`) under custom submodule names
    /// inside `crate::<mod_name>::{plain_mod, proto_mod, glue_mod}` (used for committed examples
    /// whose `witforge.toml` configures `crate::generated::{rust,proto}` and for CLI E2E output).
    #[allow(clippy::too_many_arguments)]
    pub fn add_generated_dirs(
        &mut self,
        mod_name: &str,
        plain_mod: &str,
        proto_mod: &str,
        glue_mod: &str,
        plain_src: &Path,
        proto_src: &Path,
        glue_src: &Path,
        pbjson: bool,
        roundtrip_body: impl Into<String>,
    ) -> &mut Self {
        if pbjson {
            self.enable_pbjson();
        }
        let mod_dir = self.scratch.path().join("src").join(mod_name);
        let plain_dir = mod_dir.join(plain_mod);
        let proto_dir = mod_dir.join(proto_mod);
        let glue_dir = mod_dir.join(glue_mod);

        copy_dir_tree(plain_src, &plain_dir);
        copy_dir_tree(glue_src, &glue_dir);
        let has_deps = plain_dir.join("witforge_deps.rs").is_file();
        compile_proto_module(proto_src, &proto_dir, has_deps, pbjson);

        let mut rust_files = collect_rs_files(&plain_dir);
        rust_files.extend(collect_rs_files(&glue_dir));

        self.modules.push(BatchModule {
            mod_name: mod_name.to_string(),
            plain_rel: plain_mod.to_string(),
            proto_rel: proto_mod.to_string(),
            glue_rel: glue_mod.to_string(),
            rust_files,
            roundtrip_body: roundtrip_body.into(),
        });
        self
    }

    /// Build, verify every enabled quality gate, execute all round-trip tests, and remove the
    /// scratch directory on success.
    #[allow(clippy::too_many_lines)]
    pub fn verify_and_run(self, label: &str) {
        let root = self.scratch.path().to_path_buf();
        let target = shared_target_dir();

        // Write `Cargo.toml` and copy the workspace `Cargo.lock` so builds stay `--offline`.
        let mut deps = String::from("prost = \"0.14\"\n");
        if self.use_pbjson {
            deps.push_str("pbjson = \"0.9\"\n");
        }
        if self.use_serde {
            deps.push_str(
                "serde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n",
            );
        }
        deps.push_str(&self.extra_deps);

        let manifest = format!(
            "[package]\n\
             name = \"wf-harness\"\n\
             version = \"0.0.0\"\n\
             edition = \"{edition}\"\n\
             rust-version = \"1.88\"\n\
             publish = false\n\n\
             [lib]\n\
             name = \"wf_harness\"\n\
             path = \"src/lib.rs\"\n\n\
             [[bin]]\n\
             name = \"run\"\n\
             path = \"src/bin/run.rs\"\n\n\
             [dependencies]\n\
             {deps}\n\
             [workspace]\n\
             resolver = \"{resolver}\"\n",
            edition = self.edition.as_str(),
            resolver = self.edition.resolver(),
        );
        let manifest_path = root.join("Cargo.toml");
        fs::write(&manifest_path, manifest).unwrap();
        fs::copy(workspace_root().join("Cargo.lock"), root.join("Cargo.lock")).unwrap();

        // Write `src/lib.rs`, per-case `mod.rs`, `src/roundtrips.rs`, and `src/bin/run.rs`.
        let mut lib_rs = String::from(
            "//! Generated contract compile-and-round-trip harness.\n\n\
             #![deny(warnings)]\n\n",
        );
        let mut roundtrips_rs = String::from(
            "#![allow(unused_imports, dead_code, clippy::all)]\n\n\
             pub fn run_all() {\n",
        );
        let mut all_generated_rs: Vec<PathBuf> = Vec::new();

        for module in &self.modules {
            let mod_name = &module.mod_name;
            let _ = writeln!(lib_rs, "/// Compiled contract case `{mod_name}`.");
            let _ = writeln!(lib_rs, "pub mod {mod_name};\n");

            let mod_dir = root.join("src").join(mod_name);
            let deny_attr = if self.checks.missing_docs {
                "#[deny(warnings, missing_docs)]"
            } else {
                "#[deny(warnings)]"
            };
            let mod_rs = format!(
                "/// Generated plain Rust bindings.\n\
                 {deny_attr}\n\
                 pub mod {plain};\n\n\
                 /// Generated protobuf bindings.\n\
                 #[allow(warnings, missing_docs, clippy::all)]\n\
                 #[rustfmt::skip]\n\
                 pub mod {proto};\n\n\
                 /// Generated protobuf glue.\n\
                 {deny_attr}\n\
                 pub mod {glue};\n",
                plain = module.plain_rel,
                proto = module.proto_rel,
                glue = module.glue_rel,
            );
            fs::write(mod_dir.join("mod.rs"), mod_rs).unwrap();
            all_generated_rs.extend(module.rust_files.iter().cloned());

            let _ = writeln!(roundtrips_rs, "    run_{mod_name}();");
        }
        roundtrips_rs.push_str("}\n");

        for module in &self.modules {
            let _ = writeln!(
                roundtrips_rs,
                "\nfn run_{}() {{\n{}\n}}",
                module.mod_name, module.roundtrip_body
            );
        }

        lib_rs.push_str(
            "#[rustfmt::skip]\n\
             #[allow(warnings, missing_docs, clippy::all)]\n\
             mod roundtrips;\n\n\
             /// Execute every round-trip check registered in this batch.\n\
             pub fn run_all() {\n    \
                 roundtrips::run_all();\n\
             }\n",
        );
        fs::write(root.join("src/lib.rs"), lib_rs).unwrap();
        fs::write(root.join("src/roundtrips.rs"), roundtrips_rs).unwrap();
        fs::write(
            root.join("src/bin/run.rs"),
            "fn main() {\n    wf_harness::run_all();\n}\n",
        )
        .unwrap();

        // 1. Formatting gate: `rustfmt --edition <E> --style-edition <E> --check` on every
        //    witforge-generated Rust & glue file AND `cargo fmt -- --check` on the crate.
        if self.checks.fmt {
            let ed = self.edition.as_str();
            let mut fmt_cmd = Command::new("rustfmt");
            fmt_cmd
                .args(["--edition", ed, "--style-edition", ed, "--check"])
                .args(&all_generated_rs);
            let output = fmt_cmd.output().expect("failed to run rustfmt");
            assert!(
                output.status.success(),
                "{label} (edition {ed}) rustfmt --check failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );

            let cargo_fmt = Command::new(env!("CARGO"))
                .args(["fmt", "--manifest-path"])
                .arg(&manifest_path)
                .args(["--", "--check"])
                .output()
                .expect("failed to run cargo fmt");
            assert!(
                cargo_fmt.status.success(),
                "{label} (edition {ed}) cargo fmt --check failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&cargo_fmt.stdout),
                String::from_utf8_lossy(&cargo_fmt.stderr)
            );
        }

        // 2. Lint gate: `cargo clippy --offline --all-targets -- -D warnings`.
        if self.checks.clippy {
            let clippy = Command::new(env!("CARGO"))
                .args(["clippy", "--offline", "--all-targets", "--manifest-path"])
                .arg(&manifest_path)
                .args(["--", "-D", "warnings"])
                .env("CARGO_TARGET_DIR", &target)
                .env("CARGO_TERM_COLOR", "never")
                .env_remove("RUSTFLAGS")
                .output()
                .expect("failed to run cargo clippy");
            assert!(
                clippy.status.success(),
                "{label} (edition {}) cargo clippy -D warnings failed:\nstdout:\n{}\nstderr:\n{}",
                self.edition.as_str(),
                String::from_utf8_lossy(&clippy.stdout),
                String::from_utf8_lossy(&clippy.stderr)
            );
        }

        // 3. Documentation gate: `RUSTDOCFLAGS='-D warnings' cargo doc --lib --no-deps --offline`.
        if self.checks.rustdoc {
            let doc = Command::new(env!("CARGO"))
                .args(["doc", "--lib", "--no-deps", "--offline", "--manifest-path"])
                .arg(&manifest_path)
                .env("CARGO_TARGET_DIR", &target)
                .env("CARGO_TERM_COLOR", "never")
                .env("RUSTDOCFLAGS", "-D warnings")
                .env_remove("RUSTFLAGS")
                .output()
                .expect("failed to run cargo doc");
            assert!(
                doc.status.success(),
                "{label} (edition {}) cargo doc -D warnings failed:\nstdout:\n{}\nstderr:\n{}",
                self.edition.as_str(),
                String::from_utf8_lossy(&doc.stdout),
                String::from_utf8_lossy(&doc.stderr)
            );
        }

        // 4. Doctest gate: `cargo test --doc --offline`.
        if self.checks.doctest {
            let doctest = Command::new(env!("CARGO"))
                .args(["test", "--doc", "--offline", "--manifest-path"])
                .arg(&manifest_path)
                .env("CARGO_TARGET_DIR", &target)
                .env("CARGO_TERM_COLOR", "never")
                .env_remove("RUSTFLAGS")
                .output()
                .expect("failed to run cargo test --doc");
            assert!(
                doctest.status.success(),
                "{label} (edition {}) cargo test --doc failed:\nstdout:\n{}\nstderr:\n{}",
                self.edition.as_str(),
                String::from_utf8_lossy(&doctest.stdout),
                String::from_utf8_lossy(&doctest.stderr)
            );
        }

        // 5. Compile & execute the round-trip binary (`-D warnings` via `#![deny(warnings)]`).
        let run = Command::new(env!("CARGO"))
            .args([
                "run",
                "--quiet",
                "--offline",
                "--bin",
                "run",
                "--manifest-path",
            ])
            .arg(&manifest_path)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_TERM_COLOR", "never")
            .env_remove("RUSTFLAGS")
            .output()
            .expect("failed to run cargo run");
        assert!(
            run.status.success(),
            "{label} (edition {}) round-trip execution failed:\nstdout:\n{}\nstderr:\n{}",
            self.edition.as_str(),
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );

        self.scratch.finish();
    }
}

/// Stage a single WIT contract directory (without a `v<major>.<minor>` parent) into a versioned
/// WIT root under `dst_root` with a fresh `wire.lock`.
pub fn stage_single_version_root(wit_dir: &Path, dst_root: &Path) {
    let contract = load(wit_dir).unwrap();
    let built = model::build(&contract).unwrap();
    let version_dir = dst_root.join(format!(
        "v{}.{}",
        built.package.version.major, built.package.version.minor
    ));
    copy_dir_tree(wit_dir, &version_dir);
    ensure_lock(&version_dir);
}

/// Compile and execute pre-generated `plain`, `proto`, and `glue` directories (with
/// `--plain-root crate::plain --proto-root crate::proto`) against `main_body` using the shared
/// harness, `ScratchDir` lifecycle, and cached target directory.
pub fn compile_and_run_generated(
    tag: &str,
    plain_src: &Path,
    proto_src: &Path,
    glue_src: &Path,
    pbjson: bool,
    main_body: &str,
    label: &str,
) {
    let scratch = ScratchDir::new(tag);
    let root = scratch.path();
    copy_dir_tree(plain_src, &root.join("plain"));
    copy_dir_tree(glue_src, &root.join("glue"));
    let has_deps = root.join("plain/witforge_deps.rs").is_file();
    compile_proto_module(proto_src, &root.join("proto"), has_deps, pbjson);

    let mut main = String::from("mod glue;\nmod plain;\nmod proto;\n\n");
    main.push_str(main_body);
    fs::write(root.join("main.rs"), main).unwrap();

    let deps = if pbjson {
        "prost = \"0.14\"\npbjson = \"0.9\"\nserde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n"
    } else {
        "prost = \"0.14\"\n"
    };
    run_cargo_program(root, deps, label);
    scratch.finish();
}

/// Generate `plain`, `proto`, and `glue` from a versioned WIT root (`wit/v0.1`, ...), compile
/// against `prost-build` (and optional `pbjson-build`), and run `main_body` using the shared
/// harness.
pub fn compile_and_run_wit_root(
    tag: &str,
    wit_root: &Path,
    pbjson: bool,
    main_body: &str,
    label: &str,
) {
    let scratch = ScratchDir::new(tag);
    let root = scratch.path();
    let plain_dir = root.join("plain");
    let proto_src = root.join("_proto_src");
    let glue_dir = root.join("glue");

    let plain_plan = rust::plan(wit_root).unwrap();
    let has_deps = plain_plan.files().contains_key("witforge_deps.rs");
    plain_plan.write(&plain_dir).unwrap();
    proto::plan(wit_root).unwrap().write(&proto_src).unwrap();
    glue::plan(wit_root, "crate::plain", "crate::proto")
        .unwrap()
        .write(&glue_dir)
        .unwrap();

    compile_proto_module(&proto_src, &root.join("proto"), has_deps, pbjson);

    let mut main = String::from("mod glue;\nmod plain;\nmod proto;\n\n");
    main.push_str(main_body);
    fs::write(root.join("main.rs"), main).unwrap();

    let deps = if pbjson {
        "prost = \"0.14\"\npbjson = \"0.9\"\nserde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n"
    } else {
        "prost = \"0.14\"\n"
    };
    run_cargo_program(root, deps, label);
    scratch.finish();
}

/// Compile and run a single stand-alone scratch crate whose `main.rs` is already written at
/// `root/main.rs` (used by tests with custom multi-crate or non-standard layouts).
///
/// Shares the cached `CARGO_TARGET_DIR` and workspace `Cargo.lock` so builds stay `--offline`
/// and fast.
pub fn run_cargo_program(root: &Path, dependencies: &str, label: &str) {
    let manifest = format!(
        "[package]\nname = \"wf-prog\"\nversion = \"0.0.0\"\nedition = \"2024\"\nrust-version = \"1.88\"\npublish = false\n\n[[bin]]\nname = \"wf-prog\"\npath = \"main.rs\"\n\n[dependencies]\n{dependencies}\n\n[workspace]\nresolver = \"3\"\n"
    );
    let manifest_path = root.join("Cargo.toml");
    fs::write(&manifest_path, manifest).unwrap();
    fs::copy(workspace_root().join("Cargo.lock"), root.join("Cargo.lock")).unwrap();
    let target = shared_target_dir();
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
        "{label} failed (kept scratch dir {}):\nstdout:\n{}\nstderr:\n{}",
        root.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}
