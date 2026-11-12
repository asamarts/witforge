---
layout: default
title: Non-Rust implementations
---

# Non-Rust implementations

witforge emits protobuf schema for cross-language values and calls. Use the normal protobuf
generator for the target language, such as `protoc-gen-go`, `ts-proto`, `grpcio-tools`, or the
standard Java, Kotlin, C#, C++, or Swift generator. witforge does not generate language SDKs,
services, or transport framing.

The generated protobuf type alone is not the complete WIT value contract. Some WIT rules cannot
be expressed by proto3 syntax and are enforced by witforge's generated Rust glue. A non-Rust host
or a protocol with two non-Rust endpoints must enforce the same rules explicitly.

## Decode profile

| WIT value | Protobuf projection | Required implementation behavior |
|---|---|---|
| `u8`, `u16` | `uint32` | Reject values above the WIT type's maximum. |
| `s8`, `s16` | `sint32` | Reject values outside the WIT type's signed range. |
| required scalar, string, enum, or `list<u8>` | proto3 `optional` scalar/string/enum/bytes | Reject absence. A scalar's protobuf default is not a substitute for presence. |
| `option<T>` | the same optional or wrapper shape used for `T` | Permit absence as `None`; validate a present payload normally. |
| required record, variant, flags, result, parameter, or message result | message field | Reject absence. |
| `list<T>` | `repeated T`, or a wrapper when nested | Empty is valid. Validate every element recursively. |
| enum | protobuf enum with `UNSPECIFIED = 0` | Reject zero and unknown numeric values; WIT enums are closed. |
| variant or result | message with a `oneof` | Require one known case. Reject an unset or unknown case. |
| flags | message with one `bool` per flag | Treat missing/default `false` as an unset flag. |
| nested option/list/result wrapper | synthesized message | Reject wrapper states that the WIT shape cannot represent. |

These rules apply to record fields, function parameters, and function results. They also apply
after protobuf binary or proto3-JSON parsing. Language protobuf libraries commonly preserve an
unknown enum number or expose a `uint32` without the narrower WIT range; successful protobuf
parsing therefore does not establish a valid WIT value.

A Rust endpoint using generated glue returns `GlueError` for these cases. Other implementations
should expose errors appropriate to their application while preserving the same accept/reject
boundary.

## Conformance tests

For every independently implemented contract, keep fixtures that cover:

- each narrow integer's minimum, maximum, and one out-of-range value;
- every required optional/message field both present and absent;
- `option<T>` absent and present;
- enum zero, every known case, and an unknown number;
- every variant/result case and an unset `oneof`;
- empty and non-empty lists, including nested wrappers;
- protobuf binary and proto3-JSON when both transports are supported.

Run the fixtures through every language implementation and through a Rust implementation using
witforge glue. Generated-file `--check` proves schema reproducibility; it does not replace these
behavioral tests.

## JSON is a separate choice

Proto3-JSON follows protobuf naming and presence rules. It is not the JSON shape described by
witforge's Draft 7 JSON Schema and it is not necessarily the shape produced by Serde on generated
plain Rust types. See [JSON projections](json-projections.html) before choosing a JSON boundary.

Protovalidate annotations and generated cross-language fixture bundles are not emitted today.
Adding either would expand the generated contract and runtime/tooling requirements, so they should
be introduced only behind an explicit option and with a real adopter validating the result.
