use glue::GlueError;
use glue::v0::alias::holder;
use glue::v0::types::{all, maybe_list, maybe_maybe, numbers, small, small_alias, small_list_alias, status, singular};
use plain::v0_4::alias::Holder;
use plain::v0_4::types::{All, Caps, Choice, Numbers, Singular, Small, Status};
use proto::v0 as p;

fn main() {
    let display_error = GlueError::MissingField { field: "types/small.value" };
    assert_eq!(
        format!("{display_error}"),
        "field `types/small.value` is missing"
    );
    let _: &dyn std::error::Error = &display_error;

    let small_value = Small {
        value: "hello".to_string(),
    };
    let numbers_value = Numbers {
        u8_value: 255,
        u16_value: 65_535,
        s8_value: -128,
        s16_value: -32_768,
    };
    let encoded = numbers::to_proto(&numbers_value);
    assert_eq!(encoded.u8_value, Some(255));
    assert_eq!(encoded.u16_value, Some(65_535));
    assert_eq!(encoded.s8_value, Some(-128));
    assert_eq!(encoded.s16_value, Some(-32_768));
    assert_eq!(numbers::from_proto(encoded).unwrap(), numbers_value);

    let missing = numbers::from_proto(p::TypesNumbers {
        u8_value: None,
        u16_value: Some(0),
        s8_value: Some(0),
        s16_value: Some(0),
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
        s8_value: Some(0),
        s16_value: Some(0),
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
        s8_value: Some(0),
        s16_value: Some(0),
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
        s8_value: Some(128),
        s16_value: Some(0),
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
}
