//! Integration tests verifying `witforge-build` on standalone and `deps/` generated `.proto`
//! trees (both default and with the optional `pbjson` feature), uppercase/keyword packages,
//! multi-major roots, `no_std`, and error/incremental guards.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::emit::{glue, proto, rust};
use witforge::workflow::{Baseline, VersionOptions, lock as lock_contract};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn fixture(rel: &str) -> PathBuf {
    workspace_root()
        .join("crates/witforge/tests/fixtures")
        .join(rel)
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("witforge-build-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn rustc_version_key() -> String {
    let out = Command::new("rustc").arg("-V").output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn lock_version_dir(version_dir: &Path) {
    let mut opts = VersionOptions::new(version_dir.to_path_buf());
    opts.baseline = Baseline::First;
    let outcome = lock_contract(&opts).unwrap();
    outcome.write().unwrap();
}

#[cfg(feature = "pbjson")]
fn maybe_enable_pbjson(cfg: witforge_build::Config) -> witforge_build::Config {
    cfg.with_pbjson_builder(|builder| {
        builder.ignore_unknown_fields();
    })
}

#[cfg(not(feature = "pbjson"))]
fn maybe_enable_pbjson(cfg: witforge_build::Config) -> witforge_build::Config {
    cfg
}

#[allow(clippy::too_many_lines)]
#[test]
fn discovers_and_compiles_standalone_and_deps_proto_trees_with_rerun_directives() {
    let base = scratch("direct");

    // 1. Standalone multi-version contract (no deps).
    let standalone_wit = workspace_root().join("examples/evolved/wit");
    let standalone_proto = base.join("standalone-proto");
    proto::plan(&standalone_wit)
        .unwrap()
        .write(&standalone_proto)
        .unwrap();
    let standalone_out = base.join("standalone-out");
    let fds_path = base.join("standalone.fds.bin");

    let cfg = maybe_enable_pbjson(
        witforge_build::Config::new()
            .out_dir(&standalone_out)
            .file_descriptor_set_path(&fds_path)
            .emit_rerun_if_changed(false),
    );
    let output = cfg.compile_protos(&standalone_proto).unwrap();
    assert!(!output.has_dependencies);
    assert_eq!(output.proto_files, vec![standalone_proto.join("v0.proto")]);
    assert!(fds_path.is_file());
    assert_eq!(
        output.rerun_directives,
        vec![
            format!("cargo:rerun-if-changed={}", standalone_proto.display()),
            format!(
                "cargo:rerun-if-changed={}",
                standalone_proto.join("v0.proto").display()
            ),
        ]
    );
    let include_path = output.include_file.as_ref().unwrap();
    let include_text = fs::read_to_string(include_path).unwrap();
    assert!(include_text.contains("pub mod v0 {"), "{include_text}");
    assert!(
        include_text.contains("include!(\"example.tasks.v0.rs\");"),
        "{include_text}"
    );
    #[cfg(feature = "pbjson")]
    assert!(
        include_text.contains("include!(\"example.tasks.v0.serde.rs\");"),
        "{include_text}"
    );

    // Repeating compilation with unchanged inputs preserves the include file content cleanly.
    let second = maybe_enable_pbjson(
        witforge_build::Config::new()
            .out_dir(&standalone_out)
            .emit_rerun_if_changed(false),
    )
    .compile_protos(&standalone_proto)
    .unwrap();
    assert_eq!(
        fs::read_to_string(second.include_file.as_ref().unwrap()).unwrap(),
        include_text
    );

    // 2. Contract with `deps/`.
    let deps_wit = fixture("deps/wit");
    let deps_proto = base.join("deps-proto");
    proto::plan(&deps_wit).unwrap().write(&deps_proto).unwrap();
    let deps_out = base.join("deps-out");

    let deps_cfg = maybe_enable_pbjson(
        witforge_build::Config::new()
            .out_dir(&deps_out)
            .emit_rerun_if_changed(false),
    );
    let deps_output = deps_cfg.compile_protos(&deps_proto).unwrap();
    assert!(deps_output.has_dependencies);
    assert!(deps_output.proto_files.len() >= 2);
    assert_eq!(
        deps_output.rerun_directives.len(),
        deps_output.proto_files.len() + 1
    );
    let deps_include = fs::read_to_string(deps_output.include_file.as_ref().unwrap()).unwrap();
    assert!(deps_include.contains("pub mod v0 {"), "{deps_include}");
    #[cfg(feature = "pbjson")]
    {
        assert!(deps_include.contains(".serde.rs\");"), "{deps_include}");
        // Verify indentation was preserved when splicing pbjson includes.
        for line in deps_include.lines() {
            if line.contains(".serde.rs\");") {
                assert!(line.starts_with("    "), "lost indentation: {line:?}");
            }
        }
    }

    // 3. Subdirectory discovery emits `cargo:rerun-if-changed` for every subdirectory too.
    let nested_proto = base.join("nested-proto");
    fs::create_dir_all(nested_proto.join("sub")).unwrap();
    fs::write(
        nested_proto.join("sub/item.proto"),
        "syntax = \"proto3\";\npackage ex.sub.v0;\nmessage Item { uint32 id = 1; }\n",
    )
    .unwrap();
    let nested_out = base.join("nested-out");
    let nested = witforge_build::Config::new()
        .out_dir(&nested_out)
        .no_include_file()
        .emit_rerun_if_changed(false)
        .compile_protos(&nested_proto)
        .unwrap();
    assert!(nested.include_file.is_none());
    assert!(nested.rerun_directives.contains(&format!(
        "cargo:rerun-if-changed={}",
        nested_proto.join("sub").display()
    )));

    // 4. Error paths: empty dir, invalid include_file, and symlinks (Unix).
    let empty_dir = base.join("empty");
    fs::create_dir_all(&empty_dir).unwrap();
    assert!(matches!(
        witforge_build::discover_protos(&empty_dir),
        Err(witforge_build::Error::NoProtoFiles(_))
    ));
    assert!(matches!(
        witforge_build::Config::new()
            .out_dir(&standalone_out)
            .include_file("sub/prost_include.rs")
            .emit_rerun_if_changed(false)
            .compile_protos(&standalone_proto),
        Err(witforge_build::Error::InvalidIncludeFile(_))
    ));
    #[cfg(unix)]
    {
        let root_link = base.join("link-proto");
        std::os::unix::fs::symlink(&standalone_proto, &root_link).unwrap();
        assert!(matches!(
            witforge_build::discover_protos(&root_link),
            Err(witforge_build::Error::SymlinkRefused(_))
        ));
        let child_dir = base.join("child-link-proto");
        fs::create_dir_all(&child_dir).unwrap();
        std::os::unix::fs::symlink(
            standalone_proto.join("v0.proto"),
            child_dir.join("v0.proto"),
        )
        .unwrap();
        assert!(matches!(
            witforge_build::discover_protos(&child_dir),
            Err(witforge_build::Error::SymlinkRefused(_))
        ));
    }

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn scratch_consumer_crate_builds_and_roundtrips_standalone_and_deps_via_witforge_build() {
    let base = scratch("consumer");
    let witforge_build_path = workspace_root()
        .join("crates/witforge-build")
        .display()
        .to_string()
        .replace('\\', "\\\\");
    let shared_target =
        std::env::temp_dir().join(format!("wf-build-consumer-target-{}", rustc_version_key()));

    let pbjson_enabled = cfg!(feature = "pbjson");

    // Synthetic standalone multi-major contract + uppercase & Rust-keyword package names
    // (including Edition 2024 `gen`, `%type`, `HTTP:API`, `self:super`).
    let upper_kw_wit = base.join("wit-upper-kw");
    fs::create_dir_all(upper_kw_wit.join("v0.1")).unwrap();
    fs::write(
        upper_kw_wit.join("v0.1/contract.wit"),
        "package HTTP:API@0.1.0;\ninterface api { record item { id: u32, label: string } }\n",
    )
    .unwrap();
    lock_version_dir(&upper_kw_wit.join("v0.1"));
    fs::create_dir_all(upper_kw_wit.join("v1.0")).unwrap();
    fs::write(
        upper_kw_wit.join("v1.0/contract.wit"),
        "package HTTP:API@1.0.0;\ninterface api { record item { id: u64, label: string } }\n",
    )
    .unwrap();
    lock_version_dir(&upper_kw_wit.join("v1.0"));

    let gen_kw_wit = base.join("wit-gen-kw");
    fs::create_dir_all(gen_kw_wit.join("v0.1")).unwrap();
    fs::write(
        gen_kw_wit.join("v0.1/contract.wit"),
        "package example:gen@0.1.0;\ninterface api { record item { id: u32 } }\n",
    )
    .unwrap();
    lock_version_dir(&gen_kw_wit.join("v0.1"));

    let gen_deps_wit = base.join("wit-gen-deps");
    fs::create_dir_all(gen_deps_wit.join("v0.1/deps/dep")).unwrap();
    fs::write(
        gen_deps_wit.join("v0.1/deps/dep/types.wit"),
        "package example:gen@0.1.0;\ninterface types { record item { id: u32 } }\n",
    )
    .unwrap();
    fs::write(
        gen_deps_wit.join("v0.1/contract.wit"),
        "package self:super@0.1.0;\ninterface api {\n  use example:gen/types@0.1.0.{item};\n  record box { inner: item }\n}\n",
    )
    .unwrap();
    lock_version_dir(&gen_deps_wit.join("v0.1"));

    for (label, wit_root, proto_msg_path, roundtrip_body) in [
        (
            "standalone",
            workspace_root().join("examples/evolved/wit"),
            "proto::v0::TypesTask",
            r#"
    let task = plain::v0_2::types::Task {
        id: 42,
        title: "build".to_string(),
        done: false,
        priority: Some(1),
        tags: vec!["ci".to_string()],
    };
    let wire = glue::v0::types::task::to_proto(&task);
    let encoded = ::prost::Message::encode_to_vec(&wire);
    let decoded = <proto::v0::TypesTask as ::prost::Message>::decode(encoded.as_slice()).unwrap();
    assert_eq!(glue::v0::types::task::from_proto(decoded.clone()).unwrap(), task);
"#,
        ),
        (
            "deps",
            fixture("deps/wit"),
            "proto::example::app::v0::ApiPutRequest",
            r#"
    let item = plain::witforge_deps::example_shared::types::Item {
        id: 7,
        label: "dep".to_string(),
    };
    let wire = glue::v0::api::functions::put::to_request(&item);
    let encoded = ::prost::Message::encode_to_vec(&wire);
    let decoded = <proto::example::app::v0::ApiPutRequest as ::prost::Message>::decode(
        encoded.as_slice(),
    )
    .unwrap();
    assert_eq!(glue::v0::api::functions::put::from_request(decoded.clone()).unwrap(), item);
"#,
        ),
        (
            "upper-multi-major",
            upper_kw_wit,
            "proto::v1::ApiItem",
            r#"
    let item0 = plain::v0_1::api::Item { id: 3, label: "v0".to_string() };
    let w0 = glue::v0::api::item::to_proto(&item0);
    assert_eq!(glue::v0::api::item::from_proto(w0).unwrap(), item0);
    let item1 = plain::v1_0::api::Item { id: 9, label: "v1".to_string() };
    let wire = glue::v1::api::item::to_proto(&item1);
    let encoded = ::prost::Message::encode_to_vec(&wire);
    let decoded = <proto::v1::ApiItem as ::prost::Message>::decode(encoded.as_slice()).unwrap();
    assert_eq!(glue::v1::api::item::from_proto(decoded.clone()).unwrap(), item1);
"#,
        ),
        (
            "gen-keyword",
            gen_kw_wit,
            "proto::v0::ApiItem",
            r"
    let item = plain::v0_1::api::Item { id: 11 };
    let wire = glue::v0::api::item::to_proto(&item);
    let encoded = ::prost::Message::encode_to_vec(&wire);
    let decoded = <proto::v0::ApiItem as ::prost::Message>::decode(encoded.as_slice()).unwrap();
    assert_eq!(glue::v0::api::item::from_proto(decoded.clone()).unwrap(), item);
",
        ),
        (
            "gen-deps-keyword",
            gen_deps_wit,
            "proto::self_::super_::v0::ApiBox",
            r"
    let boxed = plain::v0_1::api::Box {
        inner: plain::witforge_deps::example_gen::types::Item { id: 19 },
    };
    let wire = glue::v0::api::r#box::to_proto(&boxed);
    let encoded = ::prost::Message::encode_to_vec(&wire);
    let decoded =
        <proto::self_::super_::v0::ApiBox as ::prost::Message>::decode(encoded.as_slice()).unwrap();
    assert_eq!(glue::v0::api::r#box::from_proto(decoded.clone()).unwrap(), boxed);
",
        ),
    ] {
        let krate = base.join(label);
        fs::create_dir_all(krate.join("src")).unwrap();
        rust::plan(&wit_root)
            .unwrap()
            .write(&krate.join("src/plain"))
            .unwrap();
        proto::plan(&wit_root)
            .unwrap()
            .write(&krate.join("proto"))
            .unwrap();
        glue::plan(&wit_root, "crate::plain", "crate::proto")
            .unwrap()
            .write(&krate.join("src/glue"))
            .unwrap();
        fs::copy(
            workspace_root().join("Cargo.lock"),
            krate.join("Cargo.lock"),
        )
        .unwrap();

        let (extra_deps, build_dep_line, build_rs_body, pbjson_check) = if pbjson_enabled {
            (
                "pbjson = \"0.9\"\nserde = \"1\"\nserde_json = \"1\"\n",
                format!(
                    "witforge-build = {{ path = \"{witforge_build_path}\", features = [\"pbjson\"] }}"
                ),
                "    witforge_build::Config::new()\n        .with_pbjson_builder(|b| { b.ignore_unknown_fields(); })\n        .compile_protos(\"proto\")?;\n",
                format!(
                    "    let json = ::serde_json::to_string(&decoded).unwrap();\n\
                     let mut obj: ::serde_json::Value = ::serde_json::from_str(&json).unwrap();\n\
                     obj.as_object_mut().unwrap().insert(\"futureField\".to_string(), ::serde_json::json!(123));\n\
                     let back_json: {proto_msg_path} = ::serde_json::from_value(obj).unwrap();\n\
                     assert_eq!(back_json, decoded);\n"
                ),
            )
        } else {
            (
                "",
                format!("witforge-build = {{ path = \"{witforge_build_path}\" }}"),
                "    witforge_build::compile_protos(\"proto\")?;\n",
                String::new(),
            )
        };

        fs::write(
            krate.join("Cargo.toml"),
            format!(
                "[package]\n\
                 name = \"wf-build-{label}\"\n\
                 version = \"0.1.0\"\n\
                 edition = \"2024\"\n\
                 \n\
                 [dependencies]\n\
                 prost = \"0.14\"\n\
                 {extra_deps}\n\
                 [build-dependencies]\n\
                 {build_dep_line}\n"
            ),
        )
        .unwrap();
        fs::write(
            krate.join("build.rs"),
            format!(
                "fn main() -> Result<(), Box<dyn std::error::Error>> {{\n\
                 {build_rs_body}\
                     Ok(())\n\
                 }}\n"
            ),
        )
        .unwrap();
        fs::write(
            krate.join("src/lib.rs"),
            format!(
                "#![deny(missing_docs)]\n\
                 \n\
                 //! Consumer crate using `witforge-build`.\n\
                 \n\
                 /// Plain Rust contract types.\n\
                 #[path = \"plain/mod.rs\"]\n\
                 pub mod plain;\n\
                 \n\
                 /// Prost protobuf types compiled by `witforge-build`.\n\
                 #[allow(clippy::all, missing_docs)]\n\
                 pub mod proto {{\n\
                     include!(concat!(env!(\"OUT_DIR\"), \"/prost_include.rs\"));\n\
                 }}\n\
                 \n\
                 /// Plain <-> prost glue.\n\
                 #[path = \"glue/mod.rs\"]\n\
                 pub mod glue;\n\
                 \n\
                 #[cfg(test)]\n\
                 mod tests {{\n\
                     use super::{{glue, plain, proto}};\n\
                     #[test]\n\
                     fn roundtrips() {{\n\
                 {roundtrip_body}\
                 {pbjson_check}\
                     }}\n\
                 }}\n"
            ),
        )
        .unwrap();

        let status = Command::new(env!("CARGO"))
            .args(["test", "--offline", "--manifest-path"])
            .arg(krate.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &shared_target)
            .env("CARGO_TERM_COLOR", "never")
            .env_remove("RUSTFLAGS")
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "consumer `{label}` (pbjson={pbjson_enabled}) failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
    }

    // Also verify a `#![no_std]` + `alloc` consumer builds cleanly with `witforge-build`.
    if !pbjson_enabled {
        let nostd_krate = base.join("nostd");
        fs::create_dir_all(nostd_krate.join("src")).unwrap();
        let wit_root = workspace_root().join("examples/evolved/wit");
        let mut rust_opts = rust::RustOptions::new();
        rust_opts.no_std = true;
        rust::plan_with_options(&wit_root, None, &rust_opts)
            .unwrap()
            .write(&nostd_krate.join("src/plain"))
            .unwrap();
        proto::plan(&wit_root)
            .unwrap()
            .write(&nostd_krate.join("proto"))
            .unwrap();
        let mut glue_opts = glue::GlueOptions::new();
        glue_opts.no_std = true;
        glue::plan_with_options(&wit_root, "crate::plain", "crate::proto", None, &glue_opts)
            .unwrap()
            .write(&nostd_krate.join("src/glue"))
            .unwrap();
        fs::copy(
            workspace_root().join("Cargo.lock"),
            nostd_krate.join("Cargo.lock"),
        )
        .unwrap();
        fs::write(
            nostd_krate.join("Cargo.toml"),
            format!(
                "[package]\n\
                 name = \"wf-build-nostd\"\n\
                 version = \"0.1.0\"\n\
                 edition = \"2024\"\n\
                 \n\
                 [dependencies]\n\
                 prost = {{ version = \"0.14\", default-features = false, features = [\"derive\"] }}\n\
                 \n\
                 [build-dependencies]\n\
                 witforge-build = {{ path = \"{witforge_build_path}\" }}\n"
            ),
        )
        .unwrap();
        fs::write(
            nostd_krate.join("build.rs"),
            "fn main() -> Result<(), Box<dyn std::error::Error>> {\n\
                 witforge_build::compile_protos(\"proto\")?;\n\
                 Ok(())\n\
             }\n",
        )
        .unwrap();
        fs::write(
            nostd_krate.join("src/lib.rs"),
            "#![no_std]\n\
             #![deny(missing_docs)]\n\
             //! no_std consumer crate using `witforge-build`.\n\
             extern crate alloc;\n\
             /// Plain types.\n\
             #[path = \"plain/mod.rs\"]\n\
             pub mod plain;\n\
             /// Proto types.\n\
             #[allow(clippy::all, missing_docs)]\n\
             pub mod proto {\n\
                 include!(concat!(env!(\"OUT_DIR\"), \"/prost_include.rs\"));\n\
             }\n\
             /// Glue.\n\
             #[path = \"glue/mod.rs\"]\n\
             pub mod glue;\n",
        )
        .unwrap();
        let status = Command::new(env!("CARGO"))
            .args(["check", "--lib", "--offline", "--manifest-path"])
            .arg(nostd_krate.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &shared_target)
            .env("CARGO_TERM_COLOR", "never")
            .env_remove("RUSTFLAGS")
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "no_std consumer failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
    }

    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn second_wave_audit_include_file_and_pbjson_incremental_cleanup() {
    let base =
        std::env::temp_dir().join(format!("witforge-build-second-wave-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let proto_dir = base.join("proto");
    let out_dir = base.join("out");
    fs::create_dir_all(&proto_dir).unwrap();
    fs::create_dir_all(&out_dir).unwrap();

    fs::write(
        proto_dir.join("v0.proto"),
        "syntax = \"proto3\";\npackage ex.demo.v0;\nmessage Item { uint64 id = 1; }\n",
    )
    .unwrap();

    // 1. Trailing path separator in `include_file` is refused.
    let err = witforge_build::Config::new()
        .out_dir(&out_dir)
        .include_file("prost_include.rs/")
        .compile_protos(&proto_dir)
        .unwrap_err();
    assert!(
        matches!(err, witforge_build::Error::InvalidIncludeFile(_)),
        "expected InvalidIncludeFile for trailing slash, got {err:?}"
    );

    // 2. With `pbjson`, excluding a package on an incremental rebuild removes its stale `.serde.rs`
    //    and omits it from `prost_include.rs` instead of re-including the old file from disk.
    #[cfg(feature = "pbjson")]
    {
        let out_first = witforge_build::Config::new()
            .out_dir(&out_dir)
            .pbjson(true)
            .compile_protos(&proto_dir)
            .unwrap();
        let include_path = out_first.include_file.as_ref().unwrap();
        let serde_path = out_dir.join("ex.demo.v0.serde.rs");
        assert!(serde_path.is_file(), "expected serde output on first build");
        assert!(
            fs::read_to_string(include_path)
                .unwrap()
                .contains("ex.demo.v0.serde.rs")
        );

        let mut excluded_cfg = witforge_build::Config::new().out_dir(&out_dir).pbjson(true);
        excluded_cfg.pbjson_builder().exclude([".ex.demo.v0"]);
        excluded_cfg.compile_protos(&proto_dir).unwrap();
        assert!(
            !serde_path.exists(),
            "stale .serde.rs must be removed when package is excluded"
        );
        assert!(
            !fs::read_to_string(include_path)
                .unwrap()
                .contains("ex.demo.v0.serde.rs"),
            "prost_include.rs must not include excluded stale .serde.rs"
        );
    }

    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn third_wave_audit_empty_proto_pbjson_prefix_and_backslash_guards() {
    let base =
        std::env::temp_dir().join(format!("witforge-build-third-wave-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let proto_dir = base.join("proto");
    let out_dir = base.join("out");
    fs::create_dir_all(&proto_dir).unwrap();
    fs::create_dir_all(&out_dir).unwrap();

    // 1. Backslash in `include_file` is refused on every platform (including Unix).
    fs::write(
        proto_dir.join("v0.proto"),
        "syntax = \"proto3\";\npackage ex.empty.v0;\n",
    )
    .unwrap();
    let err = witforge_build::Config::new()
        .out_dir(&out_dir)
        .include_file(r"sub\prost_include.rs")
        .compile_protos(&proto_dir)
        .unwrap_err();
    assert!(
        matches!(err, witforge_build::Error::InvalidIncludeFile(_)),
        "expected InvalidIncludeFile for backslash, got {err:?}"
    );

    // 2. Standalone `.proto` with no messages/enums produces `pub mod v0 {}` without a dangling
    //    `include!("ex.empty.v0.rs");` and removes any stale `.rs` from an earlier build.
    fs::write(out_dir.join("ex.empty.v0.rs"), "// stale\n").unwrap();
    let out = witforge_build::Config::new()
        .out_dir(&out_dir)
        .compile_protos(&proto_dir)
        .unwrap();
    let include_text = fs::read_to_string(out.include_file.as_ref().unwrap()).unwrap();
    assert!(
        include_text.contains("pub mod v0 {}"),
        "expected empty v0 module without dangling include: {include_text}"
    );
    assert!(!include_text.contains("ex.empty.v0.rs"));
    assert!(!out_dir.join("ex.empty.v0.rs").exists());

    // 3. Under `pbjson`:
    //    a) `package google.tasks.v0;` importing `google/protobuf/timestamp.proto` must NOT emit
    //       a stray `google.protobuf.serde.rs`.
    //    b) Package-less `.proto` returns `Err(Error::Pbjson(_))` instead of panicking.
    #[cfg(feature = "pbjson")]
    {
        let wkt_proto = base.join("wkt-proto");
        let wkt_out = base.join("wkt-out");
        fs::create_dir_all(&wkt_proto).unwrap();
        fs::create_dir_all(&wkt_out).unwrap();
        fs::write(
            wkt_proto.join("v0.proto"),
            "syntax = \"proto3\";\n\
             package google.tasks.v0;\n\
             import \"google/protobuf/timestamp.proto\";\n\
             message Task { uint64 id = 1; google.protobuf.Timestamp at = 2; }\n",
        )
        .unwrap();
        witforge_build::Config::new()
            .out_dir(&wkt_out)
            .pbjson(true)
            .compile_protos(&wkt_proto)
            .unwrap();
        assert!(wkt_out.join("google.tasks.v0.serde.rs").is_file());
        assert!(
            !wkt_out.join("google.protobuf.serde.rs").exists(),
            "imported well-known proto must not leak into pbjson output"
        );

        let nopkg_proto = base.join("nopkg-proto");
        let nopkg_out = base.join("nopkg-out");
        fs::create_dir_all(&nopkg_proto).unwrap();
        fs::create_dir_all(&nopkg_out).unwrap();
        fs::write(
            nopkg_proto.join("v0.proto"),
            "syntax = \"proto3\";\nmessage Bare { uint64 id = 1; }\n",
        )
        .unwrap();
        let err = witforge_build::Config::new()
            .out_dir(&nopkg_out)
            .pbjson(true)
            .compile_protos(&nopkg_proto)
            .unwrap_err();
        assert!(
            matches!(err, witforge_build::Error::Pbjson(_)),
            "expected Error::Pbjson for package-less proto, got {err:?}"
        );
    }

    fs::remove_dir_all(&base).unwrap();
}
