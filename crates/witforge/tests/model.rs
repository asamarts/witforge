//! Model construction tests: shape, ordering, docs, stability, aliases, and refusal.

use std::fs;
use std::path::{Path, PathBuf};

use witforge::model::{
    self, MemberKind, Model, QualifiedName, ResultExpr, Stability, TypeDef, TypeDefKind, TypeExpr,
};
use witforge::{SourceFile, load, load_sources};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn temp_wit(name: &str, contract: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "witforge-model-limit-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("contract.wit"), contract).unwrap();
    dir
}

fn build(name: &str) -> Model {
    model::build(&load(&fixture(name)).unwrap()).unwrap()
}

fn model_fixture() -> Model {
    build("model")
}

#[test]
fn in_memory_loading_preserves_names_order_and_dependencies() {
    let sources = vec![
        SourceFile::new(
            "z_world.wit".to_string(),
            "package ex:root@0.1.0;\nworld plugin { export api; }\n".to_string(),
        ),
        SourceFile::new(
            "deps/shared/types.wit".to_string(),
            "package ex:shared@0.1.0;\ninterface types { record thing { id: u64 } }\n".to_string(),
        ),
        SourceFile::new(
            "a_contract.wit".to_string(),
            "package ex:root@0.1.0;\n\
             interface api { use ex:shared/types@0.1.0.{thing}; record wrapper { value: thing } }\n"
                .to_string(),
        ),
    ];
    let contract = load_sources(sources.clone()).unwrap();
    let first = model::build(&contract).unwrap();
    assert_eq!(first.package.name, "root");
    assert_eq!(first.dependencies.len(), 1);
    assert_eq!(first.dependencies[0].package.name, "shared");
    assert_eq!(first.worlds[0].exports[0].interface, "api");
    let locations = contract.member_locations();
    let wrapper_location = locations
        .iter()
        .find(|(key, _)| key.ends_with("api/wrapper.value"))
        .unwrap_or_else(|| panic!("wrapper field location missing from {locations:#?}"));
    assert_eq!(wrapper_location.1.0, "a_contract.wit");

    let mut reversed = sources;
    reversed.reverse();
    let second_contract = load_sources(reversed).unwrap();
    assert_eq!(model::build(&second_contract).unwrap(), first);
    assert_eq!(second_contract.member_locations(), locations);

    let direct_dependency = load_sources(vec![
        SourceFile::new(
            "contract.wit".into(),
            "package ex:root@0.1.0;\n\
             interface api { use ex:shared/types@0.1.0.{thing}; record wrapper { value: thing } }\n"
                .into(),
        ),
        SourceFile::new(
            "deps/shared.wit".into(),
            "package ex:shared@0.1.0;\ninterface types { record thing { id: u64 } }\n".into(),
        ),
    ])
    .unwrap();
    assert_eq!(
        model::build(&direct_dependency).unwrap().dependencies[0]
            .package
            .name,
        "shared"
    );

    for invalid in [
        SourceFile::new("../contract.wit".into(), String::new()),
        SourceFile::new("nested/contract.wit".into(), String::new()),
        SourceFile::new(r"deps\shared\contract.wit".into(), String::new()),
    ] {
        assert!(load_sources(vec![invalid]).is_err());
    }
    let duplicate = SourceFile::new("contract.wit".into(), "package ex:root@0.1.0;".into());
    assert!(load_sources(vec![duplicate.clone(), duplicate]).is_err());
}

#[test]
fn model_fixture_preserves_shape_docs_stability_and_escaped_names() {
    let model = model_fixture();
    assert_fixture_package_and_order(&model);
    assert_fixture_shapes(&model);
    assert_fixture_type_docs(&model);
    assert_fixture_function_and_world_docs(&model);
}

#[test]
fn stability_annotations_are_validated_and_preserved() {
    let contract = load_sources(vec![SourceFile::new(
        "contract.wit".into(),
        "package ex:annotations@0.2.0;\n\
         @since(version = 0.1.0)\n\
         @deprecated(version = 0.2.0)\n\
         interface api {\n\
             @since(version = 0.1.0)\n\
             @deprecated(version = 0.2.0)\n\
             record old-record { value: string }\n\
             @unstable(feature = preview)\n\
             @deprecated(version = 0.2.0)\n\
             old-fn: func();\n\
         }\n\
         @since(version = 0.1.0)\n\
         @deprecated(version = 0.2.0)\n\
         world plugin {\n\
             @since(version = 0.1.0)\n\
             @deprecated(version = 0.2.0)\n\
             import api;\n\
             @unstable(feature = preview)\n\
             export api;\n\
         }\n"
        .into(),
    )])
    .unwrap();
    assert_eq!(witforge::subset::check_diagnostics(&contract), []);
    let model = model::build(&contract).unwrap();
    let expected_stable = Stability::Deprecated {
        since: Some(model::Version::new(0, 1, 0)),
        feature: None,
        deprecated: model::Version::new(0, 2, 0),
        note: "Deprecated in WIT version 0.2.0.".into(),
    };
    assert_eq!(model.interfaces[0].stability, expected_stable);
    assert_eq!(model.interfaces[0].types[0].stability, expected_stable);
    assert_eq!(
        model.interfaces[0].functions[0].stability,
        Stability::Deprecated {
            since: None,
            feature: Some("preview".into()),
            deprecated: model::Version::new(0, 2, 0),
            note: "Deprecated in WIT version 0.2.0.".into(),
        }
    );
    assert_eq!(model.worlds[0].stability, expected_stable);
    assert_eq!(model.worlds[0].imports[0].stability, expected_stable);
    assert_eq!(
        model.worlds[0].exports[0].stability,
        Stability::Unstable {
            feature: "preview".into()
        }
    );
}

#[test]
fn invalid_stability_annotations_are_refused() {
    let invalid = load_sources(vec![SourceFile::new(
        "contract.wit".into(),
        "package ex:annotations@0.2.0;\n\
         @since(version = 0.1.1)\n\
         interface patch {}\n\
         @since(version = 0.2.0)\n\
         @deprecated(version = 0.1.0)\n\
         interface backwards {}\n\
         @since(version = 0.1.0)\n\
         @deprecated(version = 0.1.1)\n\
         interface deprecated-patch {}\n"
            .into(),
    )])
    .unwrap();
    let messages: Vec<String> = witforge::subset::check_diagnostics(&invalid)
        .into_iter()
        .map(|diagnostic| diagnostic.message)
        .collect();
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.contains("patch versions"))
            .count(),
        2
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("earlier than @since"))
    );
    let future_error = load_sources(vec![SourceFile::new(
        "contract.wit".into(),
        "package ex:annotations@0.2.0;\n\
         @since(version = 0.3.0)\n\
         interface future-item {}\n"
            .into(),
    )])
    .unwrap_err();
    assert!(format!("{future_error:#}").contains("unreleased version 0.3.0"));

    let parser_error = load_sources(vec![SourceFile::new(
        "contract.wit".into(),
        "package ex:annotations@0.2.0;\n\
         @deprecated(version = 0.2.0)\n\
         interface invalid {}\n"
            .into(),
    )])
    .unwrap_err();
    assert!(
        format!("{parser_error:#}")
            .contains("cannot specify both @deprecated without @since or @unstable")
    );
}

fn assert_fixture_package_and_order(model: &Model) {
    assert_eq!(model.package.namespace, "example");
    assert_eq!(model.package.name, "model");
    assert_eq!(model.package.version.to_string(), "1.2.0");
    assert_eq!(model.package.docs.trim(), "Model coverage package.");

    let interfaces: Vec<&str> = model
        .interfaces
        .iter()
        .map(|interface| interface.name.as_str())
        .collect();
    assert_eq!(interfaces, ["primitives", "aliases", "type"]);

    let primitives = model.interface("primitives").unwrap();
    let type_names: Vec<&str> = primitives
        .types
        .iter()
        .map(|def| def.name.as_str())
        .collect();
    assert_eq!(
        type_names,
        [
            "all",
            "choice",
            "status",
            "permissions",
            "strings",
            "maybe-all",
            "outcome",
            "bare",
            "only-ok",
            "only-err",
            "all-list",
            "maybe-outcome",
            "list-result",
            "all-alias",
            "strings-alias",
            "strings-alias-alias",
            "experimental",
        ]
    );
    let function_names: Vec<&str> = primitives
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(function_names, ["all-params", "no-result"]);

    let aliases = model.interface("aliases").unwrap();
    let alias_names: Vec<&str> = aliases.types.iter().map(|def| def.name.as_str()).collect();
    assert_eq!(
        alias_names,
        [
            "local-all",
            "local-strings",
            "local-outcome",
            "all-alias",
            "local-alias-alias",
        ]
    );
    assert_eq!(aliases.functions[0].name, "call");

    let escaped = model.interface("type").unwrap();
    assert_eq!(escaped.name, "type");
    assert_eq!(escaped.types[0].name, "where");
    assert_eq!(escaped.functions[0].name, "func");

    let worlds: Vec<&str> = model
        .worlds
        .iter()
        .map(|world| world.name.as_str())
        .collect();
    assert_eq!(worlds, ["world"]);
    let world = &model.worlds[0];
    assert_eq!(world.imports[0].interface, "primitives");
    assert_eq!(world.exports[0].interface, "aliases");
}

fn assert_fixture_shapes(model: &Model) {
    let all = model
        .type_def(&QualifiedName::new("primitives".into(), "all".into()))
        .unwrap();
    match &all.kind {
        TypeDefKind::Record(fields) => {
            let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
            assert_eq!(
                names,
                [
                    "a-bool", "a-u8", "a-u16", "a-u32", "a-u64", "a-s8", "a-s16", "a-s32", "a-s64",
                    "a-f32", "a-f64", "a-string",
                ]
            );
            assert_eq!(fields[0].ty, TypeExpr::Bool);
            assert_eq!(fields[11].ty, TypeExpr::String);
        }
        other => panic!("all should be a record, got {other:?}"),
    }

    let choice = model
        .type_def(&QualifiedName::new("primitives".into(), "choice".into()))
        .unwrap();
    match &choice.kind {
        TypeDefKind::Variant(cases) => {
            assert_eq!(cases[0].name, "with-payload");
            assert_eq!(cases[0].payload, Some(named("primitives", "all")));
            assert_eq!(cases[1].name, "without-payload");
            assert_eq!(cases[1].payload, None);
        }
        other => panic!("choice should be a variant, got {other:?}"),
    }

    for (name, matches_kind) in [
        ("status", "enum"),
        ("permissions", "flags"),
        ("all", "record"),
    ] {
        let def = model
            .type_def(&QualifiedName::new("primitives".into(), name.into()))
            .unwrap();
        match (matches_kind, &def.kind) {
            ("enum", TypeDefKind::Enum(_))
            | ("flags", TypeDefKind::Flags(_))
            | ("record", TypeDefKind::Record(_)) => {}
            (_, other) => panic!("{name} should be {matches_kind}, got {other:?}"),
        }
    }

    let ewhere = model
        .type_def(&QualifiedName::new("type".into(), "where".into()))
        .unwrap();
    match &ewhere.kind {
        TypeDefKind::Record(fields) => {
            assert_eq!(fields[0].name, "self");
            assert_eq!(fields[0].ty, TypeExpr::U32);
        }
        other => panic!("where should be a record, got {other:?}"),
    }
}

fn assert_fixture_type_docs(model: &Model) {
    let primitives = model.interface("primitives").unwrap();
    assert_eq!(
        primitives.docs.trim(),
        "Every primitive and the structural forms built from them."
    );
    assert_eq!(
        primitives.stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    let all = model
        .type_def(&QualifiedName::new("primitives".into(), "all".into()))
        .unwrap();
    assert_eq!(all.docs.trim(), "A record containing every primitive.");
    assert_eq!(
        all.stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    let fields = match &all.kind {
        TypeDefKind::Record(fields) => fields,
        other => panic!("all should be a record, got {other:?}"),
    };
    assert_eq!(fields[0].docs.trim(), "A boolean.");
    assert_eq!(fields[11].docs.trim(), "A string.");

    let choice = model
        .type_def(&QualifiedName::new("primitives".into(), "choice".into()))
        .unwrap();
    assert_eq!(
        choice.docs.trim(),
        "A variant with one payload case and one payload-less case."
    );
    let cases = match &choice.kind {
        TypeDefKind::Variant(cases) => cases,
        other => panic!("choice should be a variant, got {other:?}"),
    };
    assert_eq!(cases[0].docs.trim(), "A payload.");
    assert_eq!(cases[1].docs.trim(), "No payload.");

    let status = model
        .type_def(&QualifiedName::new("primitives".into(), "status".into()))
        .unwrap();
    assert_eq!(status.docs.trim(), "An enum.");
    let status_cases = match &status.kind {
        TypeDefKind::Enum(cases) => cases,
        other => panic!("status should be an enum, got {other:?}"),
    };
    assert_eq!(status_cases[0].docs.trim(), "Ready.");
    assert_eq!(status_cases[1].docs.trim(), "Done.");

    let permissions = model
        .type_def(&QualifiedName::new(
            "primitives".into(),
            "permissions".into(),
        ))
        .unwrap();
    assert_eq!(permissions.docs.trim(), "Flags.");
    let flags = match &permissions.kind {
        TypeDefKind::Flags(flags) => flags,
        other => panic!("permissions should be flags, got {other:?}"),
    };
    assert_eq!(flags[0].docs.trim(), "Read permission.");
    assert_eq!(flags[1].docs.trim(), "Write permission.");

    let experimental = model
        .type_def(&QualifiedName::new(
            "primitives".into(),
            "experimental".into(),
        ))
        .unwrap();
    assert_eq!(
        experimental.stability,
        Stability::Unstable {
            feature: "experimental".into()
        }
    );
    assert_eq!(
        experimental.docs.trim(),
        "An experimental type; retained even without the feature."
    );
}

fn assert_fixture_function_and_world_docs(model: &Model) {
    let primitives = model.interface("primitives").unwrap();

    let all_params = &primitives.functions[0];
    assert_eq!(all_params.name, "all-params");
    assert_eq!(
        all_params.docs.trim(),
        "A function with every parameter kind and a result."
    );
    assert_eq!(
        all_params.stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    assert_eq!(primitives.functions[1].name, "no-result");
    assert_eq!(
        primitives.functions[1].docs.trim(),
        "A function with no result."
    );

    let aliases = model.interface("aliases").unwrap();
    assert_eq!(
        aliases.docs.trim(),
        "An interface that uses aliases from `primitives`."
    );
    assert_eq!(
        aliases.stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    let escaped = model.interface("type").unwrap();
    assert_eq!(
        escaped.docs.trim(),
        "Escaped keyword identifiers remain raw names."
    );
    assert_eq!(escaped.types[0].docs.trim(), "An escaped record.");
    assert_eq!(escaped.functions[0].docs.trim(), "An escaped function.");

    let world = &model.worlds[0];
    assert_eq!(
        world.docs.trim(),
        "A world with documented and stable roles."
    );
    assert_eq!(
        world.stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    assert_eq!(
        world.imports[0].docs.trim(),
        "The primitive interface is imported."
    );
    assert_eq!(
        world.imports[0].stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    assert_eq!(
        world.exports[0].docs.trim(),
        "The alias interface is exported."
    );
    assert_eq!(
        world.exports[0].stability,
        Stability::Unstable {
            feature: "experimental".into()
        }
    );
}

#[test]
fn model_fixture_canonical_goldens() {
    let model = model_fixture();
    assert_fixture_primitive_canonical(&model);
    assert_fixture_named_canonical(&model);
    assert_fixture_function_canonical(&model);
}

fn assert_fixture_primitive_canonical(model: &Model) {
    for (ty, expected) in [
        (TypeExpr::Bool, "bool"),
        (TypeExpr::U8, "u8"),
        (TypeExpr::U16, "u16"),
        (TypeExpr::U32, "u32"),
        (TypeExpr::U64, "u64"),
        (TypeExpr::S8, "s8"),
        (TypeExpr::S16, "s16"),
        (TypeExpr::S32, "s32"),
        (TypeExpr::S64, "s64"),
        (TypeExpr::F32, "f32"),
        (TypeExpr::F64, "f64"),
        (TypeExpr::String, "string"),
    ] {
        assert_fixture_canonical(model, &ty, expected);
    }
}

fn assert_fixture_named_canonical(model: &Model) {
    for (interface, name, expected) in [
        ("primitives", "all", "primitives/all"),
        ("primitives", "choice", "primitives/choice"),
        ("primitives", "status", "primitives/status"),
        ("primitives", "permissions", "primitives/permissions"),
        ("primitives", "strings", "list<string>"),
        ("primitives", "maybe-all", "option<primitives/all>"),
        ("primitives", "outcome", "result<primitives/all,string>"),
        ("primitives", "bare", "result<_,_>"),
        ("primitives", "only-ok", "result<primitives/all,_>"),
        ("primitives", "only-err", "result<_,string>"),
        ("primitives", "all-list", "list<primitives/all>"),
        (
            "primitives",
            "maybe-outcome",
            "option<result<primitives/all,string>>",
        ),
        (
            "primitives",
            "list-result",
            "result<list<primitives/all>,string>",
        ),
        ("primitives", "all-alias", "primitives/all"),
        ("primitives", "strings-alias", "list<string>"),
        ("primitives", "strings-alias-alias", "list<string>"),
        (
            "primitives",
            "experimental",
            "result<primitives/all,string>",
        ),
        ("aliases", "local-all", "primitives/all"),
        ("aliases", "local-strings", "list<string>"),
        ("aliases", "local-outcome", "result<primitives/all,string>"),
        ("aliases", "all-alias", "primitives/all"),
        ("aliases", "local-alias-alias", "list<string>"),
        ("type", "where", "type/where"),
    ] {
        assert_fixture_canonical(model, &named(interface, name), expected);
    }
}

fn assert_fixture_function_canonical(model: &Model) {
    let primitives = model.interface("primitives").unwrap();
    let all_params = &primitives.functions[0];
    let param_names: Vec<&str> = all_params
        .params
        .iter()
        .map(|param| param.name.as_str())
        .collect();
    assert_eq!(
        param_names,
        [
            "a-bool", "a-u8", "a-u16", "a-u32", "a-u64", "a-s8", "a-s16", "a-s32", "a-s64",
            "a-f32", "a-f64", "a-string", "a-list", "a-option", "a-result", "a-nested",
        ]
    );
    for (index, expected) in [
        "bool",
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
        "string",
        "list<u8>",
        "option<string>",
        "result<u8,string>",
        "list<option<result<u8,string>>>",
    ]
    .into_iter()
    .enumerate()
    {
        assert_fixture_canonical(model, &all_params.params[index].ty, expected);
    }
    assert_eq!(
        all_params
            .result
            .as_ref()
            .map(|ty| model.canonical_type(ty).unwrap()),
        Some("result<list<primitives/all>,string>".to_string())
    );
    assert_eq!(primitives.functions[1].result, None);
    assert_eq!(
        named("primitives", "strings-alias-alias")
            .canonical(model)
            .unwrap(),
        "list<string>"
    );

    let call = &model.interface("aliases").unwrap().functions[0];
    assert_fixture_canonical(model, &call.params[0].ty, "primitives/all");
    assert_fixture_canonical(model, &call.params[1].ty, "list<string>");
    assert_eq!(
        call.result
            .as_ref()
            .map(|ty| model.canonical_type(ty).unwrap()),
        Some("result<primitives/all,string>".to_string())
    );

    let escaped = &model.interface("type").unwrap().functions[0];
    assert_fixture_canonical(model, &escaped.params[0].ty, "u32");
    assert_eq!(
        escaped
            .result
            .as_ref()
            .map(|ty| model.canonical_type(ty).unwrap()),
        Some("type/where".to_string())
    );
}

fn assert_fixture_canonical(model: &Model, ty: &TypeExpr, expected: &str) {
    let actual = model.canonical_type(ty).unwrap();
    assert_eq!(actual, expected);
    assert!(
        !actual.chars().any(char::is_whitespace),
        "canonical `{actual}` contains whitespace"
    );
}

#[test]
fn model_fixture_member_goldens() {
    let model = model_fixture();
    let members = model.members().unwrap();
    let golden = fs::read_to_string(fixture("model").join("members.golden")).unwrap();
    let expected = parse_member_golden(&golden);
    assert_eq!(members.len(), expected.len());
    for (index, (member, (key, kind, ty))) in members.iter().zip(expected).enumerate() {
        assert_eq!(member.key(), key, "member {index} key");
        assert_eq!(member.kind, kind, "member {index} kind");
        assert_eq!(member.ty.as_deref(), ty.as_deref(), "member {index} type");
    }

    let contract = load(&fixture("model")).unwrap();
    assert_eq!(model::members(&contract).unwrap(), members);
}

fn parse_member_golden(golden: &str) -> Vec<(String, MemberKind, Option<String>)> {
    golden
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            let line_no = index + 1;
            let fields: Vec<&str> = line.split('\t').collect();
            assert!(
                (2..=3).contains(&fields.len()),
                "golden line {line_no}: expected key, kind, and optional type"
            );
            let kind = fields[1]
                .parse::<MemberKind>()
                .unwrap_or_else(|error| panic!("golden line {line_no}: {error}"));
            let ty = fields.get(2).and_then(|value| {
                if value.is_empty() {
                    None
                } else {
                    Some((*value).to_string())
                }
            });
            (fields[0].to_string(), kind, ty)
        })
        .collect()
}

#[test]
fn build_preserves_package_shape_order_and_docs() {
    let model = build("ok");
    assert_package_and_types(&model);
    assert_analyzer(&model);
    assert_world_and_lookup(&model);
}

fn assert_package_and_types(model: &Model) {
    assert_eq!(model.package.namespace, "example");
    assert_eq!(model.package.name, "plugin");
    assert_eq!(model.package.version.to_string(), "0.1.0");

    let interfaces: Vec<&str> = model
        .interfaces
        .iter()
        .map(|interface| interface.name.as_str())
        .collect();
    assert_eq!(interfaces, ["types", "analyzer"]);
    assert_eq!(
        model.interface("types").unwrap().docs.trim(),
        "Shared contract types."
    );

    let types: Vec<&str> = model.interfaces[0]
        .types
        .iter()
        .map(|def| def.name.as_str())
        .collect();
    assert_eq!(types, ["span", "applicability", "caps", "edit", "finding"]);

    let span = &model.interfaces[0].types[0];
    assert_eq!(span.docs.trim(), "A byte range in a resource.");
    match &span.kind {
        TypeDefKind::Record(fields) => {
            let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
            assert_eq!(names, ["start", "end"]);
            assert_eq!(fields[0].ty, TypeExpr::U64);
            assert_eq!(fields[1].ty, TypeExpr::U64);
        }
        other => panic!("span should be a record, got {other:?}"),
    }

    let edit = &model.interfaces[0].types[3];
    match &edit.kind {
        TypeDefKind::Variant(cases) => {
            assert_eq!(cases[0].name, "replace");
            assert_eq!(
                cases[0].payload,
                Some(TypeExpr::Named(QualifiedName::new(
                    "types".into(),
                    "span".into(),
                )))
            );
            assert_eq!(cases[1].name, "delete-file");
            assert_eq!(cases[1].payload, None);
        }
        other => panic!("edit should be a variant, got {other:?}"),
    }
}

fn assert_analyzer(model: &Model) {
    let analyzer = &model.interfaces[1];
    let alias = &analyzer.types[0];
    assert_eq!(alias.name, "finding");
    match &alias.kind {
        TypeDefKind::Alias(TypeExpr::Named(target)) => {
            assert_eq!(target.interface, "types");
            assert_eq!(target.name, "finding");
        }
        other => panic!("finding alias should be an alias, got {other:?}"),
    }

    let analyze = &analyzer.functions[0];
    assert_eq!(analyze.name, "analyze");
    let params: Vec<&str> = analyze
        .params
        .iter()
        .map(|param| param.name.as_str())
        .collect();
    assert_eq!(params, ["path", "bytes"]);
    assert_eq!(analyze.params[0].ty, TypeExpr::String);
    assert_eq!(analyze.params[1].ty, TypeExpr::List(Box::new(TypeExpr::U8)));
    match &analyze.result {
        Some(TypeExpr::Result(ResultExpr {
            ok: Some(ok),
            err: Some(err),
            ..
        })) => {
            assert_eq!(**err, TypeExpr::String);
            match &**ok {
                TypeExpr::List(element) => match &**element {
                    TypeExpr::Named(target) => {
                        assert_eq!(target.interface, "analyzer");
                        assert_eq!(target.name, "finding");
                    }
                    other => panic!("result list element should be a named alias, got {other:?}"),
                },
                other => panic!("result ok should be a list, got {other:?}"),
            }
        }
        other => panic!("analyze should return result<list<finding>, string>, got {other:?}"),
    }
}

fn assert_world_and_lookup(model: &Model) {
    assert_eq!(model.worlds.len(), 1);
    let world = &model.worlds[0];
    assert_eq!(world.name, "plugin");
    // `use types.{finding}` makes the interface available; the type alias itself is not a
    // world item, but the interface dependency is.
    let imports: Vec<&str> = world
        .imports
        .iter()
        .map(|interface| interface.interface.as_str())
        .collect();
    assert_eq!(imports, ["types"]);
    let exports: Vec<&str> = world
        .exports
        .iter()
        .map(|interface| interface.interface.as_str())
        .collect();
    assert_eq!(exports, ["analyzer"]);

    assert!(model.interface("missing").is_none());
    assert_eq!(
        model
            .type_def(&QualifiedName::new("types".into(), "span".into()))
            .unwrap()
            .name,
        "span"
    );
}

#[test]
fn build_carries_docs_and_stability_and_keeps_unstable_items() {
    let (dir, model) = stability_model("docs");
    assert_eq!(model.package.docs.trim(), "Package docs.");
    let types = model.interface("types").unwrap();
    assert_eq!(types.docs.trim(), "A stable interface.");
    assert_eq!(
        types.stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    let span = model
        .type_def(&QualifiedName::new("types".into(), "span".into()))
        .unwrap();
    assert_eq!(span.docs.trim(), "A stable span.");
    assert_eq!(
        span.stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    let mode = model
        .type_def(&QualifiedName::new("types".into(), "mode".into()))
        .unwrap();
    assert_eq!(
        mode.stability,
        Stability::Unstable {
            feature: "experimental".into()
        }
    );
    assert!(matches!(&mode.kind, TypeDefKind::Enum(cases) if cases.len() == 2));

    let function_names: Vec<&str> = types
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(function_names, ["read", "write"]);

    let read = &types.functions[0];
    assert_eq!(read.name, "read");
    assert_eq!(read.docs.trim(), "A stable function.");
    assert_eq!(
        read.stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    assert_eq!(
        read.params[0].ty,
        TypeExpr::Named(QualifiedName::new("types".into(), "span".into()))
    );
    assert_eq!(
        read.result,
        Some(TypeExpr::Named(QualifiedName::new(
            "types".into(),
            "span".into(),
        )))
    );
    assert_eq!(types.functions[1].name, "write");
    assert_eq!(types.functions[1].result, None);

    let world = &model.worlds[0];
    assert_eq!(world.docs.trim(), "A stable world.");
    assert_eq!(
        world.stability,
        Stability::Stable {
            since: model.package.version
        }
    );
    assert_eq!(
        world.exports[0].docs.trim(),
        "The exported types interface."
    );
    assert_eq!(
        world.exports[0].stability,
        Stability::Stable {
            since: model.package.version
        }
    );

    fs::remove_dir_all(&dir).unwrap();
}

fn stability_model(name: &str) -> (PathBuf, Model) {
    let dir = std::env::temp_dir().join(format!(
        "witforge-model-stability-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("contract.wit"),
        r"/// Package docs.
package example:stability@0.1.0;

/// A stable interface.
@since(version = 0.1.0)
interface types {
    /// A stable span.
    @since(version = 0.1.0)
    record span {
        start: u64,
    }

    /// An experimental mode.
    @unstable(feature = experimental)
    enum mode {
        on,
        off,
    }

    /// A stable function.
    @since(version = 0.1.0)
    read: func(value: span) -> span;

    write: func(value: span);
}

/// A stable world.
@since(version = 0.1.0)
world plugin {
    /// The exported types interface.
    @since(version = 0.1.0)
    export types;
}
",
    )
    .unwrap();

    let model = model::build(&load(&dir).unwrap()).unwrap();
    (dir, model)
}

fn named(interface: &str, name: &str) -> TypeExpr {
    TypeExpr::Named(QualifiedName::new(interface.into(), name.into()))
}

#[test]
fn canonical_types_cover_primitives_and_nesting() {
    let model = build("ok");
    let primitives = [
        (TypeExpr::Bool, "bool"),
        (TypeExpr::U8, "u8"),
        (TypeExpr::U16, "u16"),
        (TypeExpr::U32, "u32"),
        (TypeExpr::U64, "u64"),
        (TypeExpr::S8, "s8"),
        (TypeExpr::S16, "s16"),
        (TypeExpr::S32, "s32"),
        (TypeExpr::S64, "s64"),
        (TypeExpr::F32, "f32"),
        (TypeExpr::F64, "f64"),
        (TypeExpr::String, "string"),
    ];
    for (ty, expected) in primitives {
        assert_eq!(model.canonical_type(&ty).unwrap(), expected);
    }

    for name in ["span", "applicability", "caps", "edit"] {
        assert_eq!(
            model.canonical_type(&named("types", name)).unwrap(),
            format!("types/{name}")
        );
    }
    assert_eq!(
        model.canonical_type(&named("analyzer", "finding")).unwrap(),
        "types/finding"
    );
    assert_eq!(
        named("analyzer", "finding").canonical(&model).unwrap(),
        "types/finding"
    );
    assert_eq!(
        model
            .canonical_type(&model.interfaces[1].functions[0].result.clone().unwrap())
            .unwrap(),
        "result<list<types/finding>,string>"
    );

    let nested = TypeExpr::List(Box::new(TypeExpr::Option(Box::new(TypeExpr::Result(
        ResultExpr::new(
            Some(Box::new(TypeExpr::U8)),
            Some(Box::new(TypeExpr::String)),
        ),
    )))));
    assert_eq!(
        model.canonical_type(&nested).unwrap(),
        "list<option<result<u8,string>>>"
    );
    assert_eq!(
        model
            .canonical_type(&TypeExpr::Result(ResultExpr::default()))
            .unwrap(),
        "result<_,_>"
    );
    assert_eq!(
        model
            .canonical_type(&TypeExpr::Result(ResultExpr::new(
                Some(Box::new(TypeExpr::U32)),
                None,
            )))
            .unwrap(),
        "result<u32,_>"
    );
    assert_eq!(
        model
            .canonical_type(&TypeExpr::Result(ResultExpr::new(
                None,
                Some(Box::new(TypeExpr::String)),
            )))
            .unwrap(),
        "result<_,string>"
    );
    assert_eq!(
        model
            .canonical_type(&TypeExpr::Result(ResultExpr::new(
                Some(Box::new(named("analyzer", "finding"))),
                Some(Box::new(named("analyzer", "finding"))),
            )))
            .unwrap(),
        "result<types/finding,types/finding>"
    );
}

#[test]
fn canonical_types_expand_named_structural_types_and_use_aliases() {
    let (dir, model) = canonical_model("canonical");
    let cases = [
        ("span", "base/span"),
        ("tags", "list<string>"),
        ("maybe-span", "option<base/span>"),
        ("outcome", "result<base/span,string>"),
        ("bare", "result<_,_>"),
        ("only-ok", "result<base/span,_>"),
        ("only-err", "result<_,string>"),
        ("number", "u64"),
        ("alias-number", "u64"),
        ("bytes", "list<u8>"),
        ("result-tags", "result<list<string>,string>"),
        ("span-alias", "base/span"),
        ("outcome-alias", "result<base/span,string>"),
        ("alias-tags", "list<string>"),
        ("alias-alias", "list<string>"),
        ("list-maybe", "list<option<base/span>>"),
        ("maybe-tags", "option<list<string>>"),
        ("list-tags", "list<list<string>>"),
        ("maybe-outcome", "option<result<base/span,string>>"),
    ];
    for (name, expected) in cases {
        let canonical = model.canonical_type(&named("base", name)).unwrap();
        assert_eq!(canonical, expected, "base/{name}");
        assert!(
            !canonical.chars().any(char::is_whitespace),
            "canonical `{canonical}` contains whitespace"
        );
    }
    assert_eq!(
        model.canonical_type(&named("user", "local-span")).unwrap(),
        "base/span"
    );
    assert_eq!(
        model.canonical_type(&named("user", "tags")).unwrap(),
        "list<string>"
    );
    assert_eq!(
        model.canonical_type(&named("user", "maybe-span")).unwrap(),
        "option<base/span>"
    );
    assert_eq!(
        model.canonical_type(&named("user", "outcome")).unwrap(),
        "result<base/span,string>"
    );
    assert_eq!(
        model.canonical_type(&named("user", "alias-alias")).unwrap(),
        "list<string>"
    );
    assert_eq!(
        model.canonical_type(&named("user", "local-tags")).unwrap(),
        "list<string>"
    );
    assert_eq!(
        model.canonical_type(&named("user", "local-maybe")).unwrap(),
        "option<base/span>"
    );
    assert_eq!(
        model
            .canonical_type(&named("user", "local-outcome"))
            .unwrap(),
        "result<base/span,string>"
    );
    let read = &model.interface("user").unwrap().functions[0];
    assert_eq!(
        model.canonical_type(&read.params[0].ty).unwrap(),
        "base/span"
    );
    assert_eq!(
        model.canonical_type(read.result.as_ref().unwrap()).unwrap(),
        "list<string>"
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn canonical_types_reject_missing_types_and_cycles() {
    let mut model = build("ok");
    assert!(model.canonical_type(&named("types", "missing")).is_err());

    let cycle = QualifiedName::new("analyzer".into(), "cycle".into());
    model.interfaces[1].types.push(TypeDef::new(
        "cycle".into(),
        String::new(),
        Stability::Unknown,
        TypeDefKind::Alias(TypeExpr::Named(cycle.clone())),
    ));
    assert!(model.canonical_type(&TypeExpr::Named(cycle)).is_err());

    for (name, kind) in [
        (
            "cycle-list",
            TypeDefKind::List(TypeExpr::Named(QualifiedName::new(
                "analyzer".into(),
                "cycle-list".into(),
            ))),
        ),
        (
            "cycle-option",
            TypeDefKind::Option(TypeExpr::Named(QualifiedName::new(
                "analyzer".into(),
                "cycle-option".into(),
            ))),
        ),
        (
            "cycle-result",
            TypeDefKind::Result(ResultExpr::new(
                Some(Box::new(TypeExpr::Named(QualifiedName::new(
                    "analyzer".into(),
                    "cycle-result".into(),
                )))),
                None,
            )),
        ),
    ] {
        let name = QualifiedName::new("analyzer".into(), name.into());
        model.interfaces[1].types.push(TypeDef::new(
            name.name.clone(),
            String::new(),
            Stability::Unknown,
            kind,
        ));
        assert!(model.canonical_type(&TypeExpr::Named(name)).is_err());
    }
}

#[test]
fn members_extract_types_functions_and_parameters_in_source_order() {
    let contract = load(&fixture("ok")).unwrap();
    let model = model::build(&contract).unwrap();
    let members = model.members().unwrap();
    let actual: Vec<(String, MemberKind, Option<String>)> = members
        .iter()
        .map(|member| (member.key(), member.kind, member.ty.clone()))
        .collect();
    let expected: Vec<(String, MemberKind, Option<String>)> = expected_ok_members()
        .into_iter()
        .map(|(key, kind, ty)| (key.to_string(), kind, ty.map(str::to_string)))
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(model::members(&contract).unwrap(), members);
}

fn expected_ok_members() -> Vec<(&'static str, MemberKind, Option<&'static str>)> {
    vec![
        (
            "example:plugin/types/span.start",
            MemberKind::Field,
            Some("u64"),
        ),
        (
            "example:plugin/types/span.end",
            MemberKind::Field,
            Some("u64"),
        ),
        (
            "example:plugin/types/applicability.safe",
            MemberKind::EnumCase,
            None,
        ),
        (
            "example:plugin/types/applicability.unsafe",
            MemberKind::EnumCase,
            None,
        ),
        (
            "example:plugin/types/applicability.suggestion",
            MemberKind::EnumCase,
            None,
        ),
        (
            "example:plugin/types/applicability.never",
            MemberKind::EnumCase,
            None,
        ),
        ("example:plugin/types/caps.git", MemberKind::Flag, None),
        (
            "example:plugin/types/caps.environment",
            MemberKind::Flag,
            None,
        ),
        (
            "example:plugin/types/edit.replace",
            MemberKind::VariantCase,
            Some("types/span"),
        ),
        (
            "example:plugin/types/edit.delete-file",
            MemberKind::VariantCase,
            None,
        ),
        (
            "example:plugin/types/finding.message",
            MemberKind::Field,
            Some("string"),
        ),
        (
            "example:plugin/types/finding.primary",
            MemberKind::Field,
            Some("option<types/span>"),
        ),
        (
            "example:plugin/types/finding.related",
            MemberKind::Field,
            Some("list<types/span>"),
        ),
        (
            "example:plugin/types/finding.payload",
            MemberKind::Field,
            Some("list<u8>"),
        ),
        (
            "example:plugin/analyzer.analyze",
            MemberKind::Method,
            Some("result<list<types/finding>,string>"),
        ),
        (
            "example:plugin/analyzer#analyze.path",
            MemberKind::Param,
            Some("string"),
        ),
        (
            "example:plugin/analyzer#analyze.bytes",
            MemberKind::Param,
            Some("list<u8>"),
        ),
    ]
}

#[test]
fn members_omit_aliases_and_named_structural_types() {
    let (dir, model) = canonical_model("members");
    let members = model.members().unwrap();
    let keys: Vec<String> = members.iter().map(model::Member::key).collect();
    assert!(keys.contains(&"example:canonical/base/span.start".to_string()));
    assert!(keys.contains(&"example:canonical/user.read".to_string()));
    assert!(keys.contains(&"example:canonical/user#read.value".to_string()));
    for skipped in [
        "example:canonical/base/tags",
        "example:canonical/base/maybe-span",
        "example:canonical/base/outcome",
        "example:canonical/base/bare",
        "example:canonical/base/alias-tags",
        "example:canonical/user/local-span",
        "example:canonical/user/local-tags",
    ] {
        assert!(
            !keys.iter().any(|key| key.starts_with(skipped)),
            "`{skipped}` must not produce members: {keys:#?}"
        );
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn member_methods_without_results_have_no_type() {
    let (dir, model) = stability_model("members");
    let members = model.members().unwrap();
    let read = members
        .iter()
        .find(|member| member.key() == "example:stability/types.read")
        .unwrap();
    assert_eq!(read.kind, MemberKind::Method);
    assert_eq!(read.ty.as_deref(), Some("types/span"));
    let write = members
        .iter()
        .find(|member| member.key() == "example:stability/types.write")
        .unwrap();
    assert_eq!(write.kind, MemberKind::Method);
    assert_eq!(write.ty, None);
    let methods: Vec<&str> = members
        .iter()
        .filter(|member| member.kind == MemberKind::Method)
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(methods, ["read", "write"]);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn member_kinds_round_trip_lockfile_tokens() {
    for kind in [
        MemberKind::Field,
        MemberKind::VariantCase,
        MemberKind::EnumCase,
        MemberKind::Flag,
        MemberKind::Method,
        MemberKind::Param,
    ] {
        assert_eq!(kind.as_str().parse::<MemberKind>().unwrap(), kind);
        assert_eq!(kind.to_string(), kind.as_str());
    }
}

#[test]
fn build_preserves_escaped_names_and_use_as_aliases() {
    let dir = std::env::temp_dir().join(format!("witforge-model-escaped-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("contract.wit"),
        r"package example:escaped@0.1.0;

interface %type {
    record %where {
        %self: u32,
    }

    %func: func(%self: u32) -> %where;
}

interface user {
    use %type.{%where as local-where};

    call: func(value: local-where) -> local-where;
}
",
    )
    .unwrap();

    let model = model::build(&load(&dir).unwrap()).unwrap();
    let types = model.interface("type").unwrap();
    let record = &types.types[0];
    assert_eq!(record.name, "where");
    match &record.kind {
        TypeDefKind::Record(fields) => assert_eq!(fields[0].name, "self"),
        other => panic!("where should be a record, got {other:?}"),
    }
    assert_eq!(types.functions[0].name, "func");
    assert_eq!(types.functions[0].params[0].name, "self");

    let user = model.interface("user").unwrap();
    let alias = &user.types[0];
    assert_eq!(alias.name, "local-where");
    match &alias.kind {
        TypeDefKind::Alias(TypeExpr::Named(target)) => {
            assert_eq!(target.interface, "type");
            assert_eq!(target.name, "where");
        }
        other => panic!("local-where should be an alias, got {other:?}"),
    }
    let call = &user.functions[0];
    assert_eq!(call.params[0].name, "value");
    assert_eq!(
        call.params[0].ty,
        TypeExpr::Named(QualifiedName::new("user".into(), "local-where".into(),))
    );
    assert_eq!(call.result, Some(call.params[0].ty.clone()));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn build_accepts_included_world_use_aliases_and_interfaces() {
    let dir = std::env::temp_dir().join(format!("witforge-model-include-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("contract.wit"),
        r"package example:included@0.1.0;

interface types {
    record span {
        start: u64,
    }
}

world base {
    export types;
    use types.{span};
}

world plugin {
    include base;
}
",
    )
    .unwrap();

    let model = model::build(&load(&dir).unwrap()).unwrap();
    let world_names: Vec<&str> = model
        .worlds
        .iter()
        .map(|world| world.name.as_str())
        .collect();
    assert_eq!(world_names, ["base", "plugin"]);
    let plugin = model
        .worlds
        .iter()
        .find(|world| world.name == "plugin")
        .unwrap();
    let exports: Vec<&str> = plugin
        .exports
        .iter()
        .map(|interface| interface.interface.as_str())
        .collect();
    assert_eq!(exports, ["types"]);

    fs::remove_dir_all(&dir).unwrap();
}

fn canonical_model(name: &str) -> (PathBuf, Model) {
    let dir = std::env::temp_dir().join(format!(
        "witforge-model-canonical-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("contract.wit"),
        r"package example:canonical@0.1.0;

interface base {
    record span {
        start: u64,
    }

    type tags = list<string>;
    type maybe-span = option<span>;
    type outcome = result<span, string>;
    type bare = result;
    type only-ok = result<span>;
    type only-err = result<_, string>;
    type number = u64;
    type alias-number = number;
    type bytes = list<u8>;
    type result-tags = result<tags, string>;
    type span-alias = span;
    type outcome-alias = outcome;
    type alias-tags = tags;
    type alias-alias = alias-tags;
    type list-maybe = list<maybe-span>;
    type maybe-tags = option<tags>;
    type list-tags = list<tags>;
    type maybe-outcome = option<outcome>;
}

interface user {
    use base.{span as local-span, tags, tags as local-tags, maybe-span, maybe-span as local-maybe, outcome, outcome as local-outcome, bare, only-ok, only-err, number, alias-number, bytes, result-tags, span-alias, outcome-alias, alias-tags, alias-alias, list-maybe, maybe-tags, list-tags, maybe-outcome};

    read: func(value: local-span) -> tags;
}
",
    )
    .unwrap();

    let model = model::build(&load(&dir).unwrap()).unwrap();
    (dir, model)
}

#[test]
fn build_refuses_subsets_the_converter_cannot_represent() {
    for name in [
        "resource",
        "handle",
        "tuple",
        "map",
        "fixed-length-list",
        "future",
        "stream",
        "char",
        "error-context",
        "world-type",
        "world-function",
        "inline-interface",
        "unversioned",
        "version-patch",
    ] {
        let contract = load(&fixture(&format!("refused/{name}"))).unwrap();
        assert!(
            model::build(&contract).is_err(),
            "`{name}` should be refused by the model"
        );
    }
}

#[test]
fn public_model_api_is_usable_without_wit_parser() {
    // This integration test can name every public model type and needs no `wit-parser` import
    // or `Contract` field access: the model's construction boundary is opaque.
    let contract = load(&fixture("ok")).unwrap();
    let model: Model = model::build(&contract).unwrap();
    let _: &model::Package = &model.package;
    let _: &model::Interface = &model.interfaces[0];
    let _: &model::TypeDef = &model.interfaces[0].types[0];
    let _: &model::TypeExpr = &model.interfaces[1].functions[0].params[0].ty;
    let _: &model::World = &model.worlds[0];
    assert_eq!(model.package.version.to_string(), "0.1.0");
}

#[test]
fn qualified_names_render_as_interface_slash_type() {
    assert_eq!(
        QualifiedName::new("types".into(), "span".into()).to_string(),
        "types/span"
    );
}

#[test]
#[allow(clippy::format_push_string)]
fn alias_chains_are_bounded() {
    let mut contract = String::from("package ex:t@0.1.0;\ninterface types {\n  type a0 = u8;\n");
    for index in 1..=40 {
        contract.push_str(&format!("  type a{index} = a{};\n", index - 1));
    }
    contract.push_str("}\n");
    let dir = temp_wit("alias-chain", &contract);
    let error = model::build(&load(&dir).unwrap()).unwrap_err();
    assert!(witforge::is_contract_error(&error), "{error:#}");
    assert!(format!("{error:#}").contains("alias chain"), "{error:#}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn nested_types_are_bounded() {
    let nested = format!("{}u8{}", "list<".repeat(40), ">".repeat(40));
    let contract =
        format!("package ex:t@0.1.0;\ninterface types {{ record r {{ value: {nested} }} }}\n");
    let dir = temp_wit("nesting", &contract);
    let error = model::build(&load(&dir).unwrap()).unwrap_err();
    assert!(witforge::is_contract_error(&error), "{error:#}");
    assert!(format!("{error:#}").contains("nesting"), "{error:#}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn canonical_type_strings_are_bounded() {
    let long = "a".repeat(1100);
    let contract = format!(
        "package ex:t@0.1.0;\ninterface types {{ record {long} {{ value: u32 }} \
         record r {{ value: {long} }} }}\n"
    );
    let dir = temp_wit("canonical-length", &contract);
    let error = model::build(&load(&dir).unwrap()).unwrap_err();
    assert!(witforge::is_contract_error(&error), "{error:#}");
    assert!(format!("{error:#}").contains("byte limit"), "{error:#}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
#[allow(clippy::format_push_string)]
fn a_doubling_contract_is_refused_before_it_grows_large() {
    let mut contract = String::from("package ex:t@0.1.0;\ninterface types {\n  type t0 = u8;\n");
    for index in 1..=12 {
        contract.push_str(&format!(
            "  type t{index} = list<result<t{},t{}>>;\n",
            index - 1,
            index - 1
        ));
    }
    contract.push_str("  record r { value: t12 }\n}\n");
    let dir = temp_wit("doubling", &contract);
    let error = model::build(&load(&dir).unwrap()).unwrap_err();
    let message = format!("{error:#}");
    assert!(witforge::is_contract_error(&error), "{message}");
    assert!(message.contains("byte limit"), "{message}");
    assert!(
        message.len() < 4096,
        "diagnostic should stay small: {message}"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn multi_file_version_directories_order_deterministically_across_all_emitters() {
    use witforge::emit::{glue, json_schema, proto, rust};
    use witforge::lock::{LockMode, WireLock};

    let build_root = |label: &str, types_filename: &str, reverse_creation: bool| {
        let root = std::env::temp_dir().join(format!(
            "witforge-multi-file-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let v01 = root.join("v0.1");
        let dep_dir = v01.join("deps/shared");
        fs::create_dir_all(&dep_dir).unwrap();

        let mut dep_files = vec![
            (
                "01-base.wit",
                "interface base {\n    record shared-id { raw: u64 }\n}\n",
            ),
            (
                "02-extra.wit",
                "package ex:shared@0.1.0;\n\ninterface extra {\n    use base.{shared-id};\n    record tag { id: shared-id }\n}\n",
            ),
        ];
        let mut root_files = vec![
            (
                types_filename,
                "interface alpha {\n    use ex:shared/extra@0.1.0.{tag};\n    record item { tag: tag, count: u32 }\n}\n",
            ),
            (
                "02-service.wit",
                "interface beta {\n    use alpha.{item};\n    fetch: func(id: u32) -> item;\n}\n",
            ),
            (
                "99-package.wit",
                "package ex:multi@0.1.0;\n\ninterface omega {\n    enum mode { fast, safe }\n}\n\nworld plugin {\n    export beta;\n}\n",
            ),
        ];
        if reverse_creation {
            dep_files.reverse();
            root_files.reverse();
        }
        for (name, contents) in dep_files {
            fs::write(dep_dir.join(name), contents).unwrap();
        }
        for (name, contents) in root_files {
            fs::write(v01.join(name), contents).unwrap();
        }

        let model = model::build(&load(&v01).unwrap()).unwrap();
        let mut lock = WireLock::default();
        lock.reconcile_with(&model.members().unwrap(), LockMode::First)
            .unwrap();
        let lock_text = lock.render();
        fs::write(v01.join("wire.lock"), &lock_text).unwrap();

        let rust_files = rust::plan(&root).unwrap();
        let proto_files = proto::plan(&root).unwrap();
        let schema_files = json_schema::plan(&root).unwrap();
        let glue_files = glue::plan(&root, "crate::plain", "crate::proto").unwrap();
        fs::remove_dir_all(&root).unwrap();
        (
            model,
            lock_text,
            rust_files,
            proto_files,
            schema_files,
            glue_files,
        )
    };

    let (m_fwd, lock_fwd, rust_fwd, proto_fwd, schema_fwd, glue_fwd) =
        build_root("fwd", "01-types.wit", false);
    let (m_rev, lock_rev, rust_rev, proto_rev, schema_rev, glue_rev) =
        build_root("rev", "01-types.wit", true);

    let iface_names: Vec<&str> = m_fwd.interfaces.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(iface_names, vec!["alpha", "beta", "omega"]);
    let dep_iface_names: Vec<&str> = m_fwd.dependencies[0]
        .interfaces
        .iter()
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(dep_iface_names, vec!["base", "extra"]);

    assert_eq!(m_fwd, m_rev);
    assert_eq!(lock_fwd, lock_rev);
    assert_eq!(rust_fwd, rust_rev);
    assert_eq!(proto_fwd, proto_rev);
    assert_eq!(schema_fwd, schema_rev);
    assert_eq!(glue_fwd, glue_rev);

    // Renaming a file moves its interface deterministically without changing wire.lock.
    let (m_renamed, lock_renamed, _, _, _, _) = build_root("renamed", "50-types.wit", false);
    let renamed_ifaces: Vec<&str> = m_renamed
        .interfaces
        .iter()
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(renamed_ifaces, vec!["beta", "alpha", "omega"]);
    assert_eq!(lock_renamed, lock_fwd);
}
