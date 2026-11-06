//! Dependency type and world glue round-trip.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::emit::{glue, proto, rust};
use witforge::load;
use witforge::model;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deps-world/wit")
}

fn rlibs(deps: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(deps)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_string_lossy();
            (name.starts_with(&format!("lib{prefix}-")) && name.ends_with(".rlib")).then_some(path)
        })
        .collect();
    paths.sort();
    paths
}

fn generated_file(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    assert!(path.is_file(), "missing generated `{}`", path.display());
    path
}

#[allow(clippy::too_many_lines, clippy::format_push_string)]
#[test]
fn dependency_glue_roundtrips_a_foreign_value() {
    let root = fixture();
    let contract = load(&root.join("v0.1")).unwrap();
    let model = model::build(&contract).unwrap();
    assert_eq!(
        model.worlds[0].imports[0].qualified_name(),
        "example:shared/types"
    );
    let out = std::env::temp_dir().join(format!("witforge-deps-glue-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(out.join("proto/example/app")).unwrap();
    fs::create_dir_all(out.join("proto/example/shared")).unwrap();
    fs::create_dir_all(out.join("proto/example/other")).unwrap();

    rust::plan(&root)
        .unwrap()
        .write(&out.join("plain"))
        .unwrap();
    let glue_files = glue::plan(&root, "crate::plain", "crate::proto").unwrap();
    glue_files.write(&out.join("glue")).unwrap();
    assert!(glue_files.check(&out.join("glue")).unwrap().is_empty());
    assert!(glue_files.files().contains_key("witforge_deps.rs"));
    proto::plan(&root)
        .unwrap()
        .write(&out.join("proto-work"))
        .unwrap();

    let proto_files = [
        out.join("proto-work/v0.proto"),
        out.join("proto-work/example.shared.v0.proto"),
        out.join("proto-work/example.other.v0.proto"),
    ];
    let descriptors = protox::compile(proto_files, [out.join("proto-work")]).unwrap();
    let generated = out.join("proto-generated");
    fs::create_dir_all(&generated).unwrap();
    prost_build::Config::new()
        .out_dir(&generated)
        .compile_fds(descriptors)
        .unwrap();

    fs::copy(
        generated_file(&generated, "example.app.v0.rs"),
        out.join("proto/example/app/v0.rs"),
    )
    .unwrap();
    fs::copy(
        generated_file(&generated, "example.shared.v0.rs"),
        out.join("proto/example/shared/v0.rs"),
    )
    .unwrap();
    fs::copy(
        generated_file(&generated, "example.other.v0.rs"),
        out.join("proto/example/other/v0.rs"),
    )
    .unwrap();
    fs::write(
        out.join("proto/mod.rs"),
        "pub mod example {\n\
         pub mod app { pub mod v0 { include!(\"example/app/v0.rs\"); } }\n\
         pub mod other { pub mod v0 { include!(\"example/other/v0.rs\"); } }\n\
         pub mod shared { pub mod v0 { include!(\"example/shared/v0.rs\"); } }\n\
         }\n",
    )
    .unwrap();

    let mut main = String::new();
    main.push_str(&format!(
        "#[path = {:?}] mod plain;\n#[path = {:?}] mod glue;\nmod proto;\n\n",
        out.join("plain/mod.rs"),
        out.join("glue/mod.rs")
    ));
    main.push_str(
        r"
fn main() {
    let value = plain::v0_1::api::Box {
        value: plain::witforge_deps::example_shared::types::Thing { id: 7 },
    };
    let encoded = glue::v0::api::r#box::to_proto(&value);
    let decoded = glue::v0::api::r#box::from_proto(encoded).unwrap();
    assert_eq!(value, decoded);

    let direct = plain::witforge_deps::example_shared::types::Thing { id: 9 };
    let encoded = glue::witforge_deps::example_shared::types::thing::to_proto(&direct);
    let decoded = glue::witforge_deps::example_shared::types::thing::from_proto(encoded).unwrap();
    assert_eq!(direct, decoded);

    let request = glue::witforge_deps::example_shared::types::functions::ping::to_request(&direct);
    let round = glue::witforge_deps::example_shared::types::functions::ping::from_request(request).unwrap();
    assert_eq!(direct, round);
}
",
    );
    fs::write(out.join("main.rs"), main).unwrap();

    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let executable = out.join(format!("deps-glue-run{}", std::env::consts::EXE_SUFFIX));
    let mut last_error = None;
    for prost in rlibs(&deps, "prost") {
        let output = Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-type=bin",
                "-L",
                &format!("dependency={}", deps.display()),
                "--extern",
                &format!("prost={}", prost.display()),
                "-o",
            ])
            .arg(&executable)
            .arg(out.join("main.rs"))
            .output()
            .unwrap();
        if output.status.success() {
            let run = Command::new(&executable).output().unwrap();
            assert!(
                run.status.success(),
                "dependency glue program failed:\n{}",
                String::from_utf8_lossy(&run.stderr)
            );
            fs::remove_dir_all(&out).unwrap();
            return;
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("E0514") {
            last_error = Some(stderr.into_owned());
            continue;
        }
        panic!("dependency glue did not compile:\n{stderr}");
    }
    panic!("no usable prost rlib combination: {last_error:?}");
}
