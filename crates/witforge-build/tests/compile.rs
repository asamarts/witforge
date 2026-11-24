//! Integration tests verifying `witforge-build` on standalone and `deps/` generated `.proto`
//! trees (both default and with the optional `pbjson` feature).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::emit::{glue, proto, rust};

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

#[cfg(feature = "pbjson")]
fn maybe_enable_pbjson(cfg: witforge_build::Config) -> witforge_build::Config {
    cfg.pbjson(true)
}

#[cfg(not(feature = "pbjson"))]
fn maybe_enable_pbjson(cfg: witforge_build::Config) -> witforge_build::Config {
    cfg
}

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
    assert!(deps_include.contains(".serde.rs\");"), "{deps_include}");

    // 3. Empty directory returns `Error::NoProtoFiles`.
    let empty_dir = base.join("empty");
    fs::create_dir_all(&empty_dir).unwrap();
    assert!(matches!(
        witforge_build::discover_protos(&empty_dir),
        Err(witforge_build::Error::NoProtoFiles(_))
    ));

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
    let shared_target = std::env::temp_dir().join("wf-build-consumer-target");

    let pbjson_enabled = cfg!(feature = "pbjson");

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
                "    witforge_build::Config::new().pbjson(true).compile_protos(\"proto\")?;\n",
                format!(
                    "    let json = ::serde_json::to_string(&decoded).unwrap();\n\
                     let back_json: {proto_msg_path} = ::serde_json::from_str(&json).unwrap();\n\
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

    fs::remove_dir_all(&base).unwrap();
}
