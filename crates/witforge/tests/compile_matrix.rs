//! `W10-068` compile-and-round-trip matrix, fixed-seed property compile sample, and committed
//! example verification across Rust Edition 2021 and Edition 2024.
//!
//! Every matrix entry explicitly declares whether it compiles and round-trips or is refused by
//! `witforge check` with a specific diagnostic code, so an entry that silently stops generating
//! fails immediately.

use std::fs;
use std::path::Path;
use std::time::Instant;

use witforge::emit::{glue, json_schema, proto, rust};
use witforge::load;
use witforge::subset;

#[path = "support/propgen.rs"]
mod propgen;
mod support;

use support::{HarnessBatch, QualityChecks, RustEdition, ScratchDir, ensure_lock, workspace_root};

enum Expected {
    Compiles {
        pbjson: bool,
        roundtrip: &'static str,
    },
    CheckRefuses {
        code: &'static str,
        needle: &'static str,
    },
}

struct MatrixEntry {
    name: &'static str,
    deps: &'static [(&'static str, &'static str)],
    root_wit: String,
    expected: Expected,
}

fn run_matrix(tag: &str, entries: &[MatrixEntry]) {
    let stage = ScratchDir::new(tag);
    let mut compiling: Vec<(&str, std::path::PathBuf, bool, &'static str)> = Vec::new();
    let mut refused_count = 0usize;

    for entry in entries {
        let wit_root = stage.path().join(entry.name);
        let v01 = wit_root.join("v0.1");
        fs::create_dir_all(&v01).unwrap();
        for (dep_dir, dep_wit) in entry.deps {
            let dir = v01.join("deps").join(dep_dir);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("types.wit"), dep_wit).unwrap();
        }
        fs::write(v01.join("contract.wit"), &entry.root_wit).unwrap();

        let contract = load(&v01)
            .unwrap_or_else(|e| panic!("matrix entry `{}` failed to parse: {e:#}", entry.name));
        let diags = subset::check_diagnostics(&contract);

        match &entry.expected {
            Expected::Compiles { pbjson, roundtrip } => {
                assert!(
                    diags.is_empty(),
                    "matrix entry `{}` was expected to compile, but `check` refused it:\n{diags:#?}",
                    entry.name
                );
                ensure_lock(&v01);
                compiling.push((entry.name, wit_root, *pbjson, *roundtrip));
            }
            Expected::CheckRefuses { code, needle } => {
                refused_count += 1;
                assert!(
                    diags
                        .iter()
                        .any(|d| d.code == *code && d.message.contains(needle)),
                    "matrix entry `{}` was expected to fail `check` with {code} containing `{needle}`, got:\n{diags:#?}",
                    entry.name
                );
            }
        }
    }

    assert!(!compiling.is_empty(), "matrix had no compiling entries");
    assert!(refused_count > 0, "matrix had no refused entries");

    for edition in [RustEdition::E2021, RustEdition::E2024] {
        let mut batch = HarnessBatch::new(
            &format!("{tag}-{}", edition.as_str()),
            edition,
            QualityChecks::ALL,
        );
        for (name, wit_root, pbjson, roundtrip) in &compiling {
            batch.add_wit_root(name, wit_root, *pbjson, false, *roundtrip);
        }
        batch.verify_and_run(&format!("{tag} matrix"));
    }

    stage.finish();
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
    "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while", "abstract", "become", "box", "do", "final", "macro", "override",
    "priv", "try", "typeof", "union", "unsized", "virtual", "yield",
];

fn rust_keywords_wit() -> String {
    let fields: Vec<String> = RUST_KEYWORDS
        .iter()
        .map(|kw| format!("    {}: u32,", propgen::wit_escape(kw)))
        .collect();
    let (flags_a, flags_b) = RUST_KEYWORDS.split_at(26);
    let flags_a: Vec<String> = flags_a
        .iter()
        .map(|kw| format!("    {},", propgen::wit_escape(kw)))
        .collect();
    let flags_b: Vec<String> = flags_b
        .iter()
        .map(|kw| format!("    {},", propgen::wit_escape(kw)))
        .collect();

    format!(
        "package %type:gen@0.1.0;\n\n\
interface %type {{\n\
  use self:super/types@0.1.0.{{self-item}};\n\
  use crate:union/types@0.1.0.{{union-item}};\n\
  use example:gen/types@0.1.0.{{gen-item}};\n\
  use example:extern/types@0.1.0.{{extern-item}};\n\
  use example:%type/types@0.1.0.{{type-item}};\n\
  use example:match/types@0.1.0.{{match-item}};\n\n\
  record all-kw {{\n{fields}\n  }}\n\
  flags flags-a {{\n{flags_a}\n  }}\n\
  flags flags-b {{\n{flags_b}\n  }}\n\
  enum kw-enum {{\n\
    self, super, crate, gen, extern, union, %type, match, %async, await, try,\n\
  }}\n\
  enum upper-self-enum {{\n\
    SELF, ok,\n\
  }}\n\
  variant kw-var {{\n\
    self(string), super, crate, gen(bool), extern(u32), union(u32), %type, match,\n\
  }}\n\
  variant upper-self-var {{\n\
    SELF(u32), ok,\n\
  }}\n\
  record self {{ value: u32 }}\n\
  record super {{ value: u32 }}\n\
  record crate {{ value: u32 }}\n\
  record gen {{ value: u32 }}\n\
  record extern {{ value: u32 }}\n\
  record union {{ value: u32 }}\n\
  record dep-bag {{\n\
    s: self-item,\n\
    u: union-item,\n\
    g: gen-item,\n\
    e: extern-item,\n\
    t: type-item,\n\
    m: match-item,\n\
  }}\n\
}}\n\n\
interface fn-kw {{\n\
  use %type.{{all-kw, dep-bag, kw-var, upper-self-enum, upper-self-var}};\n\
  %type: func(\n\
    self: u32, super: u32, crate: u32, gen: u32, extern: u32, union: u32,\n\
    %type: u32, match: u32, %async: u32, await: u32, try: u32, where: u32,\n\
  ) -> kw-var;\n\
  gen: func(self: upper-self-var) -> upper-self-enum;\n\
  extern: func(union: dep-bag) -> all-kw;\n\
}}\n\n\
interface upper-self-iface {{ record SELF {{ value: u32 }} }}\n\
interface self {{ record item {{ id: u32 }} self: func(x: item) -> item; }}\n\
interface super {{ record item {{ id: u32 }} super: func(x: item) -> item; }}\n\
interface crate {{ record item {{ id: u32 }} crate: func(x: item) -> item; }}\n\
interface gen {{ record item {{ id: u32 }} gen: func(x: item) -> item; }}\n\
interface extern {{ record item {{ id: u32 }} extern: func(x: item) -> item; }}\n\
interface union {{ record item {{ id: u32 }} union: func(x: item) -> item; }}\n\
interface match {{ record item {{ id: u32 }} match: func(x: item) -> item; }}\n\
interface %async {{ record item {{ id: u32 }} %async: func(x: item) -> item; }}\n\
interface await {{ record item {{ id: u32 }} await: func(x: item) -> item; }}\n\
interface try {{ record item {{ id: u32 }} try: func(x: item) -> item; }}\n\
interface mod {{ record item {{ id: u32 }} mod: func(x: item) -> item; }}\n\
interface where {{ record item {{ id: u32 }} where: func(x: item) -> item; }}\n\n\
world pub {{\n\
  export %type;\n\
  export self;\n\
}}\n",
        fields = fields.join("\n"),
        flags_a = flags_a.join("\n"),
        flags_b = flags_b.join("\n"),
    )
}

fn rust_keywords_roundtrip() -> String {
    use std::fmt::Write as _;

    let plain_escape = |kw: &str| match kw {
        "crate" | "self" | "super" | "Self" => format!("{kw}_"),
        _ => format!("r#{kw}"),
    };
    let mut field_inits = String::new();
    for (i, kw) in RUST_KEYWORDS.iter().enumerate() {
        let _ = writeln!(field_inits, "        {}: {},", plain_escape(kw), i + 1);
    }
    let (first_flags, second_flags) = RUST_KEYWORDS.split_at(26);
    let mut first_flag_inits = String::new();
    for (i, kw) in first_flags.iter().enumerate() {
        let _ = writeln!(
            first_flag_inits,
            "        {}: {},",
            plain_escape(kw),
            i % 2 == 0
        );
    }
    let mut second_flag_inits = String::new();
    for (i, kw) in second_flags.iter().enumerate() {
        let _ = writeln!(
            second_flag_inits,
            "        {}: {},",
            plain_escape(kw),
            i % 2 == 1
        );
    }

    format!(
        r#"
    use ::prost::Message as _;
    use crate::rust_keywords::{{glue, plain, proto}};
    use plain::v0_1::r#type as t;

    let kw = t::AllKw {{
{field_inits}    }};
    let wire = glue::v0::r#type::all_kw::to_proto(&kw);
    let bytes = wire.encode_to_vec();
    let decoded = proto::r#type::r#gen::v0::TypeAllKw::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::r#type::all_kw::from_proto(decoded).unwrap(), kw);

    let fa = t::FlagsA {{
{first_flag_inits}    }};
    let wire = glue::v0::r#type::flags_a::to_proto(&fa);
    let bytes = wire.encode_to_vec();
    let decoded = proto::r#type::r#gen::v0::TypeFlagsA::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::r#type::flags_a::from_proto(decoded).unwrap(), fa);

    let fb = t::FlagsB {{
{second_flag_inits}    }};
    let wire = glue::v0::r#type::flags_b::to_proto(&fb);
    let bytes = wire.encode_to_vec();
    let decoded = proto::r#type::r#gen::v0::TypeFlagsB::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::r#type::flags_b::from_proto(decoded).unwrap(), fb);

    for v in [
        t::KwVar::Self_("s".to_string()),
        t::KwVar::Super,
        t::KwVar::Crate,
        t::KwVar::Gen(true),
        t::KwVar::Extern(2),
        t::KwVar::Union(3),
        t::KwVar::Type,
        t::KwVar::Match,
    ] {{
        let wire = glue::v0::r#type::kw_var::to_proto(&v);
        let bytes = wire.encode_to_vec();
        let decoded = proto::r#type::r#gen::v0::TypeKwVar::decode(bytes.as_slice()).unwrap();
        assert_eq!(glue::v0::r#type::kw_var::from_proto(decoded).unwrap(), v);
    }}

    let usv = t::UpperSelfVar::Self_(99);
    let wire = glue::v0::r#type::upper_self_var::to_proto(&usv);
    let bytes = wire.encode_to_vec();
    let decoded = proto::r#type::r#gen::v0::TypeUpperSelfVar::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::r#type::upper_self_var::from_proto(decoded).unwrap(), usv);

    let usr = plain::v0_1::upper_self_iface::Self_ {{ value: 7 }};
    let wire = glue::v0::upper_self_iface::self_::to_proto(&usr);
    let bytes = wire.encode_to_vec();
    let decoded = proto::r#type::r#gen::v0::UpperSelfIfaceSelf::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::upper_self_iface::self_::from_proto(decoded).unwrap(), usr);

    let bag = t::DepBag {{
        s: plain::witforge_deps::self_super::types::SelfItem {{ r#extern: 1, r#gen: true }},
        u: plain::witforge_deps::crate_union::types::UnionItem {{ r#union: 2 }},
        g: plain::witforge_deps::example_gen::types::GenItem {{ r#type: "g".to_string() }},
        e: plain::witforge_deps::example_extern::types::ExternItem {{ self_: 3 }},
        t: plain::witforge_deps::example_type::types::TypeItem {{ super_: 4 }},
        m: plain::witforge_deps::example_match::types::MatchItem {{ crate_: 5 }},
    }};
    let wire = glue::v0::r#type::dep_bag::to_proto(&bag);
    let bytes = wire.encode_to_vec();
    let decoded = proto::r#type::r#gen::v0::TypeDepBag::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::r#type::dep_bag::from_proto(decoded).unwrap(), bag);

    let req = glue::v0::fn_kw::functions::r#type::to_request(
        &1, &2, &3, &4, &5, &6, &7, &8, &9, &10, &11, &12,
    );
    let bytes = req.encode_to_vec();
    let decoded = proto::r#type::r#gen::v0::FnKwTypeRequest::decode(bytes.as_slice()).unwrap();
    assert_eq!(
        glue::v0::fn_kw::functions::r#type::from_request(decoded).unwrap(),
        (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)
    );
"#
    )
}

#[allow(clippy::too_many_lines)]
#[test]
fn name_matrix_compiles_and_roundtrips_in_editions_2021_and_2024() {
    let rust_kw_roundtrip: &'static str = Box::leak(rust_keywords_roundtrip().into_boxed_str());

    let entries = vec![
        // 1. One-letter words in every position (root pkg, dep pkg, interfaces, types, fields,
        //    enum/variant cases, flags, functions, params, world).
        MatrixEntry {
            name: "one_letter",
            deps: &[(
                "xy",
                "package x:y@0.1.0;\ninterface z { record t { u: u32 } v: func(w: t) -> t; }\n",
            )],
            root_wit: "\
package a:b@0.1.0;

interface c {
  use x:y/z@0.1.0.{t as d};
  record e { f: u32, g: d }
  enum h { i, j }
  variant k { l(u32), m }
  flags n { o, p }
  type q = list<u32>;
  r: func(s: e, t: h) -> k;
}

world w {
  import x:y/z@0.1.0;
  export c;
}
"
            .to_string(),
            expected: Expected::Compiles {
                pbjson: true,
                roundtrip: r"
    use ::prost::Message as _;
    use crate::one_letter::{glue, plain, proto};

    let item = plain::v0_1::c::E {
        f: 7,
        g: plain::witforge_deps::x_y::z::T { u: 9 },
    };
    let wire = glue::v0::c::e::to_proto(&item);
    let bytes = wire.encode_to_vec();
    let back = proto::a::b::v0::Ce::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::c::e::from_proto(back.clone()).unwrap(), item);
    let json = serde_json::to_string(&back).unwrap();
    let from_json: proto::a::b::v0::Ce = serde_json::from_str(&json).unwrap();
    assert_eq!(glue::v0::c::e::from_proto(from_json).unwrap(), item);

    let req = glue::v0::c::functions::r::to_request(&item, &plain::v0_1::c::H::I);
    let bytes = req.encode_to_vec();
    let decoded = proto::a::b::v0::CrRequest::decode(bytes.as_slice()).unwrap();
    assert_eq!(
        glue::v0::c::functions::r::from_request(decoded).unwrap(),
        (item, plain::v0_1::c::H::I)
    );
",
            },
        },
        // 2. Upper-case words in every position.
        MatrixEntry {
            name: "upper_case",
            deps: &[(
                "xml",
                "package XML:URL@0.1.0;\ninterface API { record HTTP-id { ID: u32, URL: string } }\n",
            )],
            root_wit: "\
package HTTP:API@0.1.0;

interface HTTP-api {
  use XML:URL/API@0.1.0.{HTTP-id};
  record URL-entry { HTTP-url: string, ID: u32, foo-BAR: u32, ref-id: HTTP-id }
  enum HTTP-code { HTTP, OK, TLS-v1 }
  variant IO-event { HTTP(URL-entry), EOF }
  flags IO-flags { READ, WRITE-all }
  get-URL: func(HTTP-id: u32, URL: string) -> IO-event;
}

interface API {
  record item { URL: string }
}

world HTTP-world {
  export HTTP-api;
}
"
            .to_string(),
            expected: Expected::Compiles {
                pbjson: true,
                roundtrip: r#"
    use ::prost::Message as _;
    use crate::upper_case::{glue, plain, proto};

    let entry = plain::v0_1::http_api::UrlEntry {
        http_url: "https://example.test".to_string(),
        id: 1,
        foo_bar: 2,
        ref_id: plain::witforge_deps::xml_url::api::HttpId {
            id: 3,
            url: "u".to_string(),
        },
    };
    let ev = plain::v0_1::http_api::IoEvent::Http(entry);
    let wire = glue::v0::http_api::io_event::to_proto(&ev);
    let bytes = wire.encode_to_vec();
    let back = proto::http::api::v0::HttpApiIoEvent::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::http_api::io_event::from_proto(back.clone()).unwrap(), ev);
    let json = serde_json::to_string(&back).unwrap();
    let from_json: proto::http::api::v0::HttpApiIoEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(glue::v0::http_api::io_event::from_proto(from_json).unwrap(), ev);
"#,
            },
        },
        // 3. Digits in every position.
        MatrixEntry {
            name: "digits",
            deps: &[(
                "d1",
                "package d1:p2@0.1.0;\ninterface i3 { record r4 { f5: u32, x1y2: string } }\n",
            )],
            root_wit: "\
package v2:api3@0.1.0;

interface api2 {
  use d1:p2/i3@0.1.0.{r4};
  record item2 { a1: u32, foo2-bar: string, x1y2: bool, dep4: r4 }
  enum code2 { ok2, err404 }
  variant ev2 { v1(item2), v2 }
  flags fl2 { bit0, bit1 }
  call2: func(p1: item2, a2: u32) -> ev2;
}

world w2 {
  export api2;
}
"
            .to_string(),
            expected: Expected::Compiles {
                pbjson: true,
                roundtrip: r#"
    use ::prost::Message as _;
    use crate::digits::{glue, plain, proto};

    let item = plain::v0_1::api2::Item2 {
        a1: 10,
        foo2_bar: "two".to_string(),
        x1y2: true,
        dep4: plain::witforge_deps::d1_p2::i3::R4 {
            f5: 5,
            x1y2: "dep".to_string(),
        },
    };
    let ev = plain::v0_1::api2::Ev2::V1(item);
    let wire = glue::v0::api2::ev2::to_proto(&ev);
    let bytes = wire.encode_to_vec();
    let back = proto::v2::api3::v0::Api2Ev2::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::api2::ev2::from_proto(back.clone()).unwrap(), ev);
    let json = serde_json::to_string(&back).unwrap();
    let from_json: proto::v2::api3::v0::Api2Ev2 = serde_json::from_str(&json).unwrap();
    assert_eq!(glue::v0::api2::ev2::from_proto(from_json).unwrap(), ev);
"#,
            },
        },
        // 4. Every Rust keyword (including non-raw `self`/`super`/`crate`/`Self`, Edition 2024
        //    `gen`, weak `union`, `extern`) across every position.
        MatrixEntry {
            name: "rust_keywords",
            deps: &[
                (
                    "d-self",
                    "package self:super@0.1.0;\ninterface types { record self-item { extern: u32, gen: bool } }\n",
                ),
                (
                    "d-crate",
                    "package crate:union@0.1.0;\ninterface types { record union-item { union: u32 } }\n",
                ),
                (
                    "d-gen",
                    "package example:gen@0.1.0;\ninterface types { record gen-item { %type: string } }\n",
                ),
                (
                    "d-extern",
                    "package example:extern@0.1.0;\ninterface types { record extern-item { self: u32 } }\n",
                ),
                (
                    "d-type",
                    "package example:%type@0.1.0;\ninterface types { record type-item { super: u32 } }\n",
                ),
                (
                    "d-match",
                    "package example:match@0.1.0;\ninterface types { record match-item { crate: u32 } }\n",
                ),
            ],
            root_wit: rust_keywords_wit(),
            expected: Expected::Compiles {
                pbjson: false,
                roundtrip: rust_kw_roundtrip,
            },
        },
        // 5. Every proto3 keyword across package segments, interfaces, types, fields, cases,
        //    flags, functions, and params.
        MatrixEntry {
            name: "proto_keywords",
            deps: &[(
                "opt-pkg",
                "package %option:%package@0.1.0;\n\
                 interface %import {\n\
                   record message { syntax: string, repeated: u32, optional: bool }\n\
                 }\n",
            )],
            root_wit: "\
package service:message@0.1.0;

interface service {
  use %option:%package/%import@0.1.0.{message as dep-msg};
  record %option {
    service: u32,
    message: string,
    %option: bool,
    %enum: u32,
    %package: u32,
    %import: u32,
    syntax: u32,
    repeated: u32,
    optional: u32,
    oneof: u32,
    %map: u32,
    reserved: u32,
    rpc: u32,
    returns: u32,
    %stream: u32,
    weak: u32,
    public: u32,
    extensions: u32,
    to: u32,
    max: u32,
    required: u32,
    %bool: bool,
    bytes: list<u8>,
    %string: string,
    double: f64,
    float: f32,
    int32: s32,
    int64: s64,
    uint32: u32,
    uint64: u64,
    sint32: s32,
    sint64: s64,
    fixed32: u32,
    fixed64: u64,
    sfixed32: s32,
    sfixed64: s64,
    true: bool,
    false: bool,
    dep: dep-msg,
  }
  enum %enum { service, message, %option, oneof, %map, reserved, rpc, returns }
  variant oneof { service(%option), message(dep-msg), %import, syntax }
  flags reserved { repeated, optional, weak, public, extensions, to, max, required }
  rpc: func(service: %option, returns: %enum) -> oneof;
}

world %package {
  export service;
}
"
            .to_string(),
            expected: Expected::Compiles {
                pbjson: true,
                roundtrip: r#"
    use ::prost::Message as _;
    use crate::proto_keywords::{glue, plain, proto};

    let opt = plain::v0_1::service::Option {
        service: 1,
        message: "m".to_string(),
        option: true,
        r#enum: 2,
        package: 3,
        import: 4,
        syntax: 5,
        repeated: 6,
        optional: 7,
        oneof: 8,
        map: 9,
        reserved: 10,
        rpc: 11,
        returns: 12,
        stream: 13,
        weak: 14,
        public: 15,
        extensions: 16,
        to: 17,
        max: 18,
        required: 19,
        bool: true,
        bytes: vec![1, 2, 3],
        string: "s".to_string(),
        double: 1.5,
        float: 2.5,
        int32: -1,
        int64: -2,
        uint32: 3,
        uint64: 4,
        sint32: -5,
        sint64: -6,
        fixed32: 7,
        fixed64: 8,
        sfixed32: -9,
        sfixed64: -10,
        r#true: true,
        r#false: false,
        dep: plain::witforge_deps::option_package::import::Message {
            syntax: "proto3".to_string(),
            repeated: 1,
            optional: false,
        },
    };
    let oneof = plain::v0_1::service::Oneof::Service(opt);
    let wire = glue::v0::service::oneof::to_proto(&oneof);
    let bytes = wire.encode_to_vec();
    let back = proto::service::message::v0::ServiceOneof::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::service::oneof::from_proto(back.clone()).unwrap(), oneof);
    let json = serde_json::to_string(&back).unwrap();
    let from_json: proto::service::message::v0::ServiceOneof = serde_json::from_str(&json).unwrap();
    assert_eq!(glue::v0::service::oneof::from_proto(from_json).unwrap(), oneof);
"#,
            },
        },
        // 6. Type-only interface whose type shares its own PascalCase name (`W10-074`).
        MatrixEntry {
            name: "type_only_same_name",
            deps: &[],
            root_wit: "package ex:err@0.1.0;\ninterface error { enum error { a, b } }\n".to_string(),
            expected: Expected::Compiles {
                pbjson: true,
                roundtrip: r"
    use ::prost::Message as _;
    use crate::type_only_same_name::{glue, plain, proto};

    let err = plain::v0_1::error::Error::A;
    let wire = glue::v0::error::error::to_proto(&err);
    assert_eq!(glue::v0::error::error::from_proto(wire).unwrap(), err);
    let _ = proto::v0::ErrorError::A.as_str_name();
",
            },
        },
        // 7-14. Refused name matrix entries — each asserts `check` refuses with the exact code.
        MatrixEntry {
            name: "refuse_prost_case_clash",
            deps: &[],
            root_wit: "package ex:c@0.1.0;\ninterface api-x { record r { a: u32 } }\ninterface api { record xr { a: u32 } }\n".to_string(),
            expected: Expected::CheckRefuses {
                code: "WF1003",
                needle: "ApiXr",
            },
        },
        MatrixEntry {
            name: "refuse_request_clash",
            deps: &[],
            root_wit: "package ex:c@0.1.0;\ninterface types { record get-request { id: u32 } get: func() -> get-request; }\n".to_string(),
            expected: Expected::CheckRefuses {
                code: "WF1003",
                needle: "TypesGetRequest",
            },
        },
        MatrixEntry {
            name: "refuse_functions_mod",
            deps: &[],
            root_wit: "package ex:c@0.1.0;\ninterface types { record functions { id: u32 } }\n".to_string(),
            expected: Expected::CheckRefuses {
                code: "WF1003",
                needle: "reserved glue module `functions`",
            },
        },
        MatrixEntry {
            name: "refuse_enum_unspecified",
            deps: &[],
            root_wit: "package ex:c@0.1.0;\ninterface types { enum e { unspecified, ok } }\n".to_string(),
            expected: Expected::CheckRefuses {
                code: "WF1003",
                needle: "UNSPECIFIED",
            },
        },
        MatrixEntry {
            name: "refuse_variant_witforge_value",
            deps: &[],
            root_wit: "package ex:c@0.1.0;\ninterface types { variant v { witforge-value, ok } }\n".to_string(),
            expected: Expected::CheckRefuses {
                code: "WF1003",
                needle: "witforge_value",
            },
        },
        MatrixEntry {
            name: "refuse_accessor_clash",
            deps: &[],
            root_wit: "package ex:c@0.1.0;\ninterface types { enum mode { a, b } record cfg { mode: mode, set-mode: bool } }\n".to_string(),
            expected: Expected::CheckRefuses {
                code: "WF1003",
                needle: "set_mode",
            },
        },
        MatrixEntry {
            name: "refuse_type_vs_trait",
            deps: &[],
            root_wit: "package ex:c@0.1.0;\ninterface api { record api { id: u32 } ping: func(); }\n".to_string(),
            expected: Expected::CheckRefuses {
                code: "WF1003",
                needle: "Api",
            },
        },
        MatrixEntry {
            name: "refuse_dep_mod_clash",
            deps: &[
                ("a", "package my-ns:dep@0.1.0;\ninterface types { record x { id: u32 } }\n"),
                ("b", "package my:ns-dep@0.1.0;\ninterface types { record y { id: u32 } }\n"),
            ],
            root_wit: "package ex:c@0.1.0;\ninterface types { record ok { id: u32 } }\n".to_string(),
            expected: Expected::CheckRefuses {
                code: "WF1003",
                needle: "my_ns_dep",
            },
        },
    ];

    let started = Instant::now();
    run_matrix("nmat", &entries);
    eprintln!(
        "name_matrix ({} entries, editions 2021+2024) finished in {:.2}s",
        entries.len(),
        started.elapsed().as_secs_f64()
    );
}

#[allow(clippy::too_many_lines)]
#[test]
fn shape_matrix_compiles_and_roundtrips_in_editions_2021_and_2024() {
    let deep_list = format!("{}u8{}", "list<".repeat(40), ">".repeat(40));

    let entries = vec![
        // 1. Every kind in every position (record, variant with/without payload, enum, flags,
        //    alias, named list/option/result, inline list/option/result, all 4 result forms).
        MatrixEntry {
            name: "every_kind_pos",
            deps: &[],
            root_wit: "\
package ex:shapes@0.1.0;

interface types {
  record leaf { id: u32, name: string }
  enum status { ready, done }
  flags perms { read, write }
  variant choice { empty, with-leaf(leaf), with-status(status), with-perms(perms) }
  type leaf-alias = leaf;
  type named-list = list<leaf>;
  type named-opt = option<leaf>;
  type named-res-both = result<leaf, string>;
  type named-res-ok = result<leaf>;
  type named-res-err = result<_, status>;
  type named-res-bare = result;

  variant all-payloads {
    p-rec(leaf),
    p-var(choice),
    p-enum(status),
    p-flags(perms),
    p-alias(leaf-alias),
    p-list(list<leaf>),
    p-opt(option<leaf>),
    p-res-both(result<leaf, status>),
    p-res-ok(result<perms>),
    p-res-err(result<_, string>),
    p-res-bare(result),
    p-named-list(named-list),
    p-named-opt(named-opt),
    p-named-res(named-res-both),
    p-unit,
  }

  record all-fields {
    f-rec: leaf,
    f-var: choice,
    f-enum: status,
    f-flags: perms,
    f-alias: leaf-alias,
    f-nlist: named-list,
    f-nopt: named-opt,
    f-nres-both: named-res-both,
    f-nres-ok: named-res-ok,
    f-nres-err: named-res-err,
    f-nres-bare: named-res-bare,
    f-list-rec: list<leaf>,
    f-list-var: list<choice>,
    f-list-enum: list<status>,
    f-list-flags: list<perms>,
    f-list-alias: list<leaf-alias>,
    f-opt-rec: option<leaf>,
    f-opt-var: option<choice>,
    f-opt-enum: option<status>,
    f-opt-flags: option<perms>,
    f-opt-alias: option<leaf-alias>,
    f-res-rec-var: result<leaf, choice>,
    f-res-list-opt: result<list<status>, option<perms>>,
    f-res-ok: result<leaf>,
    f-res-err: result<_, string>,
    f-res-bare: result,
    f-payloads: all-payloads,
  }

  fn-void: func();
  fn-all: func(
    a: leaf, b: choice, c: status, d: perms, e: leaf-alias,
    f: named-list, g: named-opt, h: named-res-both,
    i: result<u32>, j: result<_, string>, k: result,
  ) -> all-fields;
  fn-bare-res: func() -> result;
  fn-ok-res: func() -> result<leaf>;
  fn-err-res: func() -> result<_, status>;
}
"
            .to_string(),
            expected: Expected::Compiles {
                pbjson: true,
                roundtrip: r#"
    use ::prost::Message as _;
    use crate::every_kind_pos::{glue, plain, proto};
    use plain::v0_1::types::{AllFields, AllPayloads, Choice, Leaf, Perms, Status};

    let leaf = Leaf { id: 1, name: "n".to_string() };
    let perms = Perms { read: true, write: false };
    let choice = Choice::WithLeaf(leaf.clone());
    let rec = AllFields {
        f_rec: leaf.clone(),
        f_var: choice.clone(),
        f_enum: Status::Ready,
        f_flags: perms.clone(),
        f_alias: leaf.clone(),
        f_nlist: vec![leaf.clone()],
        f_nopt: Some(leaf.clone()),
        f_nres_both: Ok(leaf.clone()),
        f_nres_ok: Err(()),
        f_nres_err: Ok(()),
        f_nres_bare: Err(()),
        f_list_rec: vec![leaf.clone()],
        f_list_var: vec![Choice::Empty, choice.clone()],
        f_list_enum: vec![Status::Ready, Status::Done],
        f_list_flags: vec![perms.clone()],
        f_list_alias: vec![leaf.clone()],
        f_opt_rec: Some(leaf.clone()),
        f_opt_var: Some(choice.clone()),
        f_opt_enum: Some(Status::Done),
        f_opt_flags: Some(perms.clone()),
        f_opt_alias: None,
        f_res_rec_var: Err(choice.clone()),
        f_res_list_opt: Ok(vec![Status::Ready]),
        f_res_ok: Ok(leaf.clone()),
        f_res_err: Err("oops".to_string()),
        f_res_bare: Ok(()),
        f_payloads: AllPayloads::PResBare(Ok(())),
    };
    let wire = glue::v0::types::all_fields::to_proto(&rec);
    let bytes = wire.encode_to_vec();
    let back = proto::v0::TypesAllFields::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::types::all_fields::from_proto(back.clone()).unwrap(), rec);
    let json = serde_json::to_string(&back).unwrap();
    let from_json: proto::v0::TypesAllFields = serde_json::from_str(&json).unwrap();
    assert_eq!(glue::v0::types::all_fields::from_proto(from_json).unwrap(), rec);

    let resp = glue::v0::types::functions::fn_bare_res::to_response(&Err(()));
    let bytes = resp.encode_to_vec();
    let back = proto::v0::TypesFnBareResResponse::decode(bytes.as_slice()).unwrap();
    assert_eq!(
        glue::v0::types::functions::fn_bare_res::from_response(back).unwrap(),
        Err(())
    );
"#,
            },
        },
        // 2. Three levels of nesting, both byte-list spellings (`list<u8>` and `type byte = u8;
        //    list<byte>`), and unreferenced named structural types.
        MatrixEntry {
            name: "nesting_bytes_unreferenced",
            deps: &[],
            root_wit: "\
package ex:nested@0.1.0;

interface types {
  record item { id: u16 }
  type byte = u8;
  type b2 = byte;
  type raw-bytes = list<u8>;
  type aliased-bytes = list<b2>;

  // Unreferenced named structural types (`W10-060`).
  type unused-cube = list<list<list<string>>>;
  type unused-opt3 = option<option<option<u16>>>;
  type unused-res = result<list<list<u32>>, option<option<string>>>;
  type unused-bytes3 = list<list<list<b2>>>;

  record deep {
    cube-u16: list<list<list<u16>>>,
    cube-item: list<list<list<item>>>,
    cube-bytes-raw: list<list<list<u8>>>,
    cube-bytes-alias: list<list<list<b2>>>,
    opt3-u32: option<option<option<u32>>>,
    opt3-item: option<option<option<item>>>,
    mixed-a: list<option<result<list<u16>, option<string>>>>,
    mixed-b: option<list<option<item>>>,
    mixed-c: result<option<list<u32>>, list<option<string>>>,
    b-raw: raw-bytes,
    b-alias: aliased-bytes,
  }
}
"
            .to_string(),
            expected: Expected::Compiles {
                pbjson: true,
                roundtrip: r#"
    use ::prost::Message as _;
    use crate::nesting_bytes_unreferenced::{glue, plain, proto};
    use plain::v0_1::types::{Deep, Item};

    let d = Deep {
        cube_u16: vec![vec![vec![1, 2], vec![]]],
        cube_item: vec![vec![vec![Item { id: 3 }]]],
        cube_bytes_raw: vec![vec![vec![4, 5]]],
        cube_bytes_alias: vec![vec![vec![6, 7]]],
        opt3_u32: Some(Some(None)),
        opt3_item: Some(Some(Some(Item { id: 8 }))),
        mixed_a: vec![Some(Ok(vec![9, 10])), Some(Err(None)), None],
        mixed_b: Some(vec![Some(Item { id: 11 }), None]),
        mixed_c: Ok(Some(vec![12, 13])),
        b_raw: vec![14, 15],
        b_alias: vec![16, 17],
    };
    let wire = glue::v0::types::deep::to_proto(&d);
    let bytes = wire.encode_to_vec();
    let back = proto::v0::TypesDeep::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::types::deep::from_proto(back.clone()).unwrap(), d);
    let json = serde_json::to_string(&back).unwrap();
    let from_json: proto::v0::TypesDeep = serde_json::from_str(&json).unwrap();
    assert_eq!(glue::v0::types::deep::from_proto(from_json).unwrap(), d);

    // Unreferenced named structural types round-trip directly.
    let cube = vec![vec![vec!["a".to_string()]]];
    let w = glue::v0::types::unused_cube::to_proto(&cube);
    assert_eq!(glue::v0::types::unused_cube::from_proto(w).unwrap(), cube);
    let opt3 = Some(Some(None));
    let w = glue::v0::types::unused_opt3::to_proto(&opt3);
    assert_eq!(glue::v0::types::unused_opt3::from_proto(w).unwrap(), opt3);
    let ures: Result<Vec<Vec<u32>>, Option<Option<String>>> = Err(Some(None));
    let w = glue::v0::types::unused_res::to_proto(&ures);
    assert_eq!(glue::v0::types::unused_res::from_proto(w).unwrap(), ures);
    let ub3 = vec![vec![vec![1_u8, 2_u8]]];
    let w = glue::v0::types::unused_bytes3::to_proto(&ub3);
    assert_eq!(glue::v0::types::unused_bytes3::from_proto(w).unwrap(), ub3);
"#,
            },
        },
        // 3. Two-hop dependency-declared types, byte-list aliases, 3-level nested structural
        //    types, unreferenced dependency types, functions, and world.
        MatrixEntry {
            name: "dep_shapes",
            deps: &[
                (
                    "leaf",
                    "package ex:leaf@0.1.0;\n\
                     interface types {\n\
                       record atom { id: u16 }\n\
                       type byte = u8;\n\
                       type blob = list<byte>;\n\
                       type cube = list<list<list<atom>>>;\n\
                       type opt3 = option<option<option<u32>>>;\n\
                       type unused-dep = list<option<string>>;\n\
                     }\n",
                ),
                (
                    "mid",
                    "package ex:mid@0.1.0;\n\
                     interface types {\n\
                       use ex:leaf/types@0.1.0.{atom, blob, cube, opt3};\n\
                       enum tag { a, b }\n\
                       flags bits { x, y }\n\
                       variant sum { atom(atom), blob(blob), none }\n\
                       record wrap { c: cube, o: opt3, s: sum, t: tag, b: bits }\n\
                       echo: func(w: wrap) -> wrap;\n\
                     }\n",
                ),
            ],
            root_wit: "\
package ex:app@0.1.0;

interface api {
  use ex:mid/types@0.1.0.{wrap};
  record env { w: wrap, list-w: list<option<wrap>> }
  send: func(e: env) -> result<env, string>;
}

world app {
  import ex:mid/types@0.1.0;
  export api;
}
"
            .to_string(),
            expected: Expected::Compiles {
                pbjson: true,
                roundtrip: r#"
    use ::prost::Message as _;
    use crate::dep_shapes::{glue, plain, proto};

    let atom = plain::witforge_deps::ex_leaf::types::Atom { id: 7 };
    let wrap = plain::witforge_deps::ex_mid::types::Wrap {
        c: vec![vec![vec![atom.clone()]]],
        o: Some(Some(Some(42))),
        s: plain::witforge_deps::ex_mid::types::Sum::Blob(vec![1, 2, 3]),
        t: plain::witforge_deps::ex_mid::types::Tag::B,
        b: plain::witforge_deps::ex_mid::types::Bits { x: true, y: false },
    };
    let env = plain::v0_1::api::Env {
        w: wrap.clone(),
        list_w: vec![Some(wrap), None],
    };
    let wire = glue::v0::api::env::to_proto(&env);
    let bytes = wire.encode_to_vec();
    let back = proto::ex::app::v0::ApiEnv::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::api::env::from_proto(back.clone()).unwrap(), env);
    let json = serde_json::to_string(&back).unwrap();
    let from_json: proto::ex::app::v0::ApiEnv = serde_json::from_str(&json).unwrap();
    assert_eq!(glue::v0::api::env::from_proto(from_json).unwrap(), env);

    let unused = vec![Some("u".to_string()), None];
    let w = glue::witforge_deps::ex_leaf::types::unused_dep::to_proto(&unused);
    assert_eq!(
        glue::witforge_deps::ex_leaf::types::unused_dep::from_proto(w).unwrap(),
        unused
    );
"#,
            },
        },
        // 4-6. Refused shape entries.
        MatrixEntry {
            name: "refuse_nesting_limit",
            deps: &[],
            root_wit: format!(
                "package ex:lim@0.1.0;\ninterface types {{ record r {{ v: {deep_list} }} }}\n"
            ),
            expected: Expected::CheckRefuses {
                code: "WF1001",
                needle: "nesting",
            },
        },
        MatrixEntry {
            name: "refuse_char_primitive",
            deps: &[],
            root_wit: "package ex:ch@0.1.0;\ninterface types { record r { c: char } }\n".to_string(),
            expected: Expected::CheckRefuses {
                code: "WF1001",
                needle: "char",
            },
        },
        MatrixEntry {
            name: "refuse_wrapper_collision",
            deps: &[],
            root_wit: "package ex:wc@0.1.0;\ninterface witforge { record list-of-string { v: string } }\ninterface api { type m = list<list<string>>; }\n".to_string(),
            expected: Expected::CheckRefuses {
                code: "WF1003",
                needle: "WitforgeListOfString",
            },
        },
    ];

    let started = Instant::now();
    run_matrix("smat", &entries);
    eprintln!(
        "shape_matrix ({} entries, editions 2021+2024) finished in {:.2}s",
        entries.len(),
        started.elapsed().as_secs_f64()
    );
}

#[test]
fn property_sample_compiles_in_editions_2021_and_2024() {
    const FIXED_SEED: [u8; 32] = [
        0x57, 0x46, 0x06, 0x08, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB,
        0xCC, 0xDD, 0xEE, 0xFF, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90, 0xA0, 0xB0,
        0xC0, 0xD0,
    ];
    const SAMPLE_COUNT: usize = 24;

    let started = Instant::now();
    let stage = ScratchDir::new("psamp");
    let mut passing: Vec<(String, std::path::PathBuf)> = Vec::new();

    for (idx, generated) in propgen::sample_contracts(FIXED_SEED, SAMPLE_COUNT)
        .into_iter()
        .enumerate()
    {
        let wit_root = stage.path().join(format!("c{idx}"));
        let v01 = wit_root.join("v0.1");
        generated.write_to(&v01);

        let Ok(contract) = load(&v01) else {
            continue;
        };
        if subset::check_diagnostics(&contract).is_empty() {
            ensure_lock(&v01);
            passing.push((format!("p{idx}"), wit_root));
        }
    }

    assert!(
        passing.len() >= 8,
        "expected >= 8 passing contracts in fixed-seed sample of {SAMPLE_COUNT}, got {}",
        passing.len()
    );

    for edition in [RustEdition::E2021, RustEdition::E2024] {
        let mut batch = HarnessBatch::new(
            &format!("prop-{}", edition.as_str()),
            edition,
            QualityChecks::ALL,
        );
        for (mod_name, wit_root) in &passing {
            // Confirm `GlueError` and module roots link cleanly without dead-code issues.
            let body = format!(
                "    let _err = crate::{mod_name}::glue::GlueError::MissingField {{ field: \"probe\" }};"
            );
            batch.add_wit_root(mod_name, wit_root, false, false, body);
        }
        batch.verify_and_run("property sample");
    }

    stage.finish();
    eprintln!(
        "property_sample ({} passing of {SAMPLE_COUNT}, editions 2021+2024) finished in {:.2}s",
        passing.len(),
        started.elapsed().as_secs_f64()
    );
}

#[allow(clippy::too_many_lines)]
#[test]
fn examples_generated_code_compiles_and_roundtrips() {
    let started = Instant::now();
    let root = workspace_root();

    // 1. Verify committed examples have zero drift against their WIT sources.
    for example in ["single", "evolved"] {
        let ex_dir = root.join("examples").join(example);
        let wit_root = ex_dir.join("wit");
        let gen_dir = ex_dir.join("generated");
        assert_eq!(
            rust::check(&wit_root, &gen_dir.join("rust")).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            proto::check(&wit_root, &gen_dir.join("proto")).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            json_schema::check(&wit_root, &gen_dir.join("schema")).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            glue::check(
                &wit_root,
                &gen_dir.join("glue"),
                "crate::generated::rust",
                "crate::generated::proto",
            )
            .unwrap(),
            Vec::<String>::new()
        );
    }

    // 2. Compile committed `examples/single/generated/{rust,proto,glue}` in Editions 2021 & 2024.
    let single_gen = root.join("examples/single/generated");
    let single_roundtrip = r#"
    use ::prost::Message as _;
    use crate::generated::{glue, proto, rust as plain};
    use plain::v0_1::types::{Event, Item, Options, Status};

    assert_eq!(plain::worlds::v0_1::WORLDS.len(), 1);
    assert_eq!(plain::worlds::v0_1::WORLDS[0].name, "ingestion");

    let item = Item {
        id: 42,
        name: "sensor-a".to_string(),
        tags: vec!["prod".to_string(), "eu".to_string()],
        note: Some("calibrated".to_string()),
    };
    let wire = glue::v0::types::item::to_proto(&item);
    let bytes = wire.encode_to_vec();
    let back = proto::v0::TypesItem::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::types::item::from_proto(back.clone()).unwrap(), item);
    let json = serde_json::to_string(&back).unwrap();
    let from_json: proto::v0::TypesItem = serde_json::from_str(&json).unwrap();
    assert_eq!(glue::v0::types::item::from_proto(from_json).unwrap(), item);

    let opts = Options { strict: true, trace: false };
    let wire = glue::v0::types::options::to_proto(&opts);
    let bytes = wire.encode_to_vec();
    let back = proto::v0::TypesOptions::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::types::options::from_proto(back).unwrap(), opts);

    for event in [
        Event::Started,
        Event::Item(item.clone()),
        Event::Finished(Status::Accepted),
    ] {
        let wire = glue::v0::types::event::to_proto(&event);
        let bytes = wire.encode_to_vec();
        let back = proto::v0::TypesEvent::decode(bytes.as_slice()).unwrap();
        assert_eq!(glue::v0::types::event::from_proto(back.clone()).unwrap(), event);
        let json = serde_json::to_string(&back).unwrap();
        let from_json: proto::v0::TypesEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(glue::v0::types::event::from_proto(from_json).unwrap(), event);
    }

    assert_eq!(
        glue::v0::ingest::functions::submit::METHOD,
        proto::v0::IngestMethod::Submit
    );
    let req = glue::v0::ingest::functions::submit::to_request(&item);
    let bytes = req.encode_to_vec();
    let back = proto::v0::IngestSubmitRequest::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::ingest::functions::submit::from_request(back).unwrap(), item);

    let resp = glue::v0::ingest::functions::submit::to_response(&Event::Finished(Status::Rejected));
    let bytes = resp.encode_to_vec();
    let back = proto::v0::IngestSubmitResponse::decode(bytes.as_slice()).unwrap();
    assert_eq!(
        glue::v0::ingest::functions::submit::from_response(back).unwrap(),
        Event::Finished(Status::Rejected)
    );
"#;

    for edition in [RustEdition::E2021, RustEdition::E2024] {
        let mut batch = HarnessBatch::new(
            &format!("ex1-{}", edition.as_str()),
            edition,
            QualityChecks::ALL,
        );
        batch.add_generated_dirs(
            "generated",
            "rust",
            "proto",
            "glue",
            &single_gen.join("rust"),
            &single_gen.join("proto"),
            &single_gen.join("glue"),
            true,
            single_roundtrip,
        );
        batch.verify_and_run("examples/single");
    }

    // 3. Compile committed `examples/evolved/generated/{rust,proto,glue}` in Editions 2021 & 2024.
    let evolved_gen = root.join("examples/evolved/generated");
    let evolved_roundtrip = r#"
    use ::prost::Message as _;
    use crate::generated::{glue, proto, rust as plain};

    assert_eq!(plain::worlds::v0_1::WORLDS.len(), 1);
    assert_eq!(plain::worlds::v0_2::WORLDS.len(), 1);

    let v1 = plain::v0_1::types::Task {
        id: 1,
        title: "ship 0.6.0".to_string(),
        done: false,
    };
    let upgraded = plain::convert::v0_1_to_v0_2::types::task(&v1).unwrap();
    assert_eq!(upgraded.id, 1);
    assert_eq!(upgraded.title, "ship 0.6.0");
    assert!(!upgraded.done);
    assert_eq!(upgraded.priority, None);
    assert!(upgraded.tags.is_empty());

    let v2 = plain::v0_2::types::Task {
        id: 2,
        title: "verify matrix".to_string(),
        done: true,
        priority: Some(3),
        tags: vec!["ci".to_string(), "rust".to_string()],
    };
    let downgraded = plain::convert::v0_2_to_v0_1::types::task(&v2).unwrap();
    assert_eq!(
        downgraded,
        plain::v0_1::types::Task {
            id: 2,
            title: "verify matrix".to_string(),
            done: true,
        }
    );

    let wire = glue::v0::types::task::to_proto(&v2);
    let bytes = wire.encode_to_vec();
    let back = proto::v0::TypesTask::decode(bytes.as_slice()).unwrap();
    assert_eq!(glue::v0::types::task::from_proto(back.clone()).unwrap(), v2);
    let json = serde_json::to_string(&back).unwrap();
    let from_json: proto::v0::TypesTask = serde_json::from_str(&json).unwrap();
    assert_eq!(glue::v0::types::task::from_proto(from_json).unwrap(), v2);

    assert_eq!(
        glue::v0::service::functions::create::METHOD,
        proto::v0::ServiceMethod::Create
    );
    let req = glue::v0::service::functions::create::to_request(&"demo".to_string());
    let bytes = req.encode_to_vec();
    let back = proto::v0::ServiceCreateRequest::decode(bytes.as_slice()).unwrap();
    assert_eq!(
        glue::v0::service::functions::create::from_request(back).unwrap(),
        "demo"
    );
    let resp = glue::v0::service::functions::create::to_response(&v2);
    let bytes = resp.encode_to_vec();
    let back = proto::v0::ServiceCreateResponse::decode(bytes.as_slice()).unwrap();
    assert_eq!(
        glue::v0::service::functions::create::from_response(back).unwrap(),
        v2
    );
"#;

    for edition in [RustEdition::E2021, RustEdition::E2024] {
        let mut batch = HarnessBatch::new(
            &format!("ex2-{}", edition.as_str()),
            edition,
            QualityChecks::ALL,
        );
        batch.add_generated_dirs(
            "generated",
            "rust",
            "proto",
            "glue",
            &evolved_gen.join("rust"),
            &evolved_gen.join("proto"),
            &evolved_gen.join("glue"),
            true,
            evolved_roundtrip,
        );
        batch.verify_and_run("examples/evolved");
    }

    eprintln!(
        "examples_generated_code (single + evolved, editions 2021+2024) finished in {:.2}s",
        started.elapsed().as_secs_f64()
    );
}

#[test]
fn scratch_dir_honours_tmpdir_and_keeps_on_failure() {
    let res = std::panic::catch_unwind(|| {
        let scratch = ScratchDir::new("keep-check");
        assert!(
            scratch.path().starts_with(std::env::temp_dir()),
            "scratch dir {} does not live under TMPDIR {}",
            scratch.path().display(),
            std::env::temp_dir().display()
        );
        let path = scratch.path().to_path_buf();
        // Force a panic while `scratch` is live to verify it is kept rather than deleted.
        panic!("intentional failure for {}", path.display());
    });
    let payload = res.unwrap_err();
    let msg = payload.downcast_ref::<String>().unwrap();
    let dir_path = Path::new(msg.trim_start_matches("intentional failure for ")).to_path_buf();
    assert!(
        dir_path.is_dir(),
        "failing scratch dir {} was unexpectedly deleted",
        dir_path.display()
    );
    fs::remove_dir_all(&dir_path).unwrap();
}
