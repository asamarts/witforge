//! Full dependency glue compilation and round-trip.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::emit::{glue, proto, rust};
use witforge::load;
use witforge::model;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deps-full/wit")
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

#[allow(clippy::too_many_lines, clippy::format_push_string)]
#[test]
fn full_dependency_glue_compiles_and_roundtrips() {
    let root = fixture();
    let contract = load(&root.join("v0.1")).unwrap();
    let model = model::build(&contract).unwrap();
    assert_eq!(model.dependencies.len(), 3);
    let out = std::env::temp_dir().join(format!("witforge-deps-glue-full-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    for path in [
        "proto/example/app",
        "proto/example/shared",
        "proto/example/other",
        "proto/my_ns/my_dep",
    ] {
        fs::create_dir_all(out.join(path)).unwrap();
    }

    rust::plan(&root)
        .unwrap()
        .write(&out.join("plain"))
        .unwrap();
    glue::plan(&root, "crate::plain", "crate::proto")
        .unwrap()
        .write(&out.join("glue"))
        .unwrap();
    proto::plan(&root)
        .unwrap()
        .write(&out.join("proto-work"))
        .unwrap();

    let mut proto_files = Vec::new();
    for entry in fs::read_dir(out.join("proto-work")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "proto") {
            proto_files.push(path);
        }
    }
    let descriptors = protox::compile(proto_files, [out.join("proto-work")]).unwrap();
    let generated = out.join("proto-generated");
    fs::create_dir_all(&generated).unwrap();
    prost_build::Config::new()
        .out_dir(&generated)
        .compile_fds(descriptors)
        .unwrap();

    for (name, target) in [
        ("example.app.v0.rs", "proto/example/app/v0.rs"),
        ("example.shared.v0.rs", "proto/example/shared/v0.rs"),
        ("example.other.v0.rs", "proto/example/other/v0.rs"),
        ("my_ns.my_dep.v0.rs", "proto/my_ns/my_dep/v0.rs"),
    ] {
        let source = generated.join(name);
        assert!(source.is_file(), "missing generated `{}`", source.display());
        fs::copy(source, out.join(target)).unwrap();
    }
    fs::write(
        out.join("proto/mod.rs"),
        "pub mod example {\n\
         pub mod app { pub mod v0 { include!(\"example/app/v0.rs\"); } }\n\
         pub mod other { pub mod v0 { include!(\"example/other/v0.rs\"); } }\n\
         pub mod shared { pub mod v0 { include!(\"example/shared/v0.rs\"); } }\n\
         }\n\
         pub mod my_ns { pub mod my_dep { pub mod v0 { include!(\"my_ns/my_dep/v0.rs\"); } } }\n",
    )
    .unwrap();

    let mut main = String::new();
    main.push_str(&format!(
        "#[path = {:?}] mod plain;\n#[path = {:?}] mod glue;\nmod proto;\n\n",
        out.join("plain/mod.rs"),
        out.join("glue/mod.rs")
    ));
    main.push_str(
        r#"
fn main() {
    let thing = plain::witforge_deps::example_shared::types::Thing { id: 7 };
    let other = plain::witforge_deps::example_other::types::OtherThing { id: 8 };
    let value = plain::v0_1::api::Wrapper {
        other,
        alias: thing.clone(),
        items: vec![1, 2, 3],
        opt: Some(thing.clone()),
        choice: plain::witforge_deps::example_shared::types::Choice::One,
    };
    let encoded = glue::v0::api::wrapper::to_proto(&value);
    let decoded = glue::v0::api::wrapper::from_proto(encoded).unwrap();
    assert_eq!(value, decoded);

    let numbers = vec![1_u64, 2, 3];
    let encoded = glue::v0::api::numbers::to_proto(&numbers);
    let decoded = glue::v0::api::numbers::from_proto(encoded).unwrap();
    assert_eq!(numbers, decoded);

    let list_opt = vec![Some(thing.clone()), None];
    let encoded = glue::v0::api::list_of_opt::to_proto(&list_opt);
    let decoded = glue::v0::api::list_of_opt::from_proto(encoded).unwrap();
    assert_eq!(list_opt, decoded);

    let option_opt = Some(Some(thing.clone()));
    let encoded = glue::v0::api::option_of_opt::to_proto(&option_opt);
    let decoded = glue::v0::api::option_of_opt::from_proto(encoded).unwrap();
    assert_eq!(option_opt, decoded);

    let list_outcome = vec![Ok(thing.clone()), Err("bad".to_string())];
    let encoded = glue::v0::api::list_of_outcome::to_proto(&list_outcome);
    let decoded = glue::v0::api::list_of_outcome::from_proto(encoded).unwrap();
    assert_eq!(list_outcome, decoded);

    let option_numbers = Some(numbers.clone());
    let encoded = glue::v0::api::option_of_numbers::to_proto(&option_numbers);
    let decoded = glue::v0::api::option_of_numbers::from_proto(encoded).unwrap();
    assert_eq!(option_numbers, decoded);
}
"#,
    );
    fs::write(out.join("main.rs"), main).unwrap();

    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let executable = out.join(format!("deps-glue-full{}", std::env::consts::EXE_SUFFIX));
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
                "full dependency glue program failed:\n{}",
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
        panic!("full dependency glue did not compile:\n{stderr}");
    }
    panic!("no usable prost rlib combination: {last_error:?}");
}
