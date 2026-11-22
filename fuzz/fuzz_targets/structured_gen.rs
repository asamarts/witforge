//! Structure-aware WIT contract generator and invariant checker shared between
//! `fuzz/fuzz_targets/structured_contract.rs`, `fuzz/fuzz_targets/wit_package.rs`, and
//! `crates/witforge/tests/properties.rs`.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use witforge::emit::{glue, json_schema, proto, rust};
use witforge::lock::WireLock;
use witforge::{diff, load, model, subset};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// RAII temporary scratch directory created under `std::env::temp_dir()` (honoring `TMPDIR`)
/// and removed on drop.
pub struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    pub fn new(prefix: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "witforge-fuzz-{prefix}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create fuzz scratch dir");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Deterministic zero-dependency byte cursor over fuzzer or property-test bytes.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn next_u8(&mut self) -> u8 {
        if self.bytes.is_empty() {
            return 0;
        }
        let b = self.bytes[self.pos % self.bytes.len()];
        self.pos = self.pos.wrapping_add(1);
        b
    }

    fn choose<'b, T>(&mut self, slice: &'b [T]) -> &'b T {
        &slice[usize::from(self.next_u8()) % slice.len()]
    }

    fn range(&mut self, low: usize, high_inclusive: usize) -> usize {
        if high_inclusive <= low {
            return low;
        }
        low + (usize::from(self.next_u8()) % (high_inclusive - low + 1))
    }

    fn bool(&mut self) -> bool {
        (self.next_u8() & 1) == 1
    }
}

/// WIT keywords that require `%` escaping in identifier positions.
const WIT_KEYWORDS: &[&str] = &[
    "use",
    "type",
    "func",
    "u8",
    "u16",
    "u32",
    "u64",
    "s8",
    "s16",
    "s32",
    "s64",
    "f32",
    "f64",
    "char",
    "record",
    "flags",
    "variant",
    "enum",
    "bool",
    "string",
    "option",
    "result",
    "future",
    "stream",
    "list",
    "as",
    "from",
    "static",
    "interface",
    "tuple",
    "import",
    "export",
    "world",
    "package",
    "with",
    "include",
    "resource",
    "own",
    "borrow",
    "async",
    "error-context",
    "map",
];

fn wit_escape(ident: &str) -> String {
    if WIT_KEYWORDS.contains(&ident) {
        format!("%{ident}")
    } else {
        ident.to_string()
    }
}

/// Identifier pools intentionally including one-letter, upper-case, digits, and Rust/proto/WIT
/// keywords so structured fuzzing reaches every identifier-normalization and collision path.
const PKG_SEGMENTS: &[&str] = &[
    "ex", "a", "API", "v2", "service", "message", "option", "gen", "extern", "union", "HTTP",
    "data", "my-ns", "alpha1",
];

const DEP_PKG_NAMES: &[&str] = &["shared", "b", "DEP", "lib2", "common", "meta"];

const IFACE_NAMES: &[&str] = &[
    "types", "api", "a", "x", "API", "HTTP-api", "v2", "service", "error", "type", "gen", "extern",
    "union", "match",
];

const TYPE_NAMES: &[&str] = &[
    "item",
    "node",
    "a",
    "x-y",
    "URL-entry",
    "a1",
    "foo2-bar",
    "error",
    "type",
    "where",
    "gen",
    "extern",
    "union",
    "message",
    "status",
    "choice",
    "FOO-bar",
];

const MEMBER_NAMES: &[&str] = &[
    "id", "name", "x", "a1", "URL", "foo-BAR", "foo2-bar", "message", "option", "type", "self",
    "super", "crate", "gen", "extern", "union", "mode", "label", "val",
];

const FN_NAMES: &[&str] = &[
    "ping", "get", "a", "get-x-y", "a1", "type", "match", "gen", "extern", "run", "fetch",
];

const WORLD_NAMES: &[&str] = &["plugin", "host", "w", "APP-world", "v2-world", "runner"];

const PRIMITIVES: &[&str] = &[
    "bool", "u8", "u16", "u32", "u64", "s8", "s16", "s32", "s64", "f32", "f64", "string",
];

fn pick_distinct(cursor: &mut Cursor<'_>, pool: &[&str], count: usize) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for _ in 0..count {
        for _ in 0..pool.len() {
            let cand = *cursor.choose(pool);
            if seen.insert(cand.to_ascii_lowercase()) {
                out.push(cand.to_string());
                break;
            }
        }
    }
    out
}

fn gen_type_expr(
    cursor: &mut Cursor<'_>,
    depth: usize,
    local_types: &[String],
    dep_aliases: &[String],
    byte_alias: Option<&str>,
) -> String {
    if depth >= 2 {
        return gen_leaf_type(cursor, local_types, dep_aliases, byte_alias);
    }
    match cursor.next_u8() % 10 {
        0..=5 => gen_leaf_type(cursor, local_types, dep_aliases, byte_alias),
        6 => format!(
            "list<{}>",
            gen_type_expr(cursor, depth + 1, local_types, dep_aliases, byte_alias)
        ),
        7 => format!(
            "option<{}>",
            gen_type_expr(cursor, depth + 1, local_types, dep_aliases, byte_alias)
        ),
        8 => match cursor.next_u8() % 4 {
            0 => format!(
                "result<{}, {}>",
                gen_type_expr(cursor, depth + 1, local_types, dep_aliases, byte_alias),
                gen_type_expr(cursor, depth + 1, local_types, dep_aliases, byte_alias)
            ),
            1 => format!(
                "result<{}>",
                gen_type_expr(cursor, depth + 1, local_types, dep_aliases, byte_alias)
            ),
            2 => format!(
                "result<_, {}>",
                gen_type_expr(cursor, depth + 1, local_types, dep_aliases, byte_alias)
            ),
            _ => "result".to_string(),
        },
        _ => match byte_alias {
            Some(alias) if cursor.bool() => format!("list<{}>", wit_escape(alias)),
            _ => "list<u8>".to_string(),
        },
    }
}

fn gen_leaf_type(
    cursor: &mut Cursor<'_>,
    local_types: &[String],
    dep_aliases: &[String],
    byte_alias: Option<&str>,
) -> String {
    let roll = cursor.next_u8() % 10;
    if roll < 3 && !local_types.is_empty() {
        return wit_escape(cursor.choose(local_types).as_str());
    }
    if roll == 3 && !dep_aliases.is_empty() {
        return wit_escape(cursor.choose(dep_aliases).as_str());
    }
    if roll == 4 {
        return match byte_alias {
            Some(alias) if cursor.bool() => format!("list<{}>", wit_escape(alias)),
            _ => "list<u8>".to_string(),
        };
    }
    (*cursor.choose(PRIMITIVES)).to_string()
}

/// Generated multi-file contract with optional dependencies.
#[derive(Debug, Clone)]
pub struct GeneratedContract {
    pub files: Vec<(String, String)>,
    pub deps: Vec<(String, String, String)>,
}

impl GeneratedContract {
    /// Materialize this contract into `version_dir` (`*.wit` and `deps/<dir>/<file>`).
    pub fn write_to(&self, version_dir: &Path) {
        fs::create_dir_all(version_dir).expect("create version dir");
        for (dep_dir, file_name, content) in &self.deps {
            let dir = version_dir.join("deps").join(dep_dir);
            fs::create_dir_all(&dir).expect("create dep dir");
            fs::write(dir.join(file_name), content).expect("write dep wit");
        }
        for (file_name, content) in &self.files {
            fs::write(version_dir.join(file_name), content).expect("write root wit");
        }
    }
}

/// Build a structured WIT contract from arbitrary bytes, covering one-letter/upper-case names,
/// multiple `.wit` files in the version directory, and optional `deps/` packages.
#[allow(clippy::too_many_lines)]
pub fn generate_from_bytes(data: &[u8]) -> GeneratedContract {
    let mut cursor = Cursor::new(data);
    let root_ns = (*cursor.choose(PKG_SEGMENTS)).to_string();
    let root_pkg = (*cursor.choose(PKG_SEGMENTS)).to_string();
    let header = format!(
        "package {}:{}@0.1.0;\n\n",
        wit_escape(&root_ns),
        wit_escape(&root_pkg)
    );

    // Optional 0..=2 dependency packages with distinct `(ns, name)` different from root.
    let dep_count = cursor.range(0, 2);
    let mut used_pkgs: BTreeSet<(String, String)> = BTreeSet::new();
    used_pkgs.insert((root_ns.to_ascii_lowercase(), root_pkg.to_ascii_lowercase()));

    let mut deps = Vec::new();
    let mut dep_refs: Vec<(String, String, String, String)> = Vec::new();
    for idx in 0..dep_count {
        let dep_ns = (*cursor.choose(PKG_SEGMENTS)).to_string();
        let dep_pkg = format!("{}{idx}", *cursor.choose(DEP_PKG_NAMES));
        if !used_pkgs.insert((dep_ns.to_ascii_lowercase(), dep_pkg.to_ascii_lowercase())) {
            continue;
        }
        let dep_iface = if cursor.bool() { "types" } else { "T" }.to_string();
        let dep_type = if cursor.bool() { "shared-item" } else { "D" }.to_string();
        let dep_field_count = cursor.range(1, 2);
        let dep_fields = pick_distinct(&mut cursor, MEMBER_NAMES, dep_field_count);
        let mut dep_src = format!(
            "package {}:{}@0.1.0;\n\ninterface {} {{\n  record {} {{\n",
            wit_escape(&dep_ns),
            wit_escape(&dep_pkg),
            wit_escape(&dep_iface),
            wit_escape(&dep_type),
        );
        for f in &dep_fields {
            let ty = *cursor.choose(PRIMITIVES);
            let _ = writeln!(dep_src, "    {}: {ty},", wit_escape(f));
        }
        dep_src.push_str("  }\n");
        if cursor.bool() {
            let _ = writeln!(
                dep_src,
                "  ping-dep: func(v: {}) -> bool;",
                wit_escape(&dep_type)
            );
        }
        dep_src.push_str("}\n");
        let dep_dir = format!("dep-{idx}");
        deps.push((dep_dir, "types.wit".to_string(), dep_src));
        dep_refs.push((dep_ns, dep_pkg, dep_iface, dep_type));
    }

    // Generate 1..=3 interfaces and optionally split them across multiple `.wit` files.
    let iface_count = cursor.range(1, 3);
    let iface_names = pick_distinct(&mut cursor, IFACE_NAMES, iface_count);
    let multi_file = iface_names.len() > 1 && cursor.bool();

    let mut iface_blocks: Vec<(String, String)> = Vec::new();
    for iface_name in &iface_names {
        let mut block = format!("interface {} {{\n", wit_escape(iface_name));
        let mut dep_aliases = Vec::new();
        let mut used_type_names: BTreeSet<String> = BTreeSet::new();

        for (idx, (dns, dpkg, diface, dtype)) in dep_refs.iter().enumerate() {
            if cursor.bool() {
                let alias = format!("dep-t{idx}");
                if used_type_names.insert(alias.to_ascii_lowercase()) {
                    let _ = writeln!(
                        block,
                        "  use {}:{}/{}@0.1.0.{{{} as {}}};",
                        wit_escape(dns),
                        wit_escape(dpkg),
                        wit_escape(diface),
                        wit_escape(dtype),
                        wit_escape(&alias),
                    );
                    dep_aliases.push(alias);
                }
            }
        }

        let byte_alias = if cursor.bool() && used_type_names.insert("byte-t".to_string()) {
            block.push_str("  type byte-t = u8;\n");
            Some("byte-t")
        } else {
            None
        };

        let type_count = cursor.range(1, 4);
        let candidate_types = pick_distinct(&mut cursor, TYPE_NAMES, type_count);
        let mut declared_types: Vec<String> = Vec::new();
        for tname in candidate_types {
            if !used_type_names.insert(tname.to_ascii_lowercase()) {
                continue;
            }
            match cursor.next_u8() % 5 {
                0 => {
                    let count = cursor.range(1, 3);
                    let fields = pick_distinct(&mut cursor, MEMBER_NAMES, count);
                    let _ = writeln!(block, "  record {} {{", wit_escape(&tname));
                    for f in &fields {
                        let ty = gen_type_expr(
                            &mut cursor,
                            0,
                            &declared_types,
                            &dep_aliases,
                            byte_alias,
                        );
                        let _ = writeln!(block, "    {}: {ty},", wit_escape(f));
                    }
                    block.push_str("  }\n");
                }
                1 => {
                    let count = cursor.range(1, 3);
                    let cases = pick_distinct(&mut cursor, MEMBER_NAMES, count);
                    let _ = writeln!(block, "  variant {} {{", wit_escape(&tname));
                    for c in &cases {
                        if cursor.bool() {
                            let ty = gen_type_expr(
                                &mut cursor,
                                0,
                                &declared_types,
                                &dep_aliases,
                                byte_alias,
                            );
                            let _ = writeln!(block, "    {}({ty}),", wit_escape(c));
                        } else {
                            let _ = writeln!(block, "    {},", wit_escape(c));
                        }
                    }
                    block.push_str("  }\n");
                }
                2 => {
                    let count = cursor.range(1, 3);
                    let cases = pick_distinct(&mut cursor, MEMBER_NAMES, count);
                    let _ = writeln!(block, "  enum {} {{", wit_escape(&tname));
                    for c in &cases {
                        let _ = writeln!(block, "    {},", wit_escape(c));
                    }
                    block.push_str("  }\n");
                }
                3 => {
                    let count = cursor.range(1, 3);
                    let flags = pick_distinct(&mut cursor, MEMBER_NAMES, count);
                    let _ = writeln!(block, "  flags {} {{", wit_escape(&tname));
                    for fl in &flags {
                        let _ = writeln!(block, "    {},", wit_escape(fl));
                    }
                    block.push_str("  }\n");
                }
                _ => {
                    let ty =
                        gen_type_expr(&mut cursor, 0, &declared_types, &dep_aliases, byte_alias);
                    let _ = writeln!(block, "  type {} = {ty};", wit_escape(&tname));
                }
            }
            declared_types.push(tname);
        }

        let fn_count = cursor.range(0, 2);
        let fn_names = pick_distinct(&mut cursor, FN_NAMES, fn_count);
        for fname in &fn_names {
            let param_count = cursor.range(0, 2);
            let params = pick_distinct(&mut cursor, MEMBER_NAMES, param_count);
            let rendered_params: Vec<String> = params
                .iter()
                .map(|p| {
                    let ty =
                        gen_type_expr(&mut cursor, 0, &declared_types, &dep_aliases, byte_alias);
                    format!("{}: {ty}", wit_escape(p))
                })
                .collect();
            let ret = if cursor.bool() {
                format!(
                    " -> {}",
                    gen_type_expr(&mut cursor, 0, &declared_types, &dep_aliases, byte_alias)
                )
            } else {
                String::new()
            };
            let _ = writeln!(
                block,
                "  {}: func({}){ret};",
                wit_escape(fname),
                rendered_params.join(", ")
            );
        }
        block.push_str("}\n\n");
        iface_blocks.push((iface_name.clone(), block));
    }

    // Optional world importing/exporting local and dependency interfaces.
    let mut world_block = String::new();
    if cursor.bool() {
        let iface_lower: BTreeSet<String> =
            iface_names.iter().map(|n| n.to_ascii_lowercase()).collect();
        let wname = WORLD_NAMES
            .iter()
            .copied()
            .find(|w| !iface_lower.contains(&w.to_ascii_lowercase()))
            .unwrap_or("app-world");
        let _ = writeln!(world_block, "world {} {{", wit_escape(wname));
        if let Some(first_iface) = iface_names.first() {
            let _ = writeln!(world_block, "  export {};", wit_escape(first_iface));
        }
        if let Some(second_iface) = iface_names.get(1)
            && cursor.bool()
        {
            let _ = writeln!(world_block, "  import {};", wit_escape(second_iface));
        }
        if let Some((dns, dpkg, diface, _)) = dep_refs.first()
            && cursor.bool()
        {
            let _ = writeln!(
                world_block,
                "  import {}:{}/{}@0.1.0;",
                wit_escape(dns),
                wit_escape(dpkg),
                wit_escape(diface),
            );
        }
        world_block.push_str("}\n");
    }

    let mut files = Vec::new();
    if multi_file {
        let mut first_file = header.clone();
        first_file.push_str(&iface_blocks[0].1);
        files.push(("a_first.wit".to_string(), first_file));

        let mut second_file = header;
        for (_, block) in &iface_blocks[1..] {
            second_file.push_str(block);
        }
        second_file.push_str(&world_block);
        files.push(("b_rest.wit".to_string(), second_file));
    } else {
        let mut single = header;
        for (_, block) in &iface_blocks {
            single.push_str(block);
        }
        single.push_str(&world_block);
        files.push(("contract.wit".to_string(), single));
    }

    GeneratedContract { files, deps }
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create copy dest dir");
    for entry in fs::read_dir(src).expect("read src dir") {
        let entry = entry.expect("dir entry");
        let target = dst.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir_recursive(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy file");
        }
    }
}

/// Assert every required compiler invariant on a version directory inside `wit_root`.
///
/// Returns `true` if the contract passed `subset::check_diagnostics` and all invariants were
/// exercised, or `false` if `load` or `subset` legitimately rejected it.
#[allow(clippy::too_many_lines)]
pub fn assert_invariants_for_version_dir(wit_root: &Path, version_dir: &Path) -> bool {
    let Ok(contract) = load(version_dir) else {
        return false;
    };
    let diags = subset::check_diagnostics(&contract);
    if !diags.is_empty() {
        return false;
    }
    assert!(
        subset::check(&contract).is_empty(),
        "subset::check must be empty when subset::check_diagnostics is empty"
    );

    let model = model::build(&contract).expect("subset-clean contract must build model");
    let members = model
        .members()
        .expect("subset-clean model must extract members");

    // Lockfile reconciliation, canonical round-trip, and idempotence.
    let mut lock = WireLock::default();
    lock.reconcile(&members)
        .expect("initial lockfile reconciliation must succeed");
    let rendered = lock.render();
    let reparsed = WireLock::parse(&rendered).expect("rendered lockfile must parse");
    assert_eq!(
        reparsed.render(),
        rendered,
        "lockfile must re-render byte-identically"
    );
    assert!(
        reparsed.is_canonical(&rendered),
        "re-rendered lockfile must be canonical"
    );
    let second_changes = lock
        .reconcile(&members)
        .expect("second reconcile must succeed");
    assert!(
        second_changes.is_empty(),
        "lock run twice must report zero changes: {second_changes:?}"
    );
    assert_eq!(
        lock.render(),
        rendered,
        "lock run twice must keep rendered bytes identical"
    );

    // Ensure the version directory name matches `v<major>.<minor>` for generator plans.
    let major = model.package.version.major;
    let minor = model.package.version.minor;
    let expected_name = format!("v{major}.{minor}");
    let active_dir = if version_dir.file_name().and_then(std::ffi::OsStr::to_str)
        == Some(expected_name.as_str())
    {
        version_dir.to_path_buf()
    } else {
        let renamed = wit_root.join(&expected_name);
        let _ = fs::remove_dir_all(&renamed);
        fs::rename(version_dir, &renamed).expect("rename version dir to match package version");
        renamed
    };
    fs::write(active_dir.join("wire.lock"), &rendered).expect("write wire.lock");

    // All four emitters must succeed and be byte-for-byte deterministic across runs.
    let rust1 = rust::plan(wit_root).expect("rust::plan must succeed for subset-clean contract");
    let rust2 = rust::plan(wit_root).expect("rust::plan second run must succeed");
    assert_eq!(
        rust1.files(),
        rust2.files(),
        "rust::plan must be deterministic"
    );

    let proto1 = proto::plan(wit_root).expect("proto::plan must succeed for subset-clean contract");
    let proto2 = proto::plan(wit_root).expect("proto::plan second run must succeed");
    assert_eq!(
        proto1.files(),
        proto2.files(),
        "proto::plan must be deterministic"
    );

    let schema1 = json_schema::plan(wit_root)
        .expect("json_schema::plan must succeed for subset-clean contract");
    let schema2 = json_schema::plan(wit_root).expect("json_schema::plan second run must succeed");
    assert_eq!(
        schema1.files(),
        schema2.files(),
        "json_schema::plan must be deterministic"
    );

    let glue1 = glue::plan(wit_root, "crate::plain", "crate::proto")
        .expect("glue::plan must succeed for subset-clean contract");
    let glue2 = glue::plan(wit_root, "crate::plain", "crate::proto")
        .expect("glue::plan second run must succeed");
    assert_eq!(
        glue1.files(),
        glue2.files(),
        "glue::plan must be deterministic"
    );

    // `diff` of a version with itself must be empty.
    let self_diff = diff::compare(&active_dir, &active_dir).expect("self diff must succeed");
    assert!(
        self_diff.changes.is_empty(),
        "diff of a version with itself must be empty: {:?}",
        self_diff.changes
    );

    // Two identical versions (bumped to the next minor) must convert cleanly.
    if let Some(next_minor) = minor.checked_add(1) {
        let old_ver = format!("@{major}.{minor}.0;");
        let new_ver = format!("@{major}.{next_minor}.0;");
        let next_dir = wit_root.join(format!("v{major}.{next_minor}"));
        copy_dir_recursive(&active_dir, &next_dir);
        let mut bumped = false;
        for entry in fs::read_dir(&next_dir).expect("read next_dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if path.extension().and_then(std::ffi::OsStr::to_str) == Some("wit") {
                let text = fs::read_to_string(&path).expect("read wit");
                if text.contains(&old_ver) {
                    fs::write(&path, text.replacen(&old_ver, &new_ver, 1))
                        .expect("write bumped wit");
                    bumped = true;
                }
            }
        }
        if bumped {
            let multi_rust1 = rust::plan(wit_root)
                .expect("two identical versions must generate convert.rs cleanly");
            let multi_rust2 = rust::plan(wit_root).expect("second multi-version rust::plan");
            assert_eq!(
                multi_rust1.files(),
                multi_rust2.files(),
                "multi-version rust::plan must be deterministic"
            );
            assert!(
                multi_rust1.files().contains_key("convert.rs"),
                "two identical versions must emit convert.rs"
            );
        }
    }

    true
}
