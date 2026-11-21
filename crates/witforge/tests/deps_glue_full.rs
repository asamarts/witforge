//! Full dependency glue compilation and round-trip.

use std::fs;
use std::path::{Path, PathBuf};

use witforge::emit::{glue, proto, rust};
use witforge::load;
use witforge::model;

mod support;
use support::{ScratchDir, compile_and_run_wit_root};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deps-full/wit")
}

#[test]
fn full_dependency_glue_compiles_and_roundtrips() {
    let root = fixture();
    let contract = load(&root.join("v0.1")).unwrap();
    let model = model::build(&contract).unwrap();
    assert_eq!(model.dependencies.len(), 3);

    compile_and_run_wit_root(
        "deps-full",
        &root,
        false,
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
        "full dependency glue compile/run",
    );
}

fn strip_header(s: &str) -> &str {
    s.split_once('\n').map_or(s, |(_, rest)| rest)
}

#[allow(clippy::too_many_lines)]
#[test]
fn dependency_structural_and_byte_list_aliases_compile_and_roundtrip() {
    let scratch = ScratchDir::new("deps-struct-work");
    let out = scratch.path();
    let wit_root = out.join("wit");
    let v01 = wit_root.join("v0.1");
    fs::create_dir_all(v01.join("deps/leaf")).unwrap();
    fs::create_dir_all(v01.join("deps/shared")).unwrap();

    let leaf_wit = "\
package ex:leaf@0.1.0;

interface types {
  type leaf-nested = list<list<u16>>;
  type leaf-opt-list = option<list<u16>>;
  type leaf-byte = u8;
  type leaf-blob = list<leaf-byte>;
}
";
    let shared_wit = "\
package ex:shared@0.1.0;

interface types {
  use ex:leaf/types@0.1.0.{leaf-nested, leaf-opt-list, leaf-blob};

  record item { id: u16, label: string }
  type simple-list = list<u16>;
  type nested = list<list<u16>>;
  type nested-alias = nested;
  type nested-alias2 = nested-alias;
  type nested-items = list<list<item>>;
  type opt-list = option<list<u16>>;
  type opt-opt = option<option<u16>>;
  type opt-nested = option<list<list<u16>>>;
  type list-of-opt = list<option<u16>>;
  type list-of-opt-list = list<opt-list>;
  type list-of-anon-opt-list = list<option<list<u16>>>;
  type list-of-inline-res = list<result<u16, string>>;
  type named-res = result<list<u16>, string>;
  type byte = u8;
  type blob = list<u8>;
  type blob-via-alias = list<byte>;
  type mid-alias = leaf-nested;
  type mid-opt-alias = leaf-opt-list;
  type mid-wraps-leaf = list<leaf-nested>;
  type mid-blob = leaf-blob;
  type unused-nested = list<list<string>>;
}
";
    fs::write(v01.join("deps/leaf/types.wit"), leaf_wit).unwrap();
    fs::write(v01.join("deps/shared/types.wit"), shared_wit).unwrap();
    fs::write(
        v01.join("contract.wit"),
        "\
package ex:root@0.1.0;

interface api {
  use ex:shared/types@0.1.0.{
    item,
    simple-list,
    nested,
    nested-alias2,
    nested-items,
    opt-list,
    opt-opt,
    opt-nested,
    list-of-opt,
    list-of-opt-list,
    list-of-anon-opt-list,
    list-of-inline-res,
    named-res,
    blob,
    blob-via-alias,
    mid-alias,
    mid-opt-alias,
    mid-wraps-leaf,
    mid-blob,
  };

  type r-opt-nested = option<nested>;
  type r-opt-opt-list = option<opt-list>;
  type r-opt-opt-opt = option<opt-opt>;
  type r-local-nested = list<list<u16>>;

  variant byte-variant {
    raw(blob),
    aliased(blob-via-alias),
    transitive(mid-blob),
  }

  variant struct-variant {
    n(nested),
    ol(opt-list),
    oo(opt-opt),
    lo(list-of-opt),
  }

  record holder {
    ctrl-item: item,
    ctrl-list: simple-list,
    dep-nested: nested,
    dep-nested-alias: nested-alias2,
    dep-nested-items: nested-items,
    local-nested: list<list<u16>>,
    local-nested-named: r-local-nested,
    root-nested-dep-items: list<list<item>>,
    root-opt-dep-items: option<list<item>>,
    root-list-opt-dep-items: list<option<item>>,
    dep-opt-list: opt-list,
    dep-opt-opt: opt-opt,
    dep-opt-nested: opt-nested,
    dep-list-of-opt: list-of-opt,
    dep-list-of-opt-list: list-of-opt-list,
    dep-list-of-anon-opt-list: list-of-anon-opt-list,
    list-of-dep-list-of-opt-list: list<list-of-opt-list>,
    list-of-dep-list-of-anon-opt-list: list<list-of-anon-opt-list>,
    dep-list-of-res: list-of-inline-res,
    dep-named-res: named-res,
    list-of-dep-nested: list<nested>,
    list-of-local-triple: list<list<list<u16>>>,
    list-of-dep-opt-list: list<opt-list>,
    list-of-dep-opt-opt: list<opt-opt>,
    anon-opt-dep-opt-list: option<opt-list>,
    named-opt-dep-nested: r-opt-nested,
    named-opt-dep-opt-list: r-opt-opt-list,
    named-opt-dep-opt-opt: r-opt-opt-opt,
    b-field: blob,
    b-list: list<blob>,
    b-opt: option<blob>,
    b-alias-field: blob-via-alias,
    b-alias-list: list<blob-via-alias>,
    b-alias-opt: option<blob-via-alias>,
    b-res: result<blob, blob-via-alias>,
    b-var: byte-variant,
    s-var: struct-variant,
    trans-alias: mid-alias,
    trans-opt: mid-opt-alias,
    trans-wraps: mid-wraps-leaf,
    trans-blob: mid-blob,
  }

  echo-bytes: func(a: blob, b: blob-via-alias, c: mid-blob) -> blob-via-alias;
  echo-nested: func(a: nested, b: opt-list) -> opt-opt;
}
",
    )
    .unwrap();

    let model = model::build(&load(&v01).unwrap()).unwrap();
    let mut lock = witforge::lock::WireLock::default();
    lock.reconcile(&model.members().unwrap()).unwrap();
    fs::write(v01.join("wire.lock"), lock.render()).unwrap();

    let plain_plan = rust::plan(&wit_root).unwrap();
    let proto_plan = proto::plan(&wit_root).unwrap();
    let glue_plan = glue::plan(&wit_root, "crate::plain", "crate::proto").unwrap();

    plain_plan.write(&out.join("plain")).unwrap();
    proto_plan.write(&out.join("proto-work")).unwrap();
    glue_plan.write(&out.join("glue")).unwrap();

    // Two roots sharing the exact same dependency must produce byte-identical dependency
    // schemas and bindings (modulo the line-1 root ownership header).
    let root2 = out.join("wit2");
    let root2_v01 = root2.join("v0.1");
    fs::create_dir_all(root2_v01.join("deps/leaf")).unwrap();
    fs::create_dir_all(root2_v01.join("deps/shared")).unwrap();
    fs::write(root2_v01.join("deps/leaf/types.wit"), leaf_wit).unwrap();
    fs::write(root2_v01.join("deps/shared/types.wit"), shared_wit).unwrap();
    fs::write(
        root2_v01.join("contract.wit"),
        "package ex:otherroot@0.1.0;\n\
         interface api {\n\
           use ex:shared/types@0.1.0.{item};\n\
           record small { x: item }\n\
         }\n",
    )
    .unwrap();
    let m2 = model::build(&load(&root2_v01).unwrap()).unwrap();
    let mut lock2 = witforge::lock::WireLock::default();
    lock2.reconcile(&m2.members().unwrap()).unwrap();
    fs::write(root2_v01.join("wire.lock"), lock2.render()).unwrap();
    let plain_plan2 = rust::plan(&root2).unwrap();
    let proto_plan2 = proto::plan(&root2).unwrap();
    let glue_plan2 = glue::plan(&root2, "crate::plain", "crate::proto").unwrap();
    for dep_proto in ["ex.shared.v0.proto", "ex.leaf.v0.proto"] {
        assert_eq!(
            strip_header(&proto_plan.files()[dep_proto]),
            strip_header(&proto_plan2.files()[dep_proto]),
            "dependency proto `{dep_proto}` differed between roots"
        );
    }
    assert_eq!(
        strip_header(&plain_plan.files()["witforge_deps.rs"]),
        strip_header(&plain_plan2.files()["witforge_deps.rs"]),
        "plain witforge_deps.rs differed between roots"
    );
    assert_eq!(
        strip_header(&glue_plan.files()["witforge_deps.rs"]),
        strip_header(&glue_plan2.files()["witforge_deps.rs"]),
        "glue witforge_deps.rs differed between roots"
    );

    // Compile all generated `.proto` files via the shared harness and execute the round-trip.
    compile_and_run_wit_root(
        "deps-struct",
        &wit_root,
        false,
        r#"
fn main() {
    use plain::v0_1::api::{ByteVariant, Holder, StructVariant};
    use plain::witforge_deps::ex_shared::types::Item;

    let item = |id: u16, label: &str| Item { id, label: label.to_string() };
    let holder = Holder {
        ctrl_item: item(1, "a"),
        ctrl_list: vec![1, 2, 3],
        dep_nested: vec![vec![1, 2], vec![]],
        dep_nested_alias: vec![vec![3, 4]],
        dep_nested_items: vec![vec![item(2, "b")], vec![]],
        local_nested: vec![vec![5, 6]],
        local_nested_named: vec![vec![7, 8]],
        root_nested_dep_items: vec![vec![item(3, "c")]],
        root_opt_dep_items: Some(vec![item(4, "d")]),
        root_list_opt_dep_items: vec![Some(item(5, "e")), None],
        dep_opt_list: Some(vec![9, 10]),
        dep_opt_opt: Some(Some(11)),
        dep_opt_nested: Some(vec![vec![12], vec![]]),
        dep_list_of_opt: vec![Some(13), None],
        dep_list_of_opt_list: vec![Some(vec![40, 41]), None],
        dep_list_of_anon_opt_list: vec![Some(vec![42]), None],
        list_of_dep_list_of_opt_list: vec![vec![Some(vec![43]), None]],
        list_of_dep_list_of_anon_opt_list: vec![vec![Some(vec![44]), None]],
        dep_list_of_res: vec![Ok(14), Err("bad".into())],
        dep_named_res: Ok(vec![15, 16]),
        list_of_dep_nested: vec![vec![vec![17]], vec![]],
        list_of_local_triple: vec![vec![vec![18, 19]]],
        list_of_dep_opt_list: vec![Some(vec![20]), None],
        list_of_dep_opt_opt: vec![Some(Some(21)), Some(None), None],
        anon_opt_dep_opt_list: Some(Some(vec![22])),
        named_opt_dep_nested: Some(vec![vec![23, 24]]),
        named_opt_dep_opt_list: Some(Some(vec![25])),
        named_opt_dep_opt_opt: Some(Some(None)),
        b_field: vec![1, 2, 3],
        b_list: vec![vec![4, 5], vec![]],
        b_opt: Some(vec![6, 7]),
        b_alias_field: vec![8, 9],
        b_alias_list: vec![vec![10], vec![11, 12]],
        b_alias_opt: Some(vec![13]),
        b_res: Err(vec![14, 15]),
        b_var: ByteVariant::Transitive(vec![16, 17]),
        s_var: StructVariant::Ol(Some(vec![26, 27])),
        trans_alias: vec![vec![28, 29]],
        trans_opt: Some(vec![30, 31]),
        trans_wraps: vec![vec![vec![32]], vec![]],
        trans_blob: vec![33, 34],
    };

    let encoded = glue::v0::api::holder::to_proto(&holder);
    let decoded = glue::v0::api::holder::from_proto(encoded).unwrap();
    assert_eq!(decoded, holder);

    // Unreferenced dependency structural alias also compiles and roundtrips.
    let unused = vec![vec!["x".to_string(), "y".to_string()]];
    let unused_p = glue::witforge_deps::ex_shared::types::unused_nested::to_proto(&unused);
    assert_eq!(
        glue::witforge_deps::ex_shared::types::unused_nested::from_proto(unused_p).unwrap(),
        unused
    );

    // Function parameters and results with foreign byte-list and structural aliases.
    let breq = glue::v0::api::functions::echo_bytes::to_request(
        &vec![1, 2],
        &vec![3, 4],
        &vec![5, 6],
    );
    assert_eq!(
        glue::v0::api::functions::echo_bytes::from_request(breq).unwrap(),
        (vec![1, 2], vec![3, 4], vec![5, 6])
    );
    let bresp = glue::v0::api::functions::echo_bytes::to_response(&vec![7, 8]);
    assert_eq!(
        glue::v0::api::functions::echo_bytes::from_response(bresp).unwrap(),
        vec![7, 8]
    );

    let nreq = glue::v0::api::functions::echo_nested::to_request(
        &vec![vec![1, 2]],
        &Some(vec![3, 4]),
    );
    assert_eq!(
        glue::v0::api::functions::echo_nested::from_request(nreq).unwrap(),
        (vec![vec![1, 2]], Some(vec![3, 4]))
    );
    let nresp = glue::v0::api::functions::echo_nested::to_response(&Some(None));
    assert_eq!(
        glue::v0::api::functions::echo_nested::from_response(nresp).unwrap(),
        Some(None)
    );
}
"#,
        "dependency structural and byte-list aliases glue compile/run",
    );
    scratch.finish();
}
