---
layout: default
title: JSON projections
---

# JSON projections

witforge can participate in three different JSON shapes. They project the same WIT values but do
not define one interchangeable JSON wire format.

For this WIT:

```wit
record diagnostic {
  start-byte: u32,
  id: u64,
  note: option<string>,
}
```

the default shapes are:

| Projection | Representative value |
|---|---|
| generated Rust with `--serde` | `{"start_byte":4,"id":9,"note":null}` |
| generated Draft 7 JSON Schema | `{"start-byte":4,"id":9,"note":null}` |
| emitted protobuf through proto3-JSON | `{"startByte":4,"id":"9"}` |

## Plain Rust Serde

WIT names become Rust identifiers, so `start-byte` becomes `start_byte`. Derived Serde uses that
Rust spelling. `Option<T>` accepts an omitted field as `None` and serializes `None` as `null`.
Rust integer types serialize as JSON numbers, including `u64` and `s64`.

## Draft 7 JSON Schema

The schema preserves the WIT member spelling, including `start-byte`. Every record property is
required and records set `additionalProperties: false`. `option<T>` permits `null` as the
property's value; it does not make the property omittable. Integer schemas use the WIT type's
numeric minimum and maximum.

The schema describes a WIT-shaped JSON value. It does not describe Serde's Rust-identifier shape
or protobuf's JSON mapping.

## Proto3-JSON

The emitted `.proto` determines this projection. Its standard JSON mapping uses:

- lowerCamelCase output names, so protobuf `start_byte` becomes `startByte`; parsers also accept
  the original protobuf spelling `start_byte`;
- decimal strings for `u64` and `s64`;
- base64 strings for bytes and strings for non-finite floats;
- enum names rather than integer values in normal output;
- omitted fields for absent optional values. A WIT `option<T>` containing `None` is therefore
  normally absent rather than the explicit `null` emitted by Rust Serde.

Parsing valid proto3-JSON does not enforce all WIT semantics. Required presence, narrow integer
ranges, closed enums, and selected variant/result cases still need the checks described in
[Non-Rust implementations](non-rust.html) or the generated Rust glue.

## Application defaults

WIT does not define defaults such as "missing severity means warning." Keep application defaults
in an explicit adapter and test them with fixtures.

## Choosing a JSON boundary

Before adopting generated output for an existing protocol:

1. choose exactly one authoritative JSON profile;
2. test full and minimal payloads, naming, `null`, omission, 64-bit integers, enums, and bytes;
3. keep a custom wire schema or adapter when the existing protocol differs;
4. do not validate one projection with another projection's schema;
5. do not describe a custom application profile as generated unless its actual names and
   presence rules match.

An opt-in naming mode could align future Serde output with WIT names, or a future schema mode could
describe Serde names. Either changes the JSON behavior of generated per-version types and must not
be applied retroactively to a frozen version.
