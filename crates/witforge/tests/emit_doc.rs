//! Integration tests for deterministic Markdown contract reference generation (`witforge::emit::doc`).

use std::fs;
use std::path::{Path, PathBuf};

use witforge::emit::doc;
use witforge::emit::output::GeneratedFileStatus;
use witforge::workflow::{Baseline, VersionOptions, lock};

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("witforge-emit-doc-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn lock_ver(dir: &Path, against: Option<&Path>) {
    let baseline = match against {
        Some(prev) => Baseline::Against(prev.to_path_buf()),
        None => Baseline::First,
    };
    let opts = VersionOptions::new(dir.to_path_buf()).with_baseline(baseline);
    let out = lock(&opts).unwrap();
    assert!(out.is_ok(), "{:?}", out.report.findings);
    out.write().unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn doc_emitter_renders_versions_stability_wire_numbers_reserved_diffs_and_enforces_ownership() {
    let root = scratch_dir("full");
    let wit = root.join("wit");

    let dep_wit = "package example:shared@0.1.0;\n\
                   interface common {\n\
                     record tag { key: string }\n\
                   }\n";
    let v01 = "/// Contract package docs.\n\
               package example:catalog@0.1.0;\n\
               \n\
               /// Core API.\n\
               @since(version = 0.1.0)\n\
               interface api {\n\
                 use example:shared/common@0.1.0.{tag};\n\
                 /// One catalog entry.\n\
                 @since(version = 0.1.0)\n\
                 record item {\n\
                   /// Unique identifier | primary key.\n\
                   id: u64,\n\
                   /// Legacy label.\n\
                   legacy: string,\n\
                   /// Attached tag.\n\
                   t: tag,\n\
                 }\n\
                 @since(version = 0.1.0)\n\
                 enum state { draft, active }\n\
                 @since(version = 0.1.0)\n\
                 flags caps { read, write }\n\
                 @since(version = 0.1.0)\n\
                 variant event { created(u64), deleted }\n\
                 @since(version = 0.1.0)\n\
                 type attrs = map<string, string>;\n\
                 /// Fetch one item.\n\
                 @since(version = 0.1.0)\n\
                 get: func(id: u64) -> option<item>;\n\
               }\n\
               \n\
               /// Service world.\n\
               @since(version = 0.1.0)\n\
               world service {\n\
                 @since(version = 0.1.0)\n\
                 export api;\n\
               }\n";
    let v02 = "/// Contract package docs.\n\
               package example:catalog@0.2.0;\n\
               \n\
               /// Core API.\n\
               @since(version = 0.1.0)\n\
               interface api {\n\
                 use example:shared/common@0.1.0.{tag};\n\
                 /// One catalog entry.\n\
                 @since(version = 0.1.0)\n\
                 record item {\n\
                   /// Unique identifier | primary key.\n\
                   id: u64,\n\
                   /// Attached tag.\n\
                   t: tag,\n\
                   /// Optional note.\n\
                   note: option<string>,\n\
                 }\n\
                 @since(version = 0.1.0)\n\
                 @deprecated(version = 0.2.0)\n\
                 enum state { draft, active, archived }\n\
                 @since(version = 0.1.0)\n\
                 flags caps { read, write }\n\
                 @since(version = 0.1.0)\n\
                 variant event { created(u64), deleted }\n\
                 @since(version = 0.1.0)\n\
                 type attrs = map<string, string>;\n\
                 /// Fetch one item.\n\
                 @since(version = 0.1.0)\n\
                 get: func(id: u64) -> option<item>;\n\
                 /// Experimental search.\n\
                 @unstable(feature = search-beta)\n\
                 search: func(query: string) -> list<item>;\n\
               }\n\
               \n\
               /// Service world.\n\
               @since(version = 0.1.0)\n\
               world service {\n\
                 @since(version = 0.1.0)\n\
                 export api;\n\
               }\n";

    for (ver, body) in [("v0.1", v01), ("v0.2", v02)] {
        let dir = wit.join(ver);
        let dep_dir = dir.join("deps/shared");
        fs::create_dir_all(&dep_dir).unwrap();
        fs::write(dep_dir.join("common.wit"), dep_wit).unwrap();
        fs::write(dir.join("contract.wit"), body).unwrap();
    }
    lock_ver(&wit.join("v0.1"), None);
    fs::copy(wit.join("v0.1/wire.lock"), wit.join("v0.2/wire.lock")).unwrap();
    lock_ver(&wit.join("v0.2"), Some(&wit.join("v0.1/wire.lock")));
    fs::write(wit.join("released"), "v0.1\n").unwrap();

    // 1. Plan is deterministic and byte-identical across repeated calls, LF-only, with ownership
    // header.
    let first = doc::plan(&wit).unwrap();
    let second = doc::plan(&wit).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first.header(),
        "<!-- @generated by witforge (doc, example:catalog); do not edit. -->"
    );
    let names: Vec<&str> = first.files().keys().map(String::as_str).collect();
    assert_eq!(names, vec!["index.md", "v0_1.md", "v0_2.md"]);
    for (name, content) in first.files() {
        assert!(!content.contains('\r'), "{name} must be LF-only");
        assert!(content.ends_with('\n'), "{name} must end with LF");
        assert!(content.starts_with(first.header()), "{name} missing header");
    }

    // 2. Inspect `index.md`: versions table, release/draft status, and adjacent version diff!
    let index = first.files().get("index.md").unwrap();
    assert!(
        index.contains("# `example:catalog` Contract Reference"),
        "{index}"
    );
    assert!(
        index.contains("| [0.1.0](v0_1.md) | `v0.1` | released |"),
        "{index}"
    );
    assert!(
        index.contains("| [0.2.0](v0_2.md) | `v0.2` | draft |"),
        "{index}"
    );
    assert!(index.contains("### 0.1.0 -> 0.2.0"), "{index}");
    assert!(index.contains("`api/item.note`"), "{index}");
    assert!(index.contains("`api/item.legacy`"), "{index}");
    assert!(index.contains("`api.search`"), "{index}");

    // 3. Inspect `v0_2.md`: dependencies, escaped table pipe `\|`, stability badges, wire numbers,
    // worlds, and reserved wire slot (`api/item.legacy` = 2)!
    let v02_md = first.files().get("v0_2.md").unwrap();
    assert!(
        v02_md.contains("- **Dependencies:** `example:shared@0.1.0`"),
        "{v02_md}"
    );
    assert!(
        v02_md.contains("Unique identifier \\| primary key."),
        "{v02_md}"
    );
    assert!(
        v02_md.contains("`deprecated` (since `0.1.0`, deprecated `0.2.0`)"),
        "{v02_md}"
    );
    assert!(
        v02_md.contains("`unstable` (feature `search-beta`)"),
        "{v02_md}"
    );
    assert!(
        v02_md.contains("Definition: `map<string, string>`"),
        "{v02_md}"
    );
    assert!(v02_md.contains("## Worlds"), "{v02_md}");
    assert!(v02_md.contains("## Reserved Wire Numbers"), "{v02_md}");
    assert!(
        v02_md.contains("| `example:catalog/api/item.legacy` | `2` | `field` | `string` |"),
        "{v02_md}"
    );

    // 4. Generate, `--check` clean, detect drift, refuse unowned file, and refuse foreign header!
    let out = root.join("docs-out");
    doc::generate(&wit, &out).unwrap();
    let check_clean = doc::check(&wit, &out).unwrap();
    assert!(check_clean.is_empty(), "{check_clean:?}");

    // Tamper with `v0_2.md` -> check detects Changed.
    let mut edited = fs::read_to_string(out.join("v0_2.md")).unwrap();
    edited.push_str("\nextra\n");
    fs::write(out.join("v0_2.md"), edited).unwrap();
    let findings = doc::check_with_command(&wit, &out, "witforge doc wit --out docs-out").unwrap();
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(matches!(
        findings[0].status,
        GeneratedFileStatus::Changed { .. }
    ));
    assert!(
        findings[0]
            .message()
            .contains("witforge doc wit --out docs-out")
    );

    // Restore clean and test retirement removes stale `v0_1.md` and lists it under Retired Versions!
    doc::generate(&wit, &out).unwrap();
    fs::write(wit.join("released"), "v0.1 retired\n").unwrap();
    doc::generate(&wit, &out).unwrap();
    assert!(
        !out.join("v0_1.md").exists(),
        "retired version page should be cleaned up"
    );
    let retired_index = fs::read_to_string(out.join("index.md")).unwrap();
    assert!(
        retired_index.contains("## Retired Versions"),
        "{retired_index}"
    );
    assert!(
        retired_index.contains("- `v0.1` (retired)"),
        "{retired_index}"
    );

    // Unowned `index.md` is refused!
    let unowned_out = root.join("unowned-out");
    fs::create_dir_all(&unowned_out).unwrap();
    fs::write(unowned_out.join("index.md"), "# Hand-written docs\n").unwrap();
    assert!(doc::generate(&wit, &unowned_out).is_err());

    // Foreign header (`rust` file in `doc` output dir) is refused!
    let foreign_out = root.join("foreign-out");
    fs::create_dir_all(&foreign_out).unwrap();
    fs::write(
        foreign_out.join("mod.rs"),
        "// @generated by witforge (rust, example:catalog); do not edit.\n",
    )
    .unwrap();
    assert!(doc::generate(&wit, &foreign_out).is_err());

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn eighth_wave_audit_doc_custom_released_decode_summary_and_escaped_pipe() {
    let root = scratch_dir("audit-doc");
    let wit = root.join("wit");
    fs::create_dir_all(wit.join("v0.1")).unwrap();
    fs::create_dir_all(wit.join("v0.2")).unwrap();

    // Decode-only change (new interface with function) + doc comment with `|`, `\|`, and `\\|`.
    fs::write(
        wit.join("v0.1/contract.wit"),
        "package example:docaudit@0.1.0;\n\
         interface api {\n\
           record item {\n\
             /// Pipe a | b, pre-escaped c \\| d, literal backslash before pipe e \\\\| f.\n\
             id: u64,\n\
           }\n\
         }\n",
    )
    .unwrap();
    fs::write(
        wit.join("v0.2/contract.wit"),
        "package example:docaudit@0.2.0;\n\
         interface api {\n\
           record item {\n\
             /// Pipe a | b, pre-escaped c \\| d, literal backslash before pipe e \\\\| f.\n\
             id: u64,\n\
           }\n\
         }\n\
         interface extra {\n\
           ping: func();\n\
         }\n",
    )
    .unwrap();
    lock_ver(&wit.join("v0.1"), None);
    fs::copy(wit.join("v0.1/wire.lock"), wit.join("v0.2/wire.lock")).unwrap();
    lock_ver(&wit.join("v0.2"), Some(&wit.join("v0.1/wire.lock")));

    // Explicit `--released` manifest at custom filename inside `wit/` while default `wit/released`
    // has invalid syntax — `plan_with_released` MUST use the explicit manifest (including during
    // adjacent version diffing) instead of failing on default `wit/released`!
    fs::write(wit.join("released"), "bad/entry/syntax\n").unwrap();
    let custom_released = wit.join("custom-released");
    fs::write(&custom_released, "v0.1\n").unwrap();

    let planned = doc::plan_with_released(&wit, Some(&custom_released)).unwrap();
    let index = planned.files().get("index.md").unwrap();
    assert!(
        index.contains("| [0.1.0](v0_1.md) | `v0.1` | released |"),
        "{index}"
    );
    assert!(
        index.contains(
            "Summary: 1 change(s), 0 breaking (wire: 0, source: 0, api: 0); decode: 1 (new<-old: 0, old<-new: 1)."
        ),
        "decode count must be separated from breaking parens:\n{index}"
    );

    let v01_md = planned.files().get("v0_1.md").unwrap();
    assert!(
        v01_md.contains(
            "Pipe a \\| b, pre-escaped c \\| d, literal backslash before pipe e \\\\\\| f."
        ),
        "already-escaped \\| must not become \\\\|:\n{v01_md}"
    );

    fs::remove_dir_all(&root).unwrap();
}
