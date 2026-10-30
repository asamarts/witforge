# alint plugin contract fixture

This fixture is a snapshot of alint's committed `wit/alint-plugin/0.1.0/plugin.wit`
draft and its `wire.lock`, used by witforge's end-to-end consumer test.

## Source

- alint `wit/alint-plugin/0.1.0/plugin.wit` and `wire.lock`.
- The source package is `alint:plugin@0.1.0`; the WIT declares the adapter and
  analyzer worlds and the shared contract types.

## Consumption note

alint stores the draft under `wit/alint-plugin/0.1.0/`, while witforge's generation
commands require a root containing `v<major>.<minor>` directories. The end-to-end
test stages the snapshot as `v0.1`, which keeps alint's committed source layout and
exercises witforge's generation contract. The alint adoption branch should either
adopt `v0.1` or document the staging step used by its CI.

No contract exception is required: the committed draft passes the witforge subset
and drives plain Rust, protobuf, and glue generation plus binary/proto3-JSON
round-trips.
