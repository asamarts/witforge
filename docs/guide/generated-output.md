# Generated output

This page shows representative output produced by the current repository build from the committed
[`examples/single`](https://github.com/asamarts/witforge/tree/main/examples/single) contract. The
repository commits these files and CI runs every generator with `--check`, so the linked files are
executable examples rather than illustrative pseudocode.

```sh
witforge rust --config examples/single/witforge.toml
witforge proto --config examples/single/witforge.toml
witforge schema --config examples/single/witforge.toml
witforge glue --config examples/single/witforge.toml
```

The source WIT defines an `item` record, an enum, flags, a variant, and one interface function.
Its [`wire.lock`](https://github.com/asamarts/witforge/blob/main/examples/single/wit/v0.1/wire.lock)
assigns the field, case, parameter, and method numbers shown below. Every fenced generated-output
excerpt is checked line-for-line against the linked committed file; generated comments,
unrelated definitions, and surrounding module context may be omitted.

## Plain Rust

The Rust emitter writes owned values and one trait per WIT interface that declares functions. WIT names become Rust
identifiers; no Wasm runtime types appear in this output.

```rust
pub mod types {
    #[derive(Debug, Clone, PartialEq)]
    pub struct Item {
        pub id: u64,
        pub name: ::std::string::String,
        pub tags: ::std::vec::Vec<::std::string::String>,
        pub note: ::std::option::Option<::std::string::String>,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum Status {
        Accepted,
        Rejected,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum Event {
        Started,
        Item(Item),
        Finished(Status),
    }
}

pub mod ingest {
    pub type Item = super::types::Item;
    pub type Event = super::types::Event;

    pub trait Ingest {
        fn submit(&self, item: Item) -> Event;
    }
}
```

Full files: [`v0_1.rs`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/rust/v0_1.rs),
[`worlds.rs`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/rust/worlds.rs),
and [`mod.rs`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/rust/mod.rs).
Opt-in Rust flags enable additional profiles per project: `--serde` (`serde = true`) adds plain,
format-independent `::serde::Serialize` and `::serde::Deserialize` derives using these Rust
identifiers; `--no-std` (`no-std = true`) emits `#![no_std]`-compatible `::alloc::*` and
`::core::*` paths; `--eq-hash` (`eq-hash = true`) derives `Eq` and `Hash` on float-free types;
and `--try-from` (`try-from = true`) emits `TryFrom` impls in `convert.rs`. See
[JSON projections](json-projections.md) for the exact Serde JSON keys, reading rules, and
`serde_json` edge cases.

## Protobuf

The protobuf emitter uses the numbers in `wire.lock`. Required scalar and string values use
proto3 `optional` so generated glue can distinguish absence from a default value. WIT interfaces
produce request and response messages plus a stable method-ID enum, not protobuf services.

```proto
message TypesItem {
  optional uint64 id = 1;
  optional string name = 2;
  repeated string tags = 3;
  optional string note = 4;
}

message TypesEvent {
  oneof witforge_value {
    Empty started = 1;
    TypesItem item = 2;
    TypesStatus finished = 3;
  }
}

message IngestSubmitRequest {
  TypesItem item = 1;
}

message IngestSubmitResponse {
  TypesEvent result = 1;
}

enum IngestMethod {
  INGEST_METHOD_UNSPECIFIED = 0;
  INGEST_METHOD_SUBMIT = 1;
}
```

Full file: [`v0.proto`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/proto/v0.proto).
Generate language-specific protobuf types from this file with the normal toolchain for that
language.

## JSON Schema

The schema emitter writes one JSON Schema document per contract version (Draft 7 by default, or
Draft 2020-12 via `--draft 2020-12` / `draft = "2020-12"`). Named WIT types live under
`definitions` (or `$defs` in Draft 2020-12); records are closed objects whose properties are all
required. An `option<T>` value accepts `null`, but the containing property is still required.

```json
"types/item": {"description":"One item submitted to the ingester.","type":"object","properties":{"id":{"description":"Stable item id.","type":"integer","minimum":0,"maximum":18446744073709551615},"name":{"description":"Human-readable name.","type":"string"},"tags":{"description":"Free-form tags.","type":"array","items":{"type":"string"}},"note":{"description":"Optional note.","anyOf":[{"type":"string"},{"type":"null"}]}},"required":["id","name","tags","note"],"additionalProperties":false},
"types/status": {"description":"The result state of an item.","type":"string","enum":["accepted","rejected"]},
```

Full file: [`v0_1.schema.json`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/schema/v0_1.schema.json).
Read [JSON Schema and Rust Serde](json-projections.md) before treating both projections as one
JSON wire format.

## Rust/protobuf glue

The glue emitter converts borrowed plain Rust values into owned prost values and validates the
reverse conversion. Copy values are copied directly, owned values are cloned only when the target
must own them, and already-owned identity collections move out of the protobuf value without an
extra allocation.

```rust
type Plain = crate::generated::rust::v0_1::types::Item;
type Proto = crate::generated::proto::v0::TypesItem;

/// Convert `types/item` to its protobuf representation.
pub fn to_proto(value: &Plain) -> Proto {
    Proto {
        id: ::core::option::Option::Some(value.id),
        name: ::core::option::Option::Some(::core::clone::Clone::clone(&value.name)),
        tags: ::core::clone::Clone::clone(&value.tags),
        note: ::core::clone::Clone::clone(&value.note),
    }
}

/// Convert `types/item` from its protobuf representation.
pub fn from_proto(value: Proto) -> ::core::result::Result<Plain, GlueError> {
    let ::core::option::Option::Some(v0) = value.id else {
        return ::core::result::Result::Err(GlueError::MissingField {
            field: "types/item.id",
        });
    };
    let ::core::option::Option::Some(v1) = value.name else {
        return ::core::result::Result::Err(GlueError::MissingField {
            field: "types/item.name",
        });
    };
    ::core::result::Result::Ok(Plain {
        id: v0,
        name: v1,
        tags: value.tags,
        note: value.note,
    })
}
```

Every line in this excerpt is copied from the generated file. Emitted Rust is produced directly by
witforge (without shelling out to `rustfmt`) in `rustfmt`'s default style for both Edition 2021 and
Edition 2024, and passes default Clippy lints and `#![deny(missing_docs)]`. If your crate sets
non-default `rustfmt.toml` options, put `#[rustfmt::skip]` on the module declaration that includes
the generated tree. Full files:
[`v0.rs`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/glue/v0.rs) and
[`mod.rs`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/glue/mod.rs).

Function helpers in the same module map WIT calls to protobuf request and response messages and
expose the locked method ID:

```rust
type Method = crate::generated::proto::v0::IngestMethod;
type Request = crate::generated::proto::v0::IngestSubmitRequest;
type Response = crate::generated::proto::v0::IngestSubmitResponse;
type Params = crate::generated::rust::v0_1::ingest::Item;
type Output = crate::generated::rust::v0_1::ingest::Event;

/// Protobuf method identifier for `ingest#submit`.
pub const METHOD: Method = Method::Submit;

/// Encode request parameters for `ingest#submit`.
pub fn to_request(item: &crate::generated::rust::v0_1::ingest::Item) -> Request {
    let v0 = super::super::super::types::item::to_proto(item);
    Request {
        item: ::core::option::Option::Some(v0),
    }
}

/// Decode request parameters for `ingest#submit`.
pub fn from_request(value: Request) -> ::core::result::Result<Params, GlueError> {
    let ::core::option::Option::Some(v0) = value.item else {
        return ::core::result::Result::Err(GlueError::MissingField {
            field: "ingest#submit.item",
        });
    };
    super::super::super::types::item::from_proto(v0)
}
```

These are conversion helpers, not a transport or RPC service implementation.

## Version adapters

With adjacent contract versions, `witforge rust` also writes `convert.rs`. The evolved example adds
an optional priority and a list of tags; old values receive their WIT-safe defaults.

```rust
/// Convert `types/task` from 0.1.0 to 0.2.0.
///
/// # Errors
///
/// Returns an error when the source value cannot be represented by the target version.
pub fn task(
    value: &super::__witforge_from::types::Task,
) -> ::core::result::Result<super::__witforge_to::types::Task, ConversionError> {
    ::core::result::Result::Ok(super::__witforge_to::types::Task {
        id: value.id,
        title: ::core::clone::Clone::clone(&value.title),
        done: value.done,
        priority: ::core::option::Option::None,
        tags: ::std::vec::Vec::new(),
    })
}
```

Full file:
[`convert.rs`](https://github.com/asamarts/witforge/blob/main/examples/evolved/generated/rust/convert.rs).

To emit deterministic Markdown contract reference pages (`index.md` and per-version `v<major>_<minor>.md`)
for your own contract, run `witforge doc`; to export the deterministic JSON contract IR for tooling,
run `witforge inspect --json`.
