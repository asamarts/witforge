//! Contract IR (`witforge::inspect`) tests: versioned JSON export, determinism, and summaries.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use witforge::inspect;
use witforge::lock::WireLock;
use witforge::model;
use witforge::{SourceFile, load, load_sources};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn emit_version_json_covers_all_wit_constructs_deterministically() {
    let dir = fixture("emit/rust/all");
    let contract = load(&dir).unwrap();
    let built = model::build(&contract).unwrap();
    let members = built.members().unwrap();
    let mut lock = WireLock::default();
    lock.reconcile(&members).unwrap();

    let first = inspect::emit_version_json(&built, Some(&lock)).unwrap();
    let second = inspect::emit_version_json(&built, Some(&lock)).unwrap();
    assert_eq!(
        first, second,
        "IR output must be byte-for-byte deterministic"
    );
    assert!(first.ends_with('\n'));
    assert!(!first.contains('\r'));

    let parsed: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(parsed["version"], inspect::IR_VERSION);
    assert_eq!(parsed["kind"], "version");
    assert_eq!(parsed["contract"]["package"]["namespace"], "example");
    assert_eq!(parsed["contract"]["package"]["name"], "rust");
    assert_eq!(parsed["contract"]["package"]["version"], "0.1.0");
    assert_eq!(
        parsed["contract"]["package"]["docs"],
        "Rust mapping fixture."
    );

    let interfaces = parsed["contract"]["interfaces"].as_array().unwrap();
    assert_eq!(interfaces.len(), 3);
    let values = &interfaces[0];
    assert_eq!(values["name"], "values");
    let types = values["types"].as_array().unwrap();
    let kinds: Vec<&str> = types
        .iter()
        .map(|ty| ty["kind"].as_str().unwrap())
        .collect();
    for expected in [
        "record", "variant", "enum", "flags", "list", "option", "result", "alias",
    ] {
        assert!(
            kinds.contains(&expected),
            "missing kind {expected}: {kinds:?}"
        );
    }

    // Record fields carry canonical_type, normalized docs, and lockfile wire numbers.
    let payload = types.iter().find(|t| t["name"] == "payload").unwrap();
    let name_field = &payload["fields"][0];
    assert_eq!(name_field["name"], "name");
    assert_eq!(name_field["docs"], "A string.");
    assert_eq!(name_field["canonical_type"], "string");
    assert_eq!(name_field["wire_number"], 1);

    // Functions carry method wire numbers, parameter wire numbers, and canonical result.
    let funcs = values["functions"].as_array().unwrap();
    let all_params = funcs.iter().find(|f| f["name"] == "all-params").unwrap();
    assert_eq!(all_params["wire_number"], 1);
    assert_eq!(all_params["params"][0]["name"], "bool-value");
    assert_eq!(all_params["params"][0]["wire_number"], 1);
    assert_eq!(
        all_params["canonical_result"],
        "result<list<values/payload>,string>"
    );

    // Worlds and lockfile order fingerprints are present.
    assert_eq!(parsed["contract"]["worlds"][0]["name"], "ignored");
    assert_eq!(
        parsed["contract"]["worlds"][0]["exports"][0]["interface"],
        "user"
    );
    assert!(
        !parsed["contract"]["lockfile"]["order"]
            .as_array()
            .unwrap()
            .is_empty(),
        "expected lockfile order fingerprints: {parsed}"
    );
}

#[allow(clippy::too_many_lines)]
#[test]
fn inspect_root_json_and_summary_include_versions_deps_reserved_and_retired() {
    let root = fixture("versions/wit");
    let rendered = inspect::inspect_root_json(&root, None).unwrap();
    let parsed: Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(parsed["version"], 1);
    assert_eq!(parsed["kind"], "root");
    let versions = parsed["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0]["directory"], "v0.1");
    assert_eq!(versions[0]["released"], true);
    assert_eq!(versions[1]["directory"], "v0.2");
    assert_eq!(versions[1]["released"], false);

    let summary = inspect::summarize_root(&root, None).unwrap();
    assert!(
        summary.contains("2 live version(s), 0 retired"),
        "{summary}"
    );
    assert!(summary.contains("v0.1 [released]"), "{summary}");
    assert!(summary.contains("v0.2 [draft]"), "{summary}");

    // Dependencies (`deps/`) appear with their packages, interfaces, and wire numbers.
    let deps_root = fixture("deps/wit");
    let deps_ir: Value =
        serde_json::from_str(&inspect::inspect_root_json(&deps_root, None).unwrap()).unwrap();
    let deps = deps_ir["versions"][0]["contract"]["dependencies"]
        .as_array()
        .unwrap();
    assert!(
        !deps.is_empty(),
        "expected resolved dependencies: {deps_ir}"
    );
    assert!(deps[0]["package"]["namespace"].is_string());
    assert!(
        !deps[0]["interfaces"].as_array().unwrap().is_empty(),
        "expected dependency interfaces: {deps_ir}"
    );
    // Dependency members carry deterministic non-null wire numbers derived from source order.
    let dep_item = &deps[0]["interfaces"][0]["types"][0];
    assert_eq!(dep_item["name"], "item");
    assert_eq!(dep_item["fields"][0]["name"], "id");
    assert_eq!(dep_item["fields"][0]["wire_number"], 1);
    assert_eq!(dep_item["fields"][1]["name"], "label");
    assert_eq!(dep_item["fields"][1]["wire_number"], 2);

    // Also verify single-version inspect populates dependency wire numbers.
    let single_deps_ir: Value =
        serde_json::from_str(&inspect::inspect_version_json(&deps_root.join("v0.1")).unwrap())
            .unwrap();
    assert_eq!(
        single_deps_ir["contract"]["dependencies"][0]["interfaces"][0]["types"][0]["fields"][0]["wire_number"],
        1
    );

    // Unlocked single version emits `lockfile: null` and stability metadata cleanly.
    let scratch = std::env::temp_dir().join(format!("witforge-inspect-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).unwrap();
    fs::write(
        scratch.join("contract.wit"),
        "package ex:stab@0.2.0;\n\
         interface api {\n\
           @since(version = 0.1.0)\n\
           @deprecated(version = 0.2.0)\n\
           record old { v: u32 }\n\
           @unstable(feature = preview)\n\
           record next { v: string }\n\
         }\n",
    )
    .unwrap();
    let single_ir: Value =
        serde_json::from_str(&inspect::inspect_version_json(&scratch).unwrap()).unwrap();
    assert!(single_ir["contract"]["lockfile"].is_null());
    assert_eq!(
        single_ir["contract"]["interfaces"][0]["types"][0]["stability"]["kind"],
        "deprecated"
    );
    assert_eq!(
        single_ir["contract"]["interfaces"][0]["types"][1]["stability"]["kind"],
        "unstable"
    );
    let single_summary = inspect::summarize_version(&scratch).unwrap();
    assert!(single_summary.contains("ex:stab@0.2.0"), "{single_summary}");
    assert!(
        single_summary.contains("lockfile: none"),
        "{single_summary}"
    );

    // Ensure in-memory models with block comments normalize docs cleanly.
    let mem = load_sources(vec![SourceFile::new(
        "contract.wit".into(),
        "/**\n * Clean doc.\n */\npackage ex:mem@0.1.0;\ninterface i { record r { a: u8 } }\n"
            .into(),
    )])
    .unwrap();
    let mem_ir: Value = serde_json::from_str(
        &inspect::emit_version_json(&model::build(&mem).unwrap(), None).unwrap(),
    )
    .unwrap();
    assert_eq!(mem_ir["contract"]["package"]["docs"], "Clean doc.");

    // Single-version validation guards: directory version mismatch (WF1031), wrong-package lockfile
    // (WF1002 with file location), lockfile type conflict (WF1105), reintroduced reserved member
    // (WF1103), non-regular/symlinked wire.lock, and canonical semver ordering of `retired`.
    let bad_dir = scratch.join("v0.1");
    fs::create_dir_all(&bad_dir).unwrap();
    fs::write(
        bad_dir.join("contract.wit"),
        "package ex:mismatch@0.2.0;\ninterface i { record r { a: u32 } }\n",
    )
    .unwrap();
    let mismatch_err = inspect::inspect_version_json(&bad_dir).unwrap_err();
    assert!(witforge::is_contract_error(&mismatch_err));
    assert_eq!(
        witforge::error_diagnostics(&mismatch_err)
            .unwrap()
            .first()
            .unwrap()
            .code,
        witforge::diagnostic::codes::DIRECTORY_VERSION_MISMATCH
    );

    let wrong_pkg = scratch.join("wrong-pkg");
    fs::create_dir_all(&wrong_pkg).unwrap();
    fs::write(
        wrong_pkg.join("contract.wit"),
        "package ex:actual@0.1.0;\ninterface i { record r { a: u32 } }\n",
    )
    .unwrap();
    fs::write(
        wrong_pkg.join("wire.lock"),
        format!(
            "{}\n\nex:other/i/r.a = 1 field u32\n",
            witforge::lock::HEADER
        ),
    )
    .unwrap();
    let wrong_pkg_err = inspect::inspect_version_json(&wrong_pkg).unwrap_err();
    assert!(
        witforge::is_contract_error(&wrong_pkg_err),
        "{wrong_pkg_err:#}"
    );
    let wp_diags = witforge::error_located_diagnostics(&wrong_pkg_err).unwrap();
    assert_eq!(
        wp_diags[0].diagnostic.code,
        witforge::diagnostic::codes::LOCKFILE_WRONG_PACKAGE
    );
    assert!(wp_diags[0].file.is_some());

    let retyped_dir = scratch.join("retyped");
    fs::create_dir_all(&retyped_dir).unwrap();
    fs::write(
        retyped_dir.join("contract.wit"),
        "package ex:retype@0.1.0;\ninterface i { record r { a: string } }\n",
    )
    .unwrap();
    fs::write(
        retyped_dir.join("wire.lock"),
        format!(
            "{}\n\nex:retype/i/r.a = 1 field u32\n",
            witforge::lock::HEADER
        ),
    )
    .unwrap();
    let retype_err = inspect::inspect_version_json(&retyped_dir).unwrap_err();
    assert!(witforge::is_contract_error(&retype_err), "{retype_err:#}");
    assert_eq!(
        witforge::error_diagnostics(&retype_err).unwrap()[0].code,
        witforge::diagnostic::codes::LOCKFILE_CHANGED_DRAFT
    );

    let reintro_dir = scratch.join("reintro");
    fs::create_dir_all(&reintro_dir).unwrap();
    fs::write(
        reintro_dir.join("contract.wit"),
        "package ex:reintro@0.1.0;\ninterface i { record r { a: u32 } }\n",
    )
    .unwrap();
    fs::write(
        reintro_dir.join("wire.lock"),
        format!(
            "{}\n\nex:reintro/i/r.a = 1 field u32 reserved\n",
            witforge::lock::HEADER
        ),
    )
    .unwrap();
    let reintro_err = inspect::inspect_version_json(&reintro_dir).unwrap_err();
    assert!(witforge::is_contract_error(&reintro_err), "{reintro_err:#}");
    assert_eq!(
        witforge::error_diagnostics(&reintro_err).unwrap()[0].code,
        witforge::diagnostic::codes::LOCKFILE_REINTRODUCED
    );

    let dir_lock = scratch.join("dir-lock");
    fs::create_dir_all(dir_lock.join("wire.lock")).unwrap();
    fs::write(
        dir_lock.join("contract.wit"),
        "package ex:dirlock@0.1.0;\ninterface i { record r { a: u32 } }\n",
    )
    .unwrap();
    assert!(inspect::inspect_version_json(&dir_lock).is_err());

    #[cfg(unix)]
    {
        let sym_lock = scratch.join("sym-lock");
        fs::create_dir_all(&sym_lock).unwrap();
        fs::write(
            sym_lock.join("contract.wit"),
            "package ex:symlock@0.1.0;\ninterface i { record r { a: u32 } }\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(wrong_pkg.join("wire.lock"), sym_lock.join("wire.lock"))
            .unwrap();
        let sym_err = inspect::inspect_version_json(&sym_lock).unwrap_err();
        assert_eq!(
            witforge::error_diagnostics(&sym_err).unwrap()[0].code,
            witforge::diagnostic::codes::GUARD_REFUSED
        );
    }

    // `inspect_root_json` sorts `retired` in ascending semver order regardless of manifest order.
    let retired_root = scratch.join("retired-order");
    for (dir_name, ver) in [("v0.1", "0.1.0"), ("v0.2", "0.2.0"), ("v1.0", "1.0.0")] {
        let vdir = retired_root.join(dir_name);
        fs::create_dir_all(&vdir).unwrap();
        fs::write(
            vdir.join("contract.wit"),
            format!("package ex:ret@{ver};\ninterface i {{ record r {{ a: u32 }} }}\n"),
        )
        .unwrap();
        let mut opts = witforge::workflow::VersionOptions::new(vdir.clone());
        if dir_name == "v0.2" {
            fs::copy(retired_root.join("v0.1/wire.lock"), vdir.join("wire.lock")).unwrap();
            opts.baseline =
                witforge::workflow::Baseline::Against(retired_root.join("v0.1/wire.lock"));
        } else {
            opts.baseline = witforge::workflow::Baseline::First;
        }
        witforge::workflow::lock(&opts).unwrap().write().unwrap();
    }
    fs::write(
        retired_root.join("released"),
        "v0.2 retired\nv0.1 retired\nv1.0\n",
    )
    .unwrap();
    let retired_ir: Value =
        serde_json::from_str(&inspect::inspect_root_json(&retired_root, None).unwrap()).unwrap();
    assert_eq!(
        retired_ir["retired"],
        serde_json::json!(["v0.1", "v0.2"]),
        "retired list must be sorted canonically"
    );

    fs::remove_dir_all(&scratch).unwrap();
}
