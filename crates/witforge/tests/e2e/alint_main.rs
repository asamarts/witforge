use glue::GlueError;
use glue::v0::adapter_api::functions::{
    extract, get_capabilities as adapter_caps, id as adapter_id, redacts_by_default,
};
use glue::v0::analyzer_api::functions::{
    analyze, get_capabilities as analyzer_caps, id as analyzer_id,
};
use glue::v0::types::{node, resource_info};
use plain::v0_1::types::{
    AdapterError, AdapterInput, AdapterOutput, AnalyzerError, AnalyzerInput, Capabilities, Finding,
    FlatValue, LimitError, Limits, Node, PointerSegment, RecordEntry, RelatedLocation, ResourceInfo,
    Row, RowState,
};
use proto::v0 as p;

fn roundtrip<T>(value: &T) -> T
where
    T: ::prost::Message
        + ::std::default::Default
        + ::serde::Serialize
        + for<'de> ::serde::Deserialize<'de>,
{
    let binary = value.encode_to_vec();
    let decoded = T::decode(binary.as_slice()).unwrap();
    let json = serde_json::to_string(&decoded).unwrap();
    serde_json::from_str(&json).unwrap()
}

fn main() {
    let resource = ResourceInfo {
        path: "src/lib.rs".to_string(),
        indexed: true,
        index: Some(3),
    };
    let flat = FlatValue {
        nodes: vec![
            Node::Null,
            Node::Boolean(true),
            Node::Number("12".to_string()),
            Node::Text("text".to_string()),
            Node::Bytes(vec![0, 1, 2]),
            Node::Path("src/lib.rs".to_string()),
            Node::List(vec![0, 1]),
            Node::Record(vec![RecordEntry {
                name: "field".to_string(),
                node: 2,
            }]),
        ],
    };
    let row = Row {
        resource: resource.clone(),
        pointer: Some(vec![
            PointerSegment::Key("field".to_string()),
            PointerSegment::Index(2),
        ]),
        label: Some("field".to_string()),
        ordinal: 0,
        start: Some(10),
        end: Some(20),
        state: RowState::Value(flat.clone()),
        original: Some(flat.clone()),
    };
    let output = AdapterOutput {
        rows: vec![row.clone()],
        non_strings: vec!["number".to_string()],
        notes: vec!["note".to_string()],
    };
    let input = AdapterInput {
        resource: resource.clone(),
        bytes: vec![b'{', b'}'],
        limits: Limits { max_rows: 100 },
    };
    let capabilities = Capabilities {
        spawn_user_command: true,
        host_keyring: false,
    };

    let resource_proto = resource_info::to_proto(&resource);
    assert_eq!(resource_info::from_proto(roundtrip(&resource_proto)).unwrap(), resource);
    let node_proto = node::to_proto(&flat.nodes[6]);
    assert_eq!(node::from_proto(roundtrip(&node_proto)).unwrap(), flat.nodes[6]);

    let mut missing = resource_proto.clone();
    missing.path = None;
    assert!(matches!(
        resource_info::from_proto(missing),
        Err(GlueError::MissingField {
            field: "types/resource-info.path"
        })
    ));
    let mut unknown = node_proto;
    unknown.witforge_value = None;
    assert!(matches!(
        node::from_proto(unknown),
        Err(GlueError::UnknownCase {
            message: "types/node"
        })
    ));

    assert_eq!(adapter_id::METHOD, p::AdapterApiMethod::Id);
    assert_eq!(redacts_by_default::METHOD, p::AdapterApiMethod::RedactsByDefault);
    assert_eq!(adapter_caps::METHOD, p::AdapterApiMethod::GetCapabilities);
    assert_eq!(extract::METHOD, p::AdapterApiMethod::Extract);
    assert_eq!(adapter_id::from_request(roundtrip(&adapter_id::to_request())).unwrap(), ());
    assert_eq!(
        adapter_id::from_response(roundtrip(&adapter_id::to_response(&"adapter".to_string()))).unwrap(),
        "adapter".to_string()
    );
    assert!(
        redacts_by_default::from_response(roundtrip(&redacts_by_default::to_response(&true))).unwrap()
    );
    assert_eq!(
        adapter_caps::from_response(roundtrip(&adapter_caps::to_response(&capabilities))).unwrap(),
        capabilities.clone()
    );
    let request = extract::to_request(&input);
    assert_eq!(extract::from_request(roundtrip(&request)).unwrap(), input);
    let response = extract::to_response(&Ok(output.clone()));
    assert_eq!(
        extract::from_response(roundtrip(&response)).unwrap(),
        Ok(output.clone())
    );
    let error_response = extract::to_response(&Err(AdapterError::Parse("bad".to_string())));
    assert_eq!(
        extract::from_response(roundtrip(&error_response)).unwrap(),
        Err(AdapterError::Parse("bad".to_string()))
    );
    let limit_error = AdapterError::Limit(LimitError {
        limit: 10,
        reached: 11,
    });
    assert_eq!(
        extract::from_response(roundtrip(&extract::to_response(&Err(limit_error.clone())))).unwrap(),
        Err(limit_error)
    );

    let finding = Finding {
        path: Some("src/lib.rs".to_string()),
        message: "message".to_string(),
        line: Some(1),
        column: Some(2),
        related: vec![RelatedLocation {
            path: "src/lib.rs".to_string(),
            label: "related".to_string(),
            line: Some(1),
            column: Some(3),
        }],
        baseline_key: Some("baseline".to_string()),
        sensitive: true,
        analysis_failure: None,
        failure_kind: Some("resource-parse".to_string()),
        not_fixable: true,
    };
    let analyzer_input = AnalyzerInput {
        rule_id: "rule".to_string(),
        rows: vec![row],
        limits: Limits { max_rows: 100 },
    };

    assert_eq!(analyzer_id::METHOD, p::AnalyzerApiMethod::Id);
    assert_eq!(
        analyzer_caps::METHOD,
        p::AnalyzerApiMethod::GetCapabilities
    );
    assert_eq!(analyze::METHOD, p::AnalyzerApiMethod::Analyze);
    assert_eq!(
        analyzer_id::from_response(roundtrip(&analyzer_id::to_response(&"analyzer".to_string())))
            .unwrap(),
        "analyzer".to_string()
    );
    assert_eq!(
        analyzer_caps::from_response(roundtrip(&analyzer_caps::to_response(&capabilities))).unwrap(),
        capabilities
    );
    let analyze_request = analyze::to_request(&analyzer_input);
    assert_eq!(
        analyze::from_request(roundtrip(&analyze_request)).unwrap(),
        analyzer_input
    );
    let findings = vec![finding.clone()];
    assert_eq!(
        analyze::from_response(roundtrip(&analyze::to_response(&Ok(findings.clone())))).unwrap(),
        Ok(findings)
    );
    let analyzer_error = AnalyzerError::Limit(LimitError {
        limit: 10,
        reached: 11,
    });
    assert_eq!(
        analyze::from_response(roundtrip(&analyze::to_response(&Err(analyzer_error.clone()))))
            .unwrap(),
        Err(analyzer_error)
    );
    assert_eq!(
        format!("{}", GlueError::InvalidState { message: "x" }),
        "message `x` is invalid"
    );
}
