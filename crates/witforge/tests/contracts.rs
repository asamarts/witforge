//! Acceptance tests for the representative alint plugin contract fixture.

use std::fs;
use std::path::{Path, PathBuf};

use witforge::model::{self, MemberKind, Model, QualifiedName, TypeDefKind};
use witforge::{load, subset};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/contracts/alint-plugin")
}

fn model() -> Model {
    model::build(&load(&fixture_dir()).unwrap()).unwrap()
}

#[test]
fn alint_contract_passes_subset_and_model() {
    let contract = load(&fixture_dir()).unwrap();
    assert!(subset::check(&contract).is_empty());
    let model = model::build(&contract).unwrap();
    assert_eq!(model.package.namespace, "alint");
    assert_eq!(model.package.name, "plugin");
    assert_eq!(model.package.version.to_string(), "0.1.0");

    let interfaces: Vec<&str> = model
        .interfaces
        .iter()
        .map(|interface| interface.name.as_str())
        .collect();
    assert_eq!(interfaces, ["types", "adapter-api", "analyzer-api"]);

    let types = model.interface("types").unwrap();
    let type_names: Vec<&str> = types.types.iter().map(|def| def.name.as_str()).collect();
    assert_eq!(
        type_names,
        [
            "resource-info",
            "node",
            "record-entry",
            "flat-value",
            "row-state",
            "pointer-segment",
            "row",
            "related-location",
            "finding",
            "limits",
            "adapter-input",
            "adapter-output",
            "limit-error",
            "adapter-error",
            "analyzer-input",
            "analyzer-error",
            "capabilities",
        ]
    );

    let world_names: Vec<&str> = model
        .worlds
        .iter()
        .map(|world| world.name.as_str())
        .collect();
    assert_eq!(world_names, ["adapter", "analyzer"]);
    assert_eq!(model.worlds[0].exports[0].interface, "adapter-api");
    assert_eq!(model.worlds[1].exports[0].interface, "analyzer-api");

    let adapter = model.interface("adapter-api").unwrap();
    let adapter_functions: Vec<&str> = adapter
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(
        adapter_functions,
        ["id", "redacts-by-default", "get-capabilities", "extract"]
    );
    let analyzer = model.interface("analyzer-api").unwrap();
    let analyzer_functions: Vec<&str> = analyzer
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(analyzer_functions, ["id", "get-capabilities", "analyze"]);
}

#[test]
fn alint_contract_shapes_and_members_are_stable() {
    let model = model();
    let node = model
        .type_def(&QualifiedName {
            interface: "types".into(),
            name: "node".into(),
        })
        .unwrap();
    let node_cases = match &node.kind {
        TypeDefKind::Variant(cases) => cases
            .iter()
            .map(|case| case.name.as_str())
            .collect::<Vec<_>>(),
        other => panic!("node should be a variant, got {other:?}"),
    };
    assert_eq!(
        node_cases,
        [
            "null", "boolean", "number", "text", "bytes", "path", "list", "record"
        ]
    );

    let row = model
        .type_def(&QualifiedName {
            interface: "types".into(),
            name: "row".into(),
        })
        .unwrap();
    let row_fields = match &row.kind {
        TypeDefKind::Record(fields) => fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        other => panic!("row should be a record, got {other:?}"),
    };
    assert_eq!(
        row_fields,
        [
            "resource", "pointer", "label", "ordinal", "start", "end", "state", "original",
        ]
    );

    let members = model.members().unwrap();
    assert_eq!(members.len(), 69);
    let find = |key: &str| {
        members
            .iter()
            .find(|member| member.key() == key)
            .unwrap_or_else(|| panic!("missing member {key}"))
    };
    let resource = find("alint:plugin/types/row.resource");
    assert_eq!(resource.kind, MemberKind::Field);
    assert_eq!(resource.ty.as_deref(), Some("types/resource-info"));
    let list = find("alint:plugin/types/node.list");
    assert_eq!(list.kind, MemberKind::VariantCase);
    assert_eq!(list.ty.as_deref(), Some("list<u32>"));
    let record = find("alint:plugin/types/node.record");
    assert_eq!(record.kind, MemberKind::VariantCase);
    assert_eq!(record.ty.as_deref(), Some("list<types/record-entry>"));
    let extract = find("alint:plugin/adapter-api.extract");
    assert_eq!(extract.kind, MemberKind::Method);
    assert_eq!(
        extract.ty.as_deref(),
        Some("result<types/adapter-output,types/adapter-error>")
    );
    let input = find("alint:plugin/adapter-api#extract.input");
    assert_eq!(input.kind, MemberKind::Param);
    assert_eq!(input.ty.as_deref(), Some("types/adapter-input"));
    let analyze = find("alint:plugin/analyzer-api.analyze");
    assert_eq!(
        analyze.ty.as_deref(),
        Some("result<list<types/finding>,types/analyzer-error>")
    );
    assert_eq!(
        find("alint:plugin/analyzer-api#analyze.input")
            .ty
            .as_deref(),
        Some("types/analyzer-input")
    );
    assert_eq!(
        find("alint:plugin/types/capabilities.host-keyring")
            .ty
            .as_deref(),
        Some("bool")
    );
    assert_eq!(
        find("alint:plugin/types/limits.max-rows").ty.as_deref(),
        Some("u64")
    );
    assert_eq!(
        find("alint:plugin/adapter-api.id").ty.as_deref(),
        Some("string")
    );
    assert_eq!(
        find("alint:plugin/analyzer-api.get-capabilities")
            .ty
            .as_deref(),
        Some("types/capabilities")
    );
}

#[test]
fn alint_contract_source_note_records_adaptations() {
    let note = fs::read_to_string(fixture_dir().join("README.md")).unwrap();
    assert!(note.contains("ADR-0024"), "{note}");
    assert!(note.contains("a13703a25a83735c84bc2ad577a397b02d4d08f6"));
    assert!(note.contains("Design findings"));
    assert!(note.contains("No subset refusal was required"));
}
