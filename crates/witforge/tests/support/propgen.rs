//! Shared deterministic & proptest WIT contract generator for `W10-067` and `W10-068`.
//!
//! Produces valid WIT version directories (with optional `deps/` packages, worlds, one-letter
//! names, upper-case names, digits, Rust/proto keywords, nested structural types, byte-list
//! aliases, and unreachable unsupported dependency chains).

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use proptest::prelude::*;

/// One generated dependency package under `deps/<dir_name>/types.wit`.
#[derive(Debug, Clone)]
pub struct GeneratedDep {
    pub dir_name: String,
    pub namespace: String,
    pub package: String,
    pub wit: String,
    /// One supported record type exported by `types` that the root (or another dep) can `use`.
    pub exported_type: Option<String>,
}

/// One generated single-version WIT contract (root + optional dependencies).
#[derive(Debug, Clone)]
pub struct GeneratedContract {
    pub namespace: String,
    pub package: String,
    pub root_wit: String,
    pub deps: Vec<GeneratedDep>,
}

impl GeneratedContract {
    /// Materialize this contract into `version_dir` (`contract.wit` and `deps/*/types.wit`).
    pub fn write_to(&self, version_dir: &Path) {
        fs::create_dir_all(version_dir).unwrap();
        for dep in &self.deps {
            let dep_dir = version_dir.join("deps").join(&dep.dir_name);
            fs::create_dir_all(&dep_dir).unwrap();
            fs::write(dep_dir.join("types.wit"), &dep.wit).unwrap();
        }
        fs::write(version_dir.join("contract.wit"), &self.root_wit).unwrap();
    }
}

/// WIT identifiers that need `%` escaping in WIT syntax.
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

/// Escape a WIT identifier with `%` when it is a WIT keyword.
pub fn wit_escape(ident: &str) -> String {
    if WIT_KEYWORDS.contains(&ident) {
        format!("%{ident}")
    } else {
        ident.to_string()
    }
}

const PKG_SEGMENTS: &[&str] = &[
    "example", "ex", "a", "API", "v2", "service", "message", "option", "enum", "package", "import",
    "gen", "extern", "union", "self", "super", "my-ns", "alpha1", "HTTP", "data",
];

const IFACE_NAMES: &[&str] = &[
    "types", "api", "a", "x", "API", "HTTP-api", "v2", "service", "message", "error", "type",
    "gen", "extern", "union", "self", "match",
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
    "get-request",
    "functions",
    "type",
    "where",
    "gen",
    "extern",
    "union",
    "self",
    "message",
    "service",
    "status",
    "choice",
];

const MEMBER_NAMES: &[&str] = &[
    "id",
    "name",
    "x",
    "a1",
    "URL",
    "foo-BAR",
    "foo2-bar",
    "message",
    "option",
    "type",
    "self",
    "super",
    "crate",
    "gen",
    "extern",
    "union",
    "mode",
    "set-mode",
    "unspecified",
    "witforge-value",
];

const FN_NAMES: &[&str] = &[
    "ping",
    "get",
    "a",
    "get-x-y",
    "a1",
    "type",
    "match",
    "gen",
    "extern",
    "self",
    "unspecified",
    "run",
];

#[derive(Debug, Clone)]
enum TypeSpec {
    Prim(&'static str),
    ByteList,
    ByteAliasList,
    LocalNamed(usize),
    DepNamed(usize),
    List(Box<TypeSpec>),
    Option(Box<TypeSpec>),
    Result(Option<Box<TypeSpec>>, Option<Box<TypeSpec>>),
}

fn arb_type_spec(max_depth: u32, local_count: usize, dep_count: usize) -> BoxedStrategy<TypeSpec> {
    let mut leaves: Vec<BoxedStrategy<TypeSpec>> = vec![
        prop_oneof![
            Just(TypeSpec::Prim("bool")),
            Just(TypeSpec::Prim("u8")),
            Just(TypeSpec::Prim("u16")),
            Just(TypeSpec::Prim("u32")),
            Just(TypeSpec::Prim("u64")),
            Just(TypeSpec::Prim("s8")),
            Just(TypeSpec::Prim("s16")),
            Just(TypeSpec::Prim("s32")),
            Just(TypeSpec::Prim("s64")),
            Just(TypeSpec::Prim("f32")),
            Just(TypeSpec::Prim("f64")),
            Just(TypeSpec::Prim("string")),
            Just(TypeSpec::ByteList),
            Just(TypeSpec::ByteAliasList),
        ]
        .boxed(),
    ];
    if local_count > 0 {
        leaves.push((0..local_count).prop_map(TypeSpec::LocalNamed).boxed());
    }
    if dep_count > 0 {
        leaves.push((0..dep_count).prop_map(TypeSpec::DepNamed).boxed());
    }
    let leaf = proptest::strategy::Union::new(leaves);
    if max_depth == 0 {
        return leaf.boxed();
    }
    leaf.prop_recursive(max_depth, 8, 2, |inner| {
        prop_oneof![
            inner.clone().prop_map(|t| TypeSpec::List(Box::new(t))),
            inner.clone().prop_map(|t| TypeSpec::Option(Box::new(t))),
            (
                proptest::option::of(inner.clone()),
                proptest::option::of(inner),
            )
                .prop_map(|(ok, err)| TypeSpec::Result(ok.map(Box::new), err.map(Box::new))),
        ]
    })
    .boxed()
}

fn render_type_spec(
    spec: &TypeSpec,
    local_names: &[String],
    dep_aliases: &[String],
    byte_alias: Option<&str>,
) -> String {
    match spec {
        TypeSpec::Prim(p) => (*p).to_string(),
        TypeSpec::ByteList => "list<u8>".to_string(),
        TypeSpec::ByteAliasList => match byte_alias {
            Some(alias) => format!("list<{}>", wit_escape(alias)),
            None => "list<u8>".to_string(),
        },
        TypeSpec::LocalNamed(idx) => {
            if let Some(name) = local_names.get(*idx) {
                wit_escape(name)
            } else {
                "u32".to_string()
            }
        }
        TypeSpec::DepNamed(idx) => {
            if let Some(name) = dep_aliases.get(*idx) {
                wit_escape(name)
            } else {
                "u32".to_string()
            }
        }
        TypeSpec::List(elem) => format!(
            "list<{}>",
            render_type_spec(elem, local_names, dep_aliases, byte_alias)
        ),
        TypeSpec::Option(inner) => format!(
            "option<{}>",
            render_type_spec(inner, local_names, dep_aliases, byte_alias)
        ),
        TypeSpec::Result(ok, err) => match (ok, err) {
            (Some(ok), Some(err)) => format!(
                "result<{}, {}>",
                render_type_spec(ok, local_names, dep_aliases, byte_alias),
                render_type_spec(err, local_names, dep_aliases, byte_alias),
            ),
            (Some(ok), None) => format!(
                "result<{}>",
                render_type_spec(ok, local_names, dep_aliases, byte_alias)
            ),
            (None, Some(err)) => format!(
                "result<_, {}>",
                render_type_spec(err, local_names, dep_aliases, byte_alias)
            ),
            (None, None) => "result".to_string(),
        },
    }
}

#[derive(Debug, Clone)]
enum TypeDeclSpec {
    Record(Vec<(String, TypeSpec)>),
    Variant(Vec<(String, Option<TypeSpec>)>),
    Enum(Vec<String>),
    Flags(Vec<String>),
    Alias(TypeSpec),
}

#[derive(Debug, Clone)]
struct FuncSpec {
    name: String,
    params: Vec<(String, TypeSpec)>,
    result: Option<TypeSpec>,
}

#[derive(Debug, Clone)]
struct InterfaceSpec {
    name: String,
    include_byte_alias: bool,
    types: Vec<(String, TypeDeclSpec)>,
    functions: Vec<FuncSpec>,
}

fn pick_distinct(pool: &'static [&'static str], count: usize) -> BoxedStrategy<Vec<String>> {
    proptest::collection::vec(proptest::sample::select(pool), count..=count)
        .prop_map(|items| {
            let mut seen = BTreeSet::new();
            let mut out = Vec::new();
            for item in items {
                if seen.insert(item.to_ascii_lowercase()) {
                    out.push(item.to_string());
                }
            }
            out
        })
        .boxed()
}

fn arb_interface(dep_count: usize) -> BoxedStrategy<InterfaceSpec> {
    (
        proptest::sample::select(IFACE_NAMES),
        any::<bool>(),
        (1usize..=4).prop_flat_map(|n| pick_distinct(TYPE_NAMES, n)),
        (0usize..=2).prop_flat_map(|n| pick_distinct(FN_NAMES, n)),
    )
        .prop_flat_map(
            move |(iface_name, include_byte_alias, type_names, fn_names)| {
                let type_count = type_names.len();
                let type_decls: Vec<BoxedStrategy<(String, TypeDeclSpec)>> = type_names
                    .into_iter()
                    .enumerate()
                    .map(|(idx, name)| {
                        let decl = prop_oneof![
                            (1usize..=3)
                                .prop_flat_map(|k| pick_distinct(MEMBER_NAMES, k))
                                .prop_flat_map(move |fields| {
                                    let fs: Vec<_> = fields
                                        .into_iter()
                                        .map(|f| (Just(f), arb_type_spec(2, idx, dep_count)))
                                        .collect();
                                    fs.prop_map(TypeDeclSpec::Record)
                                }),
                            (1usize..=3)
                                .prop_flat_map(|k| pick_distinct(MEMBER_NAMES, k))
                                .prop_flat_map(move |cases| {
                                    let cs: Vec<_> = cases
                                        .into_iter()
                                        .map(|c| {
                                            (
                                                Just(c),
                                                proptest::option::of(arb_type_spec(
                                                    2, idx, dep_count,
                                                )),
                                            )
                                        })
                                        .collect();
                                    cs.prop_map(TypeDeclSpec::Variant)
                                }),
                            (1usize..=3)
                                .prop_flat_map(|k| pick_distinct(MEMBER_NAMES, k))
                                .prop_map(TypeDeclSpec::Enum),
                            (1usize..=3)
                                .prop_flat_map(|k| pick_distinct(MEMBER_NAMES, k))
                                .prop_map(TypeDeclSpec::Flags),
                            arb_type_spec(2, idx, dep_count).prop_map(TypeDeclSpec::Alias),
                        ];
                        (Just(name), decl).boxed()
                    })
                    .collect();
                let func_decls: Vec<BoxedStrategy<FuncSpec>> = fn_names
                    .into_iter()
                    .map(|fname| {
                        (
                            (0usize..=2)
                                .prop_flat_map(|k| pick_distinct(MEMBER_NAMES, k))
                                .prop_flat_map(move |params| {
                                    let ps: Vec<_> = params
                                        .into_iter()
                                        .map(|p| (Just(p), arb_type_spec(2, type_count, dep_count)))
                                        .collect();
                                    ps
                                }),
                            proptest::option::of(arb_type_spec(2, type_count, dep_count)),
                        )
                            .prop_map(move |(params, result)| FuncSpec {
                                name: fname.clone(),
                                params,
                                result,
                            })
                            .boxed()
                    })
                    .collect();
                (
                    Just(iface_name.to_string()),
                    Just(include_byte_alias),
                    type_decls,
                    func_decls,
                )
                    .prop_map(|(name, include_byte_alias, types, functions)| {
                        InterfaceSpec {
                            name,
                            include_byte_alias,
                            types,
                            functions,
                        }
                    })
            },
        )
        .boxed()
}

/// Strategy generating arbitrary single-version WIT contracts (with optional dependencies,
/// unreachable unsupported dependency chains, worlds, keywords, one-letter and upper-case names).
#[allow(clippy::too_many_lines)]
pub fn arb_contract() -> BoxedStrategy<GeneratedContract> {
    (
        proptest::sample::select(PKG_SEGMENTS),
        proptest::sample::select(PKG_SEGMENTS),
        0usize..=2,
        any::<bool>(),
        any::<bool>(),
    )
        .prop_flat_map(|(root_ns, root_pkg, dep_count, dep_has_unsupported, add_world)| {
            let iface_count = 1usize..=2;
            (
                Just(root_ns.to_string()),
                Just(root_pkg.to_string()),
                Just(dep_count),
                Just(dep_has_unsupported),
                Just(add_world),
                proptest::collection::vec(proptest::sample::select(PKG_SEGMENTS), dep_count * 2),
                iface_count.prop_flat_map(move |n| {
                    proptest::collection::vec(arb_interface(dep_count), n..=n)
                }),
            )
        })
        .prop_map(
            |(root_ns, root_pkg, dep_count, dep_has_unsupported, add_world, dep_segs, ifaces)| {
                let mut used_pkgs = BTreeSet::new();
                used_pkgs.insert(format!(
                    "{}:{}",
                    root_ns.to_ascii_lowercase(),
                    root_pkg.to_ascii_lowercase()
                ));
                let mut deps = Vec::new();
                for i in 0..dep_count {
                    let raw_ns = dep_segs[i * 2];
                    let raw_pkg = dep_segs[i * 2 + 1];
                    let mut ns = raw_ns.to_string();
                    let mut pkg = raw_pkg.to_string();
                    let key = format!("{}:{}", ns.to_ascii_lowercase(), pkg.to_ascii_lowercase());
                    if !used_pkgs.insert(key) {
                        ns = format!("dep{i}");
                        pkg = format!("pkg{i}");
                        used_pkgs.insert(format!("{ns}:{pkg}"));
                    }
                    let mut wit = format!(
                        "package {}:{}@0.1.0;\n\ninterface types {{\n  record shared-item {{\n    id: u32,\n    label: string,\n  }}\n  type shared-bytes = list<u8>;\n",
                        wit_escape(&ns),
                        wit_escape(&pkg),
                    );
                    if dep_has_unsupported {
                        wit.push_str(
                            "  resource hidden-res;\n  record hidden-wrap { h: hidden-res }\n  record hidden-outer { w: hidden-wrap }\n",
                        );
                    }
                    if i == 1
                        && dep_has_unsupported
                        && let Some(prev) = deps.first()
                    {
                        let prev: &GeneratedDep = prev;
                        let _ = writeln!(
                            wit,
                            "  use {}:{}/types@0.1.0.{{hidden-outer}};\n  record cross-hidden {{ o: hidden-outer }}",
                            wit_escape(&prev.namespace),
                            wit_escape(&prev.package),
                        );
                    }
                    wit.push_str("}\n");
                    deps.push(GeneratedDep {
                        dir_name: format!("d{i}"),
                        namespace: ns,
                        package: pkg,
                        wit,
                        exported_type: Some("shared-item".to_string()),
                    });
                }

                let mut root_wit = format!(
                    "package {}:{}@0.1.0;\n",
                    wit_escape(&root_ns),
                    wit_escape(&root_pkg),
                );
                let mut seen_ifaces = BTreeSet::new();
                let mut emitted_ifaces = Vec::new();
                for iface in ifaces {
                    if !seen_ifaces.insert(iface.name.to_ascii_lowercase()) {
                        continue;
                    }
                    emitted_ifaces.push(iface.name.clone());
                    let _ = writeln!(root_wit, "\ninterface {} {{", wit_escape(&iface.name));

                    let mut used_in_iface = BTreeSet::new();
                    let mut dep_aliases = Vec::new();
                    for (idx, dep) in deps.iter().enumerate() {
                        if let Some(ty) = &dep.exported_type {
                            let alias = format!("dep{idx}-item");
                            used_in_iface.insert(alias.to_ascii_lowercase());
                            let _ = writeln!(
                                root_wit,
                                "  use {}:{}/types@0.1.0.{{{} as {}}};",
                                wit_escape(&dep.namespace),
                                wit_escape(&dep.package),
                                wit_escape(ty),
                                wit_escape(&alias),
                            );
                            dep_aliases.push(alias);
                        }
                    }

                    let byte_alias = if iface.include_byte_alias {
                        used_in_iface.insert("byte-unit".to_string());
                        root_wit.push_str("  type byte-unit = u8;\n");
                        Some("byte-unit")
                    } else {
                        None
                    };

                    let mut local_names = Vec::new();
                    for (tname, decl) in &iface.types {
                        if !used_in_iface.insert(tname.to_ascii_lowercase()) {
                            continue;
                        }
                        let escaped = wit_escape(tname);
                        match decl {
                            TypeDeclSpec::Record(fields) if !fields.is_empty() => {
                                let _ = writeln!(root_wit, "  record {escaped} {{");
                                for (fname, fty) in fields {
                                    let _ = writeln!(
                                        root_wit,
                                        "    {}: {},",
                                        wit_escape(fname),
                                        render_type_spec(fty, &local_names, &dep_aliases, byte_alias),
                                    );
                                }
                                root_wit.push_str("  }\n");
                            }
                            TypeDeclSpec::Variant(cases) if !cases.is_empty() => {
                                let _ = writeln!(root_wit, "  variant {escaped} {{");
                                for (cname, cty) in cases {
                                    if let Some(ty) = cty {
                                        let _ = writeln!(
                                            root_wit,
                                            "    {}({}),",
                                            wit_escape(cname),
                                            render_type_spec(
                                                ty,
                                                &local_names,
                                                &dep_aliases,
                                                byte_alias
                                            ),
                                        );
                                    } else {
                                        let _ = writeln!(root_wit, "    {},", wit_escape(cname));
                                    }
                                }
                                root_wit.push_str("  }\n");
                            }
                            TypeDeclSpec::Enum(cases) if !cases.is_empty() => {
                                let _ = writeln!(root_wit, "  enum {escaped} {{");
                                for cname in cases {
                                    let _ = writeln!(root_wit, "    {},", wit_escape(cname));
                                }
                                root_wit.push_str("  }\n");
                            }
                            TypeDeclSpec::Flags(flags) if !flags.is_empty() => {
                                let _ = writeln!(root_wit, "  flags {escaped} {{");
                                for fname in flags {
                                    let _ = writeln!(root_wit, "    {},", wit_escape(fname));
                                }
                                root_wit.push_str("  }\n");
                            }
                            TypeDeclSpec::Alias(target) => {
                                let _ = writeln!(
                                    root_wit,
                                    "  type {escaped} = {};",
                                    render_type_spec(target, &local_names, &dep_aliases, byte_alias),
                                );
                            }
                            _ => continue,
                        }
                        local_names.push(tname.clone());
                    }

                    for func in &iface.functions {
                        if !used_in_iface.insert(func.name.to_ascii_lowercase()) {
                            continue;
                        }
                        let params: Vec<String> = func
                            .params
                            .iter()
                            .map(|(pname, pty)| {
                                format!(
                                    "{}: {}",
                                    wit_escape(pname),
                                    render_type_spec(pty, &local_names, &dep_aliases, byte_alias)
                                )
                            })
                            .collect();
                        let ret = func.result.as_ref().map_or_else(String::new, |r| {
                            format!(
                                " -> {}",
                                render_type_spec(r, &local_names, &dep_aliases, byte_alias)
                            )
                        });
                        let _ = writeln!(
                            root_wit,
                            "  {}: func({}){ret};",
                            wit_escape(&func.name),
                            params.join(", "),
                        );
                    }
                    root_wit.push_str("}\n");
                }

                if add_world && !emitted_ifaces.is_empty() && !dep_has_unsupported {
                    let world_name = if seen_ifaces.contains("app") {
                        "app-world"
                    } else {
                        "app"
                    };
                    let _ = writeln!(root_wit, "\nworld {world_name} {{");
                    for iface in &emitted_ifaces {
                        let _ = writeln!(root_wit, "  export {};", wit_escape(iface));
                    }
                    root_wit.push_str("}\n");
                }

                GeneratedContract {
                    namespace: root_ns,
                    package: root_pkg,
                    root_wit,
                    deps,
                }
            },
        )
        .boxed()
}

/// Produce a deterministic fixed-seed sample of `count` generated contracts for compile-matrix
/// tests (`W10-068`).
pub fn sample_contracts(seed: [u8; 32], count: usize) -> Vec<GeneratedContract> {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

    let strategy = arb_contract();
    let mut runner = TestRunner::new_with_rng(
        Config::default(),
        TestRng::from_seed(RngAlgorithm::ChaCha, &seed),
    );
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let tree = strategy.new_tree(&mut runner).unwrap();
        out.push(tree.current());
    }
    out
}
