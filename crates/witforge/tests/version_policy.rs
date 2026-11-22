//! Version-shape and memberless-definition policy tests.

use std::fs;
use std::path::{Path, PathBuf};

use witforge::emit::rust;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
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

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "witforge-version-policy-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    copy_tree(&fixture("versions/wit"), &path.join("wit"));
    path
}

#[test]
fn docs_only_changes_in_a_draft_leave_wire_and_api_shape_alone() {
    let root = temp("docs");
    let contract = root.join("wit/v0.2/contract.wit");
    let text = fs::read_to_string(&contract).unwrap();
    let changed = text.replace(
        "interface types {",
        "/// Documentation added in a draft minor.\ninterface types {",
    );
    assert_ne!(text, changed);
    fs::write(&contract, changed).unwrap();
    let lock_before = fs::read(root.join("wit/v0.2/wire.lock")).unwrap();

    let files = rust::plan(&root.join("wit")).unwrap();
    let version = files.files().get("v0_2.rs").unwrap();
    assert!(
        version.contains("/// Documentation added in a draft minor."),
        "{version}"
    );
    assert_eq!(
        fs::read(root.join("wit/v0.2/wire.lock")).unwrap(),
        lock_before
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_memberless_addition_in_a_new_minor_has_no_wire_entry() {
    let root = temp("memberless");
    let contract = root.join("wit/v0.2/contract.wit");
    let text = fs::read_to_string(&contract).unwrap();
    let changed = text.replacen(
        "interface types {",
        "interface types {\n    /// A memberless alias added in a draft minor.\n    type generated-alias = u32;",
        1,
    );
    assert_ne!(text, changed);
    fs::write(&contract, changed).unwrap();
    let lock_before = fs::read(root.join("wit/v0.2/wire.lock")).unwrap();

    let files = rust::plan(&root.join("wit")).unwrap();
    let version = files.files().get("v0_2.rs").unwrap();
    assert!(
        version.contains("pub type GeneratedAlias = u32;"),
        "{version}"
    );
    assert_eq!(
        fs::read(root.join("wit/v0.2/wire.lock")).unwrap(),
        lock_before
    );
    assert!(
        !String::from_utf8_lossy(&lock_before).contains("generated-alias"),
        "a memberless definition must not acquire a wire entry"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn near_miss_version_directories_and_files_are_refused() {
    let root = temp("near-miss");
    let wit = root.join("wit");

    // Unrelated entries not starting with v/V + digit remain ignored.
    fs::create_dir_all(wit.join("vendor")).unwrap();
    fs::write(wit.join("versions.md"), "# notes\n").unwrap();
    fs::write(wit.join(".hidden"), "").unwrap();
    fs::write(wit.join(".v0.3"), "").unwrap();
    rust::plan(&wit).unwrap();

    // Use `V0.3` (not `V0.2`, which collides with existing `v0.2` on case-insensitive APFS/NTFS);
    // Win32 strips trailing dots from created entries (`v1.` becomes `v1`), so test `v1.` off Windows.
    let near_misses: &[&str] = &[
        "v0.2.0",
        "V0.3",
        "v0.2-rc1",
        "v00.2",
        "v0.02",
        "v1",
        "v1.x",
        #[cfg(not(windows))]
        "v1.",
        "v99999999999999999999999999.0",
        "v0.99999999999999999999999999",
    ];
    for &name in near_misses {
        // As a directory:
        let dir_path = wit.join(name);
        fs::create_dir_all(&dir_path).unwrap();
        let err = format!("{:#}", rust::plan(&wit).unwrap_err());
        assert!(err.contains(name), "dir `{name}`: {err}");
        fs::remove_dir_all(&dir_path).unwrap();

        // As a regular file:
        let file_path = wit.join(name);
        fs::write(&file_path, "").unwrap();
        let err = format!("{:#}", rust::plan(&wit).unwrap_err());
        assert!(err.contains(name), "file `{name}`: {err}");
        fs::remove_file(&file_path).unwrap();
    }

    // Even a syntactically canonical version name that is a regular file is refused.
    let canonical_file = wit.join("v0.3");
    fs::write(&canonical_file, "").unwrap();
    let err = format!("{:#}", rust::plan(&wit).unwrap_err());
    assert!(
        err.contains("v0.3") && err.contains("not a directory"),
        "{err}"
    );
    fs::remove_file(&canonical_file).unwrap();

    // Missing entry in `released` stays an error.
    fs::write(wit.join("released"), "v0.1\nv0.9\n").unwrap();
    let err = format!("{:#}", rust::plan(&wit).unwrap_err());
    assert!(
        err.contains("v0.9") && err.contains("does not exist"),
        "{err}"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn released_manifest_accepts_bom_crlf_and_whitespace_and_wit_bom_is_handled() {
    let root = temp("bom");
    let wit = root.join("wit");

    // UTF-8 BOM + CRLF + trailing spaces + blank line + comment: accepted.
    fs::write(
        wit.join("released"),
        "\u{feff}# released versions\r\n\r\n  v0.1  \r\n",
    )
    .unwrap();
    let manifest = witforge::ReleasedManifest::load(&wit.join("released")).unwrap();
    assert_eq!(manifest.directories(), ["v0.1"]);
    rust::plan(&wit).unwrap();

    // Duplicate entry after BOM is still refused.
    fs::write(wit.join("released"), "\u{feff}v0.1\r\nv0.1\r\n").unwrap();
    let dup_err = format!(
        "{:#}",
        witforge::ReleasedManifest::load(&wit.join("released")).unwrap_err()
    );
    assert!(dup_err.contains("duplicate"), "{dup_err}");

    // Test what `wit-parser` does with a leading UTF-8 BOM in a `.wit` file: either it accepts
    // the file cleanly or our loader names the byte-order mark in the error.
    let bom_dir = root.join("bom-wit");
    fs::create_dir_all(&bom_dir).unwrap();
    fs::write(
        bom_dir.join("contract.wit"),
        "\u{feff}package ex:bom@0.1.0;\ninterface types { record r { id: u32 } }\n",
    )
    .unwrap();
    if let Err(err) = witforge::load(&bom_dir) {
        let msg = format!("{err:#}");
        assert!(msg.contains("byte-order mark"), "{msg}");
    }

    fs::remove_dir_all(&root).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn version_retirement_drops_generated_outputs_and_enforces_oldest_prefix() {
    use witforge::emit::{glue, json_schema, proto};
    use witforge::lock::WireLock;
    use witforge::{ReleasedManifest, is_contract_error, load, model};

    let root = temp("retirement");
    let wit = root.join("wit");
    let v10 = wit.join("v1.0");
    fs::create_dir_all(&v10).unwrap();
    fs::write(
        v10.join("contract.wit"),
        "package example:evolve@1.0.0;\n\
         interface types {\n\
           record item { id: string }\n\
         }\n",
    )
    .unwrap();
    let v10_model = model::build(&load(&v10).unwrap()).unwrap();
    let mut v10_lock = WireLock::default();
    v10_lock.reconcile(&v10_model.members().unwrap()).unwrap();
    fs::write(v10.join("wire.lock"), v10_lock.render()).unwrap();

    // Mark v0.1 and v0.2 live first; generate all four surfaces.
    fs::write(wit.join("released"), "v0.1\nv0.2\n").unwrap();
    let out_rust = root.join("rust");
    let out_proto = root.join("proto");
    let out_schema = root.join("schema");
    let out_glue = root.join("glue");
    rust::generate(&wit, &out_rust).unwrap();
    proto::generate(&wit, &out_proto).unwrap();
    json_schema::generate(&wit, &out_schema).unwrap();
    glue::generate(&wit, &out_glue, "crate::plain", "crate::proto").unwrap();

    assert!(out_rust.join("v0_1.rs").is_file());
    assert!(out_rust.join("v0_2.rs").is_file());
    assert!(out_rust.join("v1_0.rs").is_file());
    assert!(out_rust.join("convert.rs").is_file());
    let v0_proto_before = fs::read_to_string(out_proto.join("v0.proto")).unwrap();
    assert!(out_proto.join("v1.proto").is_file());
    let v0_glue_before = fs::read_to_string(out_glue.join("v0.rs")).unwrap();
    assert!(out_glue.join("v1.rs").is_file());
    assert!(out_schema.join("v0_1.schema.json").is_file());
    assert!(out_schema.join("v0_2.schema.json").is_file());
    assert!(out_schema.join("v1_0.schema.json").is_file());

    // Retire the oldest version (`v0.1`): drops v0_1.rs, v0_1.schema.json, and v0_1 <-> v0_2
    // adapters, while leaving v0.proto and glue/v0.rs byte-identical (still built from v0.2).
    fs::write(wit.join("released"), "v0.1 retired\nv0.2\n").unwrap();
    let manifest = ReleasedManifest::load(&wit.join("released")).unwrap();
    assert_eq!(manifest.directories(), ["v0.1", "v0.2"]);
    assert!(manifest.contains("v0.1"));
    assert!(manifest.is_retired("v0.1"));
    assert!(!manifest.is_retired("v0.2"));
    assert_eq!(manifest.retired_directories(), ["v0.1"]);
    assert_eq!(manifest.live_directories(), ["v0.2"]);

    rust::generate(&wit, &out_rust).unwrap();
    proto::generate(&wit, &out_proto).unwrap();
    json_schema::generate(&wit, &out_schema).unwrap();
    glue::generate(&wit, &out_glue, "crate::plain", "crate::proto").unwrap();

    assert!(
        !out_rust.join("v0_1.rs").exists(),
        "v0_1.rs should be pruned"
    );
    assert!(out_rust.join("v0_2.rs").is_file());
    assert!(out_rust.join("v1_0.rs").is_file());
    let convert_src = fs::read_to_string(out_rust.join("convert.rs")).unwrap();
    assert!(!convert_src.contains("v0_1_to_v0_2"), "{convert_src}");
    assert!(convert_src.contains("v0_2_to_v1_0"), "{convert_src}");
    assert_eq!(
        fs::read_to_string(out_proto.join("v0.proto")).unwrap(),
        v0_proto_before,
        "retiring an older minor must not change the major's proto file"
    );
    assert_eq!(
        fs::read_to_string(out_glue.join("v0.rs")).unwrap(),
        v0_glue_before,
        "retiring an older minor must not change the major's glue file"
    );
    assert!(!out_schema.join("v0_1.schema.json").exists());
    assert!(out_schema.join("v0_2.schema.json").is_file());
    assert!(out_schema.join("v1_0.schema.json").is_file());

    // Retire every version of major 0 (`v0.1` and `v0.2`): drops v0_2, convert.rs, v0.proto, and
    // glue/v0.rs, leaving only major 1.
    fs::write(wit.join("released"), "v0.1 retired\nv0.2 retired\n").unwrap();
    rust::generate(&wit, &out_rust).unwrap();
    proto::generate(&wit, &out_proto).unwrap();
    json_schema::generate(&wit, &out_schema).unwrap();
    glue::generate(&wit, &out_glue, "crate::plain", "crate::proto").unwrap();

    assert!(!out_rust.join("v0_1.rs").exists());
    assert!(!out_rust.join("v0_2.rs").exists());
    assert!(!out_rust.join("convert.rs").exists());
    assert!(out_rust.join("v1_0.rs").is_file());
    assert!(!out_proto.join("v0.proto").exists());
    assert!(out_proto.join("v1.proto").is_file());
    assert!(!out_glue.join("v0.rs").exists());
    assert!(out_glue.join("v1.rs").is_file());
    assert!(!out_schema.join("v0_1.schema.json").exists());
    assert!(!out_schema.join("v0_2.schema.json").exists());
    assert!(out_schema.join("v1_0.schema.json").is_file());

    // Retiring a non-oldest version (`v0.2` while `v0.1` is live) is refused.
    fs::write(wit.join("released"), "v0.1\nv0.2 retired\n").unwrap();
    let err = rust::plan(&wit).unwrap_err();
    assert!(is_contract_error(&err), "{err:#}");
    assert!(
        format!("{err:#}").contains("cannot retire `v0.2`"),
        "{err:#}"
    );

    // Retiring every version is refused.
    fs::write(
        wit.join("released"),
        "v0.1 retired\nv0.2 retired\nv1.0 retired\n",
    )
    .unwrap();
    let err = rust::plan(&wit).unwrap_err();
    assert!(is_contract_error(&err), "{err:#}");
    assert!(
        format!("{err:#}").contains("every version directory"),
        "{err:#}"
    );

    // Unknown state tokens in `released` are rejected as malformed.
    fs::write(wit.join("released"), "v0.1 archived\n").unwrap();
    let err = ReleasedManifest::load(&wit.join("released")).unwrap_err();
    assert!(
        format!("{err:#}").contains("plain directory name"),
        "{err:#}"
    );

    // Retired directories are still validated (intra-major continuity into live `v0.2`).
    fs::write(wit.join("released"), "v0.1 retired\nv0.2\n").unwrap();
    let v02_lock_path = wit.join("v0.2/wire.lock");
    let broken = fs::read_to_string(&v02_lock_path)
        .unwrap()
        .replace("= 1 ", "= 99 ");
    fs::write(&v02_lock_path, broken).unwrap();
    let err = rust::plan(&wit).unwrap_err();
    assert!(is_contract_error(&err), "{err:#}");
    assert!(format!("{err:#}").contains("does not continue"), "{err:#}");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn explicit_released_manifest_drives_retirement_and_retired_intra_major_deps_must_match() {
    use witforge::emit::{glue, json_schema, proto};
    use witforge::lock::WireLock;
    use witforge::{is_contract_error, load, model};

    let root = temp("explicit-released");
    let wit = root.join("wit");
    // Default `wit/released` marks `v0.1` live; an explicit manifest marks it `retired`.
    fs::write(wit.join("released"), "v0.1\n").unwrap();
    let custom_manifest = wit.join("custom-released");
    fs::write(&custom_manifest, "v0.1 retired\nv0.2\n").unwrap();

    // Without the explicit manifest, v0.1 is live.
    assert!(rust::plan(&wit).unwrap().files().contains_key("v0_1.rs"));

    // With `plan_with_released(..., Some(&custom_manifest))`, all four generators drop `v0.1`.
    let rust_files = rust::plan_with_released(&wit, false, Some(&custom_manifest)).unwrap();
    assert!(!rust_files.files().contains_key("v0_1.rs"));
    assert!(rust_files.files().contains_key("v0_2.rs"));
    let proto_files = proto::plan_with_released(&wit, Some(&custom_manifest)).unwrap();
    assert!(proto_files.files().contains_key("v0.proto"));
    let schema_files = json_schema::plan_with_released(&wit, Some(&custom_manifest)).unwrap();
    assert!(!schema_files.files().contains_key("v0_1.schema.json"));
    assert!(schema_files.files().contains_key("v0_2.schema.json"));
    let glue_files =
        glue::plan_with_released(&wit, "crate::plain", "crate::proto", Some(&custom_manifest))
            .unwrap();
    assert!(glue_files.files().contains_key("v0.rs"));

    // Retiring an older minor of a major must still reject a dependency graph mismatch between
    // retired `v0.1` and live `v0.2` (matching `witforge check <wit_root>`).
    let dep_root = root.join("wit-dep-mismatch");
    let dep_a = "package example:shared@0.1.0;\ninterface types { record t { x: u32 } }\n";
    let dep_b = "package example:shared@0.1.0;\ninterface types { record t { x: u64 } }\n";
    for (dir, ver, dep_src) in [("v0.1", "0.1.0", dep_a), ("v0.2", "0.2.0", dep_b)] {
        let d = dep_root.join(dir);
        fs::create_dir_all(d.join("deps/shared")).unwrap();
        fs::write(d.join("deps/shared/c.wit"), dep_src).unwrap();
        fs::write(
            d.join("contract.wit"),
            format!(
                "package example:m@{ver};\n\
                 interface api {{\n\
                   use example:shared/types@0.1.0.{{t}};\n\
                   record holder {{ v: t }}\n\
                 }}\n"
            ),
        )
        .unwrap();
        let m = model::build(&load(&d).unwrap()).unwrap();
        let mut lock = WireLock::default();
        lock.reconcile(&m.members().unwrap()).unwrap();
        fs::write(d.join("wire.lock"), lock.render()).unwrap();
    }
    fs::write(dep_root.join("released"), "v0.1 retired\nv0.2\n").unwrap();
    let err = rust::plan(&dep_root).unwrap_err();
    assert!(is_contract_error(&err), "{err:#}");
    assert!(
        format!("{err:#}").contains("dependency") || format!("{err:#}").contains("example:shared"),
        "{err:#}"
    );

    fs::remove_dir_all(&root).unwrap();
}
