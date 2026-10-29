use glue::GlueError;
use prost::Message as _;
use glue::v0::alias::holder;
use glue::v0::service::functions::{echo, list_result, many, maybe_result, no_result, ping};
use glue::v0::service::functions::r#type as keyword_func;
use glue::v0::types::{all, maybe_list, maybe_maybe, numbers, small, small_alias, small_list_alias, status, singular};
use plain::v0_4::alias::Holder;
use plain::v0_4::types::{All, Caps, Choice, Numbers, Singular, Small, Status};
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
    let display_error = GlueError::MissingField { field: "types/small.value" };
    assert_eq!(
        format!("{display_error}"),
        "field `types/small.value` is missing"
    );
    let _: &dyn std::error::Error = &display_error;
    assert_eq!(
        format!(
            "{}",
            GlueError::IntegerOutOfRange {
                field: "types/numbers.u8-value",
                value: 256
            }
        ),
        "field `types/numbers.u8-value`: integer 256 is out of range"
    );
    assert_eq!(
        format!(
            "{}",
            GlueError::UnknownEnumValue {
                enumeration: "types/status",
                value: 99
            }
        ),
        "enum `types/status` has unknown value 99"
    );
    assert_eq!(
        format!(
            "{}",
            GlueError::UnknownCase {
                message: "types/choice"
            }
        ),
        "message `types/choice` has no case"
    );
    assert_eq!(
        format!(
            "{}",
            GlueError::InvalidState {
                message: "types/maybe-maybe"
            }
        ),
        "message `types/maybe-maybe` is invalid"
    );

    let small_value = Small {
        value: "hello".to_string(),
    };
    let numbers_value = Numbers {
        u8_value: 255,
        u16_value: 65_535,
        u32_value: 4_294_967_295,
        u64_value: 18_446_744_073_709_551_615,
        s8_value: -128,
        s16_value: -32_768,
        s64_value: -9_223_372_036_854_775_808,
        f32_value: 1.5,
        f64_value: -2.25,
    };
    let encoded = numbers::to_proto(&numbers_value);
    assert_eq!(encoded.u8_value, Some(255));
    assert_eq!(encoded.u16_value, Some(65_535));
    assert_eq!(encoded.s8_value, Some(-128));
    assert_eq!(encoded.s16_value, Some(-32_768));
    assert_eq!(numbers::from_proto(encoded.clone()).unwrap(), numbers_value);
    assert_eq!(
        numbers::from_proto(roundtrip(&encoded)).unwrap(),
        numbers_value
    );
    let small_proto = small::to_proto(&small_value);
    assert_eq!(
        small::from_proto(roundtrip(&small_proto)).unwrap(),
        small_value
    );

    let missing = numbers::from_proto(p::TypesNumbers {
        u8_value: None,
        u16_value: Some(0),
        u32_value: Some(0),
        u64_value: Some(0),
        s8_value: Some(0),
        s16_value: Some(0),
        s64_value: Some(0),
        f32_value: Some(0.0),
        f64_value: Some(0.0),
    })
    .unwrap_err();
    assert!(matches!(
        missing,
        GlueError::MissingField {
            field: "types/numbers.u8-value"
        }
    ));

    let out_of_range = numbers::from_proto(p::TypesNumbers {
        u8_value: Some(256),
        u16_value: Some(0),
        u32_value: Some(0),
        u64_value: Some(0),
        s8_value: Some(0),
        s16_value: Some(0),
        s64_value: Some(0),
        f32_value: Some(0.0),
        f64_value: Some(0.0),
    })
    .unwrap_err();
    assert!(matches!(
        out_of_range,
        GlueError::IntegerOutOfRange {
            field: "types/numbers.u8-value",
            value: 256
        }
    ));
    let out_of_range = numbers::from_proto(p::TypesNumbers {
        u8_value: Some(0),
        u16_value: Some(65_536),
        u32_value: Some(0),
        u64_value: Some(0),
        s8_value: Some(0),
        s16_value: Some(0),
        s64_value: Some(0),
        f32_value: Some(0.0),
        f64_value: Some(0.0),
    })
    .unwrap_err();
    assert!(matches!(
        out_of_range,
        GlueError::IntegerOutOfRange {
            field: "types/numbers.u16-value",
            value: 65_536
        }
    ));
    let out_of_range = numbers::from_proto(p::TypesNumbers {
        u8_value: Some(0),
        u16_value: Some(0),
        u32_value: Some(0),
        u64_value: Some(0),
        s8_value: Some(128),
        s16_value: Some(0),
        s64_value: Some(0),
        f32_value: Some(0.0),
        f64_value: Some(0.0),
    })
    .unwrap_err();
    assert!(matches!(
        out_of_range,
        GlueError::IntegerOutOfRange {
            field: "types/numbers.s8-value",
            value: 128
        }
    ));

    assert!(matches!(
        status::from_proto(0),
        Err(GlueError::UnknownEnumValue {
            enumeration: "types/status",
            value: 0
        })
    ));
    assert!(matches!(
        status::from_proto(99),
        Err(GlueError::UnknownEnumValue {
            enumeration: "types/status",
            value: 99
        })
    ));
    assert_eq!(status::from_proto(1).unwrap(), Status::One);

    let missing_small = small::from_proto(p::TypesSmall { value: None }).unwrap_err();
    assert!(matches!(
        missing_small,
        GlueError::MissingField {
            field: "types/small.value"
        }
    ));

    let unknown_choice = glue::v0::types::choice::from_proto(p::TypesChoice {
        witforge_value: None,
    })
    .unwrap_err();
    assert!(matches!(
        unknown_choice,
        GlueError::UnknownCase {
            message: "types/choice"
        }
    ));

    assert_eq!(
        maybe_list::to_proto(&None),
        None
    );
    assert_eq!(
        maybe_list::to_proto(&Some(Vec::new())),
        Some(p::TypesMaybeList { values: Vec::new() })
    );
    assert_eq!(
        maybe_list::from_proto(Some(p::TypesMaybeList {
            values: vec![p::TypesSmall {
                value: Some(small_value.value.clone())
            }]
        }))
        .unwrap(),
        Some(vec![small_value.clone()])
    );

    assert_eq!(maybe_maybe::to_proto(&None), None);
    assert_eq!(
        maybe_maybe::to_proto(&Some(None)),
        Some(p::TypesMaybeMaybe {
            value: Some(p::types_maybe_maybe::Value::None(p::Empty {}))
        })
    );
    assert_eq!(
        maybe_maybe::from_proto(Some(p::TypesMaybeMaybe {
            value: Some(p::types_maybe_maybe::Value::Some("q".to_string()))
        }))
        .unwrap(),
        Some(Some("q".to_string()))
    );
    let invalid_direct = maybe_maybe::from_proto(Some(p::TypesMaybeMaybe { value: None })).unwrap_err();
    assert!(matches!(
        invalid_direct,
        GlueError::InvalidState {
            message: "types/maybe-maybe"
        }
    ));

    for value in [
        Singular::MaybeString(None),
        Singular::MaybeString(Some("s".to_string())),
        Singular::MaybeSmall(None),
        Singular::MaybeSmall(Some(small_value.clone())),
        Singular::MaybeList(None),
        Singular::MaybeList(Some(Vec::new())),
        Singular::MaybeList(Some(vec![small_value.clone()])),
        Singular::MaybeMaybe(None),
        Singular::MaybeMaybe(Some(None)),
        Singular::MaybeMaybe(Some(Some("n".to_string()))),
    ] {
        let encoded = singular::to_proto(&value);
        assert_eq!(singular::from_proto(encoded).unwrap(), value);
    }
    let invalid_singular = singular::from_proto(p::TypesSingular {
        witforge_value: Some(p::types_singular::WitforgeValue::MaybeString(
            p::WitforgeOptionOfString { value: None },
        )),
    })
    .unwrap_err();
    assert!(matches!(
        invalid_singular,
        GlueError::InvalidState {
            message: "types/singular"
        }
    ));

    let all_value = All {
        numbers: numbers_value.clone(),
        status: Status::Two,
        choice: Choice::Small(small_value.clone()),
        flags: Caps { a: true, b: false },
        bytes: vec![0, 1, 255],
        items: vec![small_value.clone()],
        maybe: Some("m".to_string()),
        maybe_small: Some(small_value.clone()),
        maybe_list: Some(vec![small_value.clone()]),
        maybe_maybe: Some(Some("n".to_string())),
        outcome: Ok(small_value.clone()),
        only_ok: Ok(small_value.clone()),
        only_err: Err("only".to_string()),
        maybe_bytes: Some(vec![1, 2, 3]),
        maybe_status: Some(Status::One),
        maybe_choice: Some(Choice::Empty),
        maybe_caps: Some(Caps { a: false, b: true }),
        maybe_outcome: Some(Ok(small_value.clone())),
        nested: vec![Some(Ok(7)), None, Some(Err("bad".to_string()))],
        alias_small: small_value.clone(),
        alias_list: vec![small_value.clone()],
        alias_maybe: Some("alias".to_string()),
    };
    let encoded = all::to_proto(&all_value);
    assert_eq!(all::from_proto(encoded.clone()).unwrap(), all_value);

    let mut empty = all_value.clone();
    empty.bytes = Vec::new();
    empty.items = Vec::new();
    assert_eq!(all::from_proto(all::to_proto(&empty)).unwrap(), empty);

    let missing_numbers = all::from_proto(p::TypesAll {
        numbers: None,
        ..encoded.clone()
    })
    .unwrap_err();
    assert!(matches!(
        missing_numbers,
        GlueError::MissingField {
            field: "types/all.numbers"
        }
    ));

    let missing_result = all::from_proto(p::TypesAll {
        outcome: None,
        ..encoded.clone()
    })
    .unwrap_err();
    assert!(matches!(
        missing_result,
        GlueError::MissingField {
            field: "types/all.outcome"
        }
    ));

    let optional_absent = all::from_proto(p::TypesAll {
        maybe_list: None,
        maybe_maybe: None,
        ..encoded
    })
    .unwrap();
    assert_eq!(optional_absent.maybe_list, None);
    assert_eq!(optional_absent.maybe_maybe, None);

    let unknown_outcome = glue::v0::types::outcome::from_proto(p::TypesOutcome { value: None })
        .unwrap_err();
    assert!(matches!(
        unknown_outcome,
        GlueError::UnknownCase {
            message: "types/outcome"
        }
    ));

    let alias_small = small_alias::to_proto(&small_value);
    assert_eq!(small_alias::from_proto(alias_small).unwrap(), small_value);
    let alias_list = small_list_alias::to_proto(&vec![small_value.clone()]);
    assert_eq!(
        small_list_alias::from_proto(alias_list).unwrap(),
        vec![small_value.clone()]
    );

    let holder_value = Holder {
        one: small_value.clone(),
        many: vec![small_value.clone()],
        maybe: Some("holder".to_string()),
    };
    assert_eq!(
        holder::from_proto(holder::to_proto(&holder_value)).unwrap(),
        holder_value
    );

    assert_eq!(ping::METHOD, p::ServiceMethod::Ping);
    assert_eq!(echo::METHOD, p::ServiceMethod::Echo);
    assert_eq!(many::METHOD, p::ServiceMethod::Many);
    assert_eq!(no_result::METHOD, p::ServiceMethod::NoResult);
    assert_eq!(maybe_result::METHOD, p::ServiceMethod::MaybeResult);
    assert_eq!(list_result::METHOD, p::ServiceMethod::ListResult);
    assert_eq!(keyword_func::METHOD, p::ServiceMethod::Type);

    let ping_request = ping::to_request();
    assert_eq!(ping::from_request(ping_request).unwrap(), ());
    assert_eq!(
        ping::from_response(ping::to_response(&"pong".to_string())).unwrap(),
        "pong".to_string()
    );
    assert!(matches!(
        ping::from_response(p::ServicePingResponse { result: None }),
        Err(GlueError::MissingField {
            field: "service#ping.result"
        })
    ));

    let echo_request = echo::to_request(&"hello".to_string());
    assert_eq!(echo_request.value, Some("hello".to_string()));
    assert_eq!(
        echo::from_request(echo_request).unwrap(),
        "hello".to_string()
    );
    assert!(matches!(
        echo::from_request(p::ServiceEchoRequest { value: None }),
        Err(GlueError::MissingField {
            field: "service#echo.value"
        })
    ));

    let many_request = many::to_request(
        &small_value,
        &Some("m".to_string()),
        &vec![small_value.clone()],
        &true,
    );
    assert_eq!(many_request.maybe, Some("m".to_string()));
    assert_eq!(many_request.flag, Some(true));
    let (item, maybe, items, flag) = many::from_request(many_request.clone()).unwrap();
    assert_eq!(item, small_value);
    assert_eq!(maybe, Some("m".to_string()));
    assert_eq!(items, vec![small_value.clone()]);
    assert!(flag);
    let optional_many = many::from_request(p::ServiceManyRequest {
        item: Some(p::TypesSmall {
            value: Some(small_value.value.clone()),
        }),
        maybe: None,
        items: Vec::new(),
        flag: Some(false),
    })
    .unwrap();
    assert_eq!(optional_many.1, None);
    assert_eq!(optional_many.2, Vec::<Small>::new());
    assert!(!optional_many.3);
    assert!(matches!(
        many::from_request(p::ServiceManyRequest {
            item: None,
            maybe: None,
            items: Vec::new(),
            flag: Some(false),
        }),
        Err(GlueError::MissingField {
            field: "service#many.item"
        })
    ));
    let many_ok = many::from_response(many::to_response(&Ok(vec![small_value.clone()]))).unwrap();
    assert_eq!(many_ok, Ok(vec![small_value.clone()]));
    let many_err = many::from_response(many::to_response(&Err("bad".to_string()))).unwrap();
    assert_eq!(many_err, Err("bad".to_string()));

    let no_result_request = no_result::to_request(&7);
    assert_eq!(no_result_request.value, Some(7));
    assert_eq!(no_result::from_request(no_result_request).unwrap(), 7);
    assert_eq!(no_result::from_response(no_result::to_response()).unwrap(), ());

    assert_eq!(
        maybe_result::from_response(maybe_result::to_response(&None)).unwrap(),
        None
    );
    assert_eq!(
        maybe_result::from_response(maybe_result::to_response(&Some("x".to_string()))).unwrap(),
        Some("x".to_string())
    );
    assert_eq!(
        maybe_result::from_response(p::ServiceMaybeResultResponse { result: None }).unwrap(),
        None
    );

    assert_eq!(
        list_result::from_response(list_result::to_response(&Vec::new())).unwrap(),
        Vec::<Small>::new()
    );
    assert_eq!(
        list_result::from_response(list_result::to_response(&vec![small_value.clone()])).unwrap(),
        vec![small_value.clone()]
    );
    let keyword_request = keyword_func::to_request(&9);
    assert_eq!(keyword_request.self_, Some(9));
    assert_eq!(keyword_func::from_request(roundtrip(&keyword_request)).unwrap(), 9);
    assert_eq!(
        keyword_func::from_response(roundtrip(&keyword_func::to_response(&11))).unwrap(),
        11
    );

    let all_proto = all::to_proto(&all_value);
    assert_eq!(all::from_proto(roundtrip(&all_proto)).unwrap(), all_value);
    let singular_proto = singular::to_proto(&Singular::MaybeMaybe(Some(None)));
    assert_eq!(
        singular::from_proto(roundtrip(&singular_proto)).unwrap(),
        Singular::MaybeMaybe(Some(None))
    );
    let holder_proto = holder::to_proto(&holder_value);
    assert_eq!(
        holder::from_proto(roundtrip(&holder_proto)).unwrap(),
        holder_value
    );
    let outcome_proto = glue::v0::types::outcome::to_proto(&Ok(small_value.clone()));
    assert_eq!(
        glue::v0::types::outcome::from_proto(roundtrip(&outcome_proto)).unwrap(),
        Ok(small_value.clone())
    );
    let empty_choice = glue::v0::types::choice::to_proto(&Choice::Empty);
    assert_eq!(
        glue::v0::types::choice::from_proto(roundtrip(&empty_choice)).unwrap(),
        Choice::Empty
    );

    let many_request = many::to_request(&small_value, &Some("m".to_string()), &vec![small_value.clone()], &true);
    let many_decoded = many::from_request(roundtrip(&many_request)).unwrap();
    assert_eq!(many_decoded.1, Some("m".to_string()));
    assert_eq!(many_decoded.2, vec![small_value.clone()]);
    assert!(many_decoded.3);
    let many_response = many::to_response(&Err("bad".to_string()));
    assert_eq!(
        many::from_response(roundtrip(&many_response)).unwrap(),
        Err("bad".to_string())
    );

    let ping_request = ping::to_request();
    assert_eq!(ping::from_request(roundtrip(&ping_request)).unwrap(), ());
    let ping_response = ping::to_response(&"pong".to_string());
    assert_eq!(
        ping::from_response(roundtrip(&ping_response)).unwrap(),
        "pong".to_string()
    );
    let echo_request = echo::to_request(&"hello".to_string());
    assert_eq!(
        echo::from_request(roundtrip(&echo_request)).unwrap(),
        "hello".to_string()
    );
    let echo_response = echo::to_response(&"world".to_string());
    assert_eq!(
        echo::from_response(roundtrip(&echo_response)).unwrap(),
        "world".to_string()
    );
    let maybe_request = maybe_result::to_request();
    assert_eq!(maybe_result::from_request(roundtrip(&maybe_request)).unwrap(), ());
    let list_request = list_result::to_request();
    assert_eq!(list_result::from_request(roundtrip(&list_request)).unwrap(), ());
    let no_result_request = no_result::to_request(&7);
    assert_eq!(no_result::from_request(roundtrip(&no_result_request)).unwrap(), 7);
    let no_result_response = no_result::to_response();
    assert_eq!(no_result::from_response(roundtrip(&no_result_response)).unwrap(), ());
    let maybe_response = maybe_result::to_response(&Some("m".to_string()));
    assert_eq!(
        maybe_result::from_response(roundtrip(&maybe_response)).unwrap(),
        Some("m".to_string())
    );
    let list_response = list_result::to_response(&vec![small_value.clone()]);
    assert_eq!(
        list_result::from_response(roundtrip(&list_response)).unwrap(),
        vec![small_value.clone()]
    );

    let missing_json: p::ServiceEchoRequest = serde_json::from_str("{}").unwrap();
    assert!(matches!(
        echo::from_request(missing_json),
        Err(GlueError::MissingField {
            field: "service#echo.value"
        })
    ));
    let null_json: p::ServiceEchoRequest = serde_json::from_str(r#"{"value":null}"#).unwrap();
    assert!(matches!(
        echo::from_request(null_json),
        Err(GlueError::MissingField {
            field: "service#echo.value"
        })
    ));
    let optional_absent_json: p::ServiceManyRequest =
        serde_json::from_str(r#"{"item":{"value":"x"},"flag":false}"#).unwrap();
    let (item, maybe, items, flag) = many::from_request(optional_absent_json).unwrap();
    assert_eq!(item.value, "x");
    assert_eq!(maybe, None);
    assert!(items.is_empty());
    assert!(!flag);
    let missing_item_json: p::ServiceManyRequest =
        serde_json::from_str(r#"{"flag":false}"#).unwrap();
    assert!(matches!(
        many::from_request(missing_item_json),
        Err(GlueError::MissingField {
            field: "service#many.item"
        })
    ));
    let absent_response: p::ServiceMaybeResultResponse =
        serde_json::from_str("{}").unwrap();
    assert_eq!(maybe_result::from_response(absent_response).unwrap(), None);
    let missing_response: p::ServiceEchoResponse = serde_json::from_str("{}").unwrap();
    assert!(matches!(
        echo::from_response(missing_response),
        Err(GlueError::MissingField {
            field: "service#echo.result"
        })
    ));
}
