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
| generated JSON Schema (Draft 7 / 2020-12) | `{"start-byte":4,"id":9,"note":null}` |
| emitted protobuf through proto3-JSON | `{"startByte":4,"id":"9"}` |

## Plain Rust Serde

`--serde` adds plain, format-independent `#[derive(::serde::Serialize, ::serde::Deserialize)]`
attributes to generated records, enums, variants, and flags. Derived Serde uses the generated Rust
spelling rather than the WIT spelling:

- **Fields and flags:** `start-byte` becomes `"start_byte"`, upper-case words fold to Rust casing
  (`URL` becomes `"url"`), non-raw escaped keywords keep their trailing underscore (`%self` is
  keyed `"self_"`), and raw Rust identifiers drop `r#` (`%type` becomes `r#type` and is keyed
  `"type"`).
- **Enums and variants:** enum cases serialize as PascalCase strings (`done-now` -> `"DoneNow"`),
  payload-less variant cases serialize as unit strings (`"Started"`), and payload variant cases use
  externally tagged single-key objects (`{"Item": {...}}`).
- **Bytes, characters, fixed-length lists, maps, and integers:** `list<u8>` (including byte aliases)
  and `list<u8, N>` serialize as JSON arrays of integers (`[0, 127, 255]`), not base64; arrays with
  length `N > 32` (including inside `Option`, `Vec`, `BTreeMap`, or `Result`) use a private
  generated `#[serde(with = "...")]` helper module with no extra crate dependencies. `char`
  serializes as a single-character JSON string, and `map<K, V>` (`BTreeMap<K, V>`) serializes as a
  JSON object with stringified keys in deterministic key order. Rust integer types serialize as
  exact JSON numbers, including `u64` and `s64` (JavaScript `JSON.parse` loses precision above
  `±2^53`).
- **Reading and evolution:** derived Serde treats an omitted `option<T>` field as `None` (writing
  `None` as `null`) and silently ignores unknown fields. Reading a `v0.1` JSON document directly as
  `v0.2` therefore yields `None` for an added `option<T>` field, but fails with a missing-field
  error for an added `list<T>` or `map<K, V>` field—unlike generated `convert.rs`, which defaults
  `option<T>`, `list<T>`, and `map<K, V>`.
- **`serde_json` losses:** because JSON has no distinct representation for nested nulls or
  non-finite floats, `serde_json` serializes `Some(None)` on nested options (`option<option<T>>`,
  through aliases, or in lists) as `null` and deserializes it as `None`, and serializes `NaN` /
  `±Infinity` as `null` (which fails to deserialize for required fields/lists and deserializes as
  `None` inside `option<f32>`/`option<f64>`). Formats that tag `Option` or encode binary IEEE-754
  floats round-trip both. See `docs/design/emitters.md` section 3.3 for the full mapping table.

## JSON Schema (Draft 7 / Draft 2020-12)

The schema preserves the WIT member spelling, including `start-byte` (Draft 7 is the default with
`definitions`; opt-in Draft 2020-12 uses `$defs` and native `deprecated: true` with the same JSON
value shape). Every record property is
required and records set `additionalProperties: false`. `option<T>` permits `null` as the
property's value; it does not make the property omittable (`option<option<T>>` shares `null` for
`None` and `Some(None)`, as in Serde). Integer schemas use the WIT type's exact numeric `minimum`
and `maximum` (JSON parsers using IEEE 754 doubles are exact only up to `±2^53`, and Draft 7
`integer` accepts `1.0`). `f32` carries its finite bounds (`-3.4028235e38` to `3.4028235e38`)
while accepting in-range decimals that round to `f32`; `NaN` and `±Infinity` have no JSON form.

Every WIT `result`—including `result<T>`, `result<_, E>`, and bare `result`—is a closed single-key
object with `"ok"` or `"err"`, using `null` for a side without a payload (matching payload-less
variant cases):

| WIT value | Generated Rust `--serde` | JSON Schema |
|---|---|---|
| `result<u32, string>` (`Ok(7)` / `Err("e")`) | `{"Ok":7}` / `{"Err":"e"}` | `{"ok":7}` / `{"err":"e"}` |
| `result<u32>` (`Ok(7)` / `Err(())`) | `{"Ok":7}` / `{"Err":null}` | `{"ok":7}` / `{"err":null}` |
| `result<_, string>` (`Ok(())` / `Err("e")`) | `{"Ok":null}` / `{"Err":"e"}` | `{"ok":null}` / `{"err":"e"}` |
| bare `result` (`Ok(())` / `Err(())`) | `{"Ok":null}` / `{"Err":null}` | `{"ok":null}` / `{"err":null}` |

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
[Non-Rust implementations](non-rust.md) or the generated Rust glue.

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
