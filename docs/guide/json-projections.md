---
layout: default
title: JSON Schema and Rust Serde
---

# JSON Schema and Rust Serde

witforge's Rust Serde and JSON Schema outputs project the same WIT values, but they do not define one identical JSON wire format.

## Naming

For this WIT:

```wit
record diagnostic {
  start-byte: u32,
}
```

Generated Rust uses `start_byte`, and derived Serde serializes:

```json
{"start_byte": 4}
```

The JSON Schema preserves the WIT property name:

```json
{"start-byte": 4}
```

## Optional values

A WIT `option<T>` means the value may be null. The generated schema still requires the record property. Derived Rust `Option<T>` accepts an omitted field as `None` and serializes `None` as `null`.

## Defaults

WIT does not define application defaults such as "missing severity means warning." Keep those defaults in an explicit application adapter and test them with fixtures.

## Existing JSON protocols

Before adopting generated types for an existing protocol:

1. test full and minimal payloads against generated Serde;
2. decide whether `null` is accepted;
3. compare property names with generated JSON Schema;
4. keep a custom wire schema if naming or presence differs;
5. do not claim the WIT-shaped schema validates a different application JSON profile.
