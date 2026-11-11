# WIT-first plugin framework evaluation

Status: evaluation decision, 2026-11-11. This document records the research owned by
[`W10-001`](../../aplan/tasks/W10-001.md); it does not commit witforge to a plugin runtime or
create a second task list.

## Executive decision

A broad, general Rust WebAssembly plugin framework is not a justified witforge project. Direct
Wasmtime embedding, Extism, `wasm-link`, `wasm_component_layer`, waPC, wRPC, wasmCloud, and
application-specific systems already cover most meanings of that phrase.

A narrower **WIT-first, multi-host plugin kit** could be useful after it is proven in real
applications. Its distinctive promise would be one application-owned WIT contract projected onto:

- plain Rust types and traits for trusted implementations compiled into a host;
- Component Model guest and host bindings for sandboxed WebAssembly components;
- stable protobuf messages for local out-of-process implementations;
- version adapters and one semantic conformance suite across those execution paths.

witforge is a foundation for that design, not the runtime. It should continue to own the contract
model, supported subset, stable protobuf identity, plain Rust and schema projections, and version
conversion. Wasmtime and `wit-bindgen` should own Component Model execution and bindings. A future
runtime or protocol kit, if the validation gates below earn one, should be a separate package that
may consume witforge output but that witforge does not depend on.

The immediate proving ground is alint. arouter is a plausible later second implementation. aplan's
reviewed architecture explicitly rejects runtime rule, format, and evidence plugins, so it is not a
consumer hypothesis for this work.

This evaluation is not a substitute for the independent-consumer requirement in `W10-001`. A
framework, example, or sibling project controlled by the same maintainer does not supply
independent adoption evidence.

## Scope and method

The review asked four questions:

1. Do alint, arouter, or aplan demonstrate a reusable plugin-host requirement?
2. Which existing projects already solve the runtime, ABI, composition, distribution, or policy
   parts of that requirement?
3. Which part, if any, is specific to witforge's WIT-first projections?
4. Should this work precede witforge 1.0 or change the `W10-001` adoption gate?

The review covered witforge's model, emitter, evolution, public-API and release contracts; the
current alint WIT draft and plugin design; arouter's scaffold, research and milestone sequence;
aplan's evolution proposal; primary documentation for the projects in the ecosystem comparison;
and local smoke runs described in [Observed verification](#observed-verification).

The findings distinguish observed behavior from design judgment. Performance, allocation, pooling,
and cross-host parity remain hypotheses until measured with a real consumer payload.

## Current projects

### witforge

witforge currently provides the contract layer:

- WIT loading and a pure, synchronous supported subset;
- plain Rust values and interface traits with no component-runtime dependency;
- protobuf request/response messages, locked method identifiers, and conversion glue;
- Draft 7 JSON Schema for contract values;
- generated conversions between adjacent contract versions;
- world metadata listing imported and exported interfaces.

It deliberately does not provide:

- Component Model guest or host bindings;
- a Wasm engine, linker, store, instance lifecycle, or resource policy;
- process framing, handshake, multiplexing, cancellation, or restart policy;
- plugin discovery, integrity, signing, distribution, or a registry;
- application capability names, grants, ordering, conflict rules, or failure semantics.

`worlds.rs` is inventory metadata, not executable dispatch. The protobuf emitter writes function
request/response messages and a locked method enum, not a protobuf service or process session.
JSON Schema describes values represented in WIT; it becomes a plugin-options schema only when the
application deliberately models those options as WIT values.

There is also no generated bridge between witforge's plain Rust types and the separate Rust types
that `wit-bindgen` or `wasmtime::component::bindgen!` creates. Such a bridge is the most plausible
future witforge-adjacent feature, but nested structural conversion can add allocation and ties the
output to another generator's API. It requires consumer measurements before it becomes a promised
emitter.

### alint

alint is the strongest near-term fit. Its draft `alint:plugin@0.1.0` contract is host-pushed,
synchronous, callback-free, and value-oriented. Its adapter and analyzer worlds fit witforge's
supported subset without resources, handles, streams, futures, or ambient host access.

The current state is still a contract pilot, not a plugin platform:

- no native, process, or Component Model plugin host exists;
- the WIT exports adapter and analyzer APIs but no callable plugin-information or configuration
  API, even though metadata types are present;
- the planned fixer world is not in the draft;
- the generated crate proves parity and serialization, not dynamic loading or isolation.

Those are appropriate draft gaps. A real host should force their resolution before the contract is
frozen or a general runtime abstraction is extracted.

alint's consumer script pins witforge 0.4.1 as of this review. Its committed contract and generated
output also pass `check`, `rust --check`, `proto --check`, and `glue --check` under witforge 0.4.3,
so the release can be adopted without generated-output changes. The pin still needs its own alint
change and CI review before 0.4.3 is an adopted consumer version.

### arouter

arouter is a design and scaffold: its binary does not proxy. Its plugin platform is milestone M7,
after the gateway, policy seam, live compatibility work, and production hardening.

A narrow policy contract can fit witforge later: bounded request metadata in; explicit allow, deny,
header edit, route, or profile decision out. Streaming bodies, per-chunk transformations, async
host callbacks, and hot-path resources do not fit the current subset. Those operations should stay
native, use a host-driven bounded batch contract, or use a separate Component Model interface if
future measurements justify resources and streams.

arouter's current `RESEARCH.md` section 11 predates witforge 0.4.3. It names 0.1.2 as the latest
release and treats dependency packages, JSON Schema, and version conversion as unavailable or
unreleased. That section should be re-reviewed when the M7 contract is designed; changing it now
would make a future design document describe a platform that still does not exist.

### aplan

aplan's architecture proposal recommends a closed policy vocabulary with external composition. It
explicitly rejects runtime rule, format, and evidence plugins because they would weaken exhaustive
audits, stable capability facts, deterministic installation, and the evidence trust boundary.
Machine output plus external policy processes is its intended extension seam.

Aplan might invoke or be invoked by a separate policy tool in the future. That does not justify a
Wasm plugin runtime inside aplan and does not provide a third use case for a shared plugin kit.

### Fit summary

| Project | Contract fit | Current demand evidence | Decision |
|---|---|---|---|
| alint | Strong for pure adapters and analyzers | Draft contract and generated consumer; no host | First runtime pilot |
| arouter | Strong for bounded header/auth/routing policy; weak for streaming transforms | Planned M7 after the functional gateway | Later second implementation |
| aplan | Runtime plugins conflict with its reviewed policy boundary | Explicit rejection | Do not add a plugin host |

This is one concrete contract pilot and one future overlapping hypothesis, not three adopters.

## Existing ecosystem

The comparison uses each project's own documentation and repository as reviewed on 2026-11-11.
Versions name the artifacts inspected locally, not a compatibility promise by witforge.

| Project | Existing capability | Relevance and boundary |
|---|---|---|
| [Wasmtime Component API](https://docs.wasmtime.dev/api/wasmtime/component/index.html) and [plugin example](https://docs.wasmtime.dev/wasip2-plugins.html) | Typed Component Model loading, WIT host bindings, linkers, stores, fuel, epochs and resource limits | The boring baseline for alint. It is runtime plumbing rather than an application plugin lifecycle or cross-transport framework. |
| [Extism 1.41](https://extism.org/docs/overview/) | Cross-language host SDKs and PDKs, byte-buffer calls, manifests, cancellation, pooling, memory limits, hashes, and allowed hosts/paths | Strong existing general plugin framework. Its documented public ABI is bytes in/out with caller-chosen serialization rather than arbitrary WIT Component Model contracts. |
| [`wasm-link` 0.4.1](https://docs.rs/wasm-link/latest/wasm_link/) | Wasmtime-based Component Model plugin DAGs, cardinality, dynamic values, resources, fuel, epochs, memory limits, and async dispatch | The closest direct WIT plugin-runtime comparison. It requires Rust 1.94 while witforge and alint promise 1.88 and arouter currently declares 1.85. Its README also warns that the current async API is temporary. Evaluate it before duplicating it. |
| [`wasm_component_layer` 0.1.18](https://docs.rs/wasm_component_layer/latest/wasm_component_layer/) | Runtime-agnostic Component Model loading over multiple core-Wasm engines | Useful low-level alternative. Its documentation lists host-binding macros, subtyping, and more comprehensive tests as unfinished. |
| [waPC](https://github.com/wapc/wapc-rs) | Engine-independent host/guest RPC, callbacks, MessagePack codecs, and pools | A separate established ABI, not WIT Component Model canonical typing. |
| [wRPC](https://github.com/bytecodealliance/wrpc) | WIT-native RPC over TCP, Unix sockets, QUIC, HTTP/2 and other transports, with Rust and Go bindings | Relevant if a plugin becomes remote or distributed. It is not a local sandbox lifecycle and uses Component Model value encoding rather than witforge's stable protobuf projection. |
| [wasmCloud](https://wasmcloud.com/docs/overview/hosts/plugins/) | WIT workloads, native and component host plugins, capability grants, pooling, OCI distribution, and cross-workload dispatch | Valuable prior art but a full application/distributed runtime, larger than alint's or arouter's local plugin requirement. |
| [Spin](https://spinframework.dev/v4/extending-and-embedding) | Embeddable WIT-oriented application runtime and custom triggers | An application runtime, not a transport-neutral plugin SDK. Its custom trigger APIs are documented as experimental. |
| [dprint](https://dprint.dev/plugins/), [Zed](https://zed.dev/blog/zed-decoded-extensions), and [Typst](https://typst.app/docs/reference/foundations/plugin/) | Production Wasm extension ecosystems with sandboxed artifacts and application SDKs | Evidence that domain-specific plugin systems work. Each owns application-specific configuration, purity, lifecycle, capabilities, ABI, and distribution rather than relying on a universal framework. |
| [Proxy-Wasm](https://github.com/proxy-wasm/spec) and [http-wasm](https://http-wasm.io/http-handler-abi/) | Portable proxy and HTTP middleware ABIs | Potential arouter compatibility targets; not general contracts for alint or aplan. |
| [OPA Wasm](https://www.openpolicyagent.org/docs/wasm) | A policy language compiled to a versioned Wasm ABI | Prefer it when the product need is Rego policy evaluation, rather than allowing arbitrary third-party Rust logic. |

Component packaging and distribution also have a standards-oriented implementation:
[`wkg`/`wasm-pkg-tools`](https://github.com/bytecodealliance/wasm-pkg-tools) publishes and fetches
WIT packages and components through OCI. A future kit should integrate with or remain agnostic to
that tooling instead of inventing another registry. `wkg.lock` pins package artifacts and
dependencies; witforge's `wire.lock` separately preserves protobuf member identity.

The reviewed projects do not show a mature general tool with witforge's exact combination of WIT
to runtime-independent plain Rust, stable protobuf, JSON Schema, and version adapters. That is a
bounded differentiation claim about this review, not a claim that no unreviewed implementation
exists.

## Candidate architecture

The reusable design, if earned, is a thin kit around application-owned contracts rather than one
framework that owns application semantics.

```text
                             application-owned WIT
                              /        |        \
                             /         |         \
                    witforge     wit-bindgen    Wasmtime bindgen
                   /   |   \          |              |
          plain Rust  proto  schema   guest         typed host
             |          |               |              |
       native host   process host    component ---- Wasmtime host
             \          |               /              /
              +---------+--------------+--------------+
                                |
                    application adapter and policy
```

### Application-owned surface

The application must own:

- interface names, values, methods, and semantic invariants;
- plugin metadata and configuration shape;
- capability vocabulary and the operator's grant policy;
- result validation and whether a failure denies, fails analysis, retries, or degrades;
- ordering, conflicts, tenancy, state, cache identity, and redaction;
- trusted native registration and user-facing diagnostics.

A generic framework cannot safely infer those decisions from WIT shapes.

### Potential shared surface

A separate reusable crate set could eventually own only mechanics that repeat:

1. **Component loading:** raw-component validation and local compilation, expected-world checks,
   import allowlisting, engine/store construction, and instance lifecycle.
2. **Resource controls:** guest memory/table/instance limits, fuel, epoch deadlines, input/output
   byte and item limits, and bounded logs.
3. **Process mechanics:** length-prefixed frames, protocol and exact-contract negotiation, request
   identifiers, multiplexing, cancellation, shutdown, bounded stderr, and crash/timeout
   classification.
4. **Conformance:** run the same domain cases against native, component, and process
   implementations, including version mismatch, traps, malformed output, limits, and cancellation.
5. **Optional generated bridge:** convert Component Model binding values to and from witforge plain
   values, but only after allocation and API-coupling measurements justify it.

Native discovery, Wasm distribution, signatures, and a public registry are separate concerns and
should not be bundled into the first reusable surface.

## Why the runtime stays outside witforge

Keeping a runtime dependency out of witforge preserves concrete properties:

- generated plain Rust and glue have no runtime dependency;
- witforge's MSRV is 1.88 and is tested separately from fast-moving Wasm runtimes;
- consumers remain free to choose their async runtime, Wasm engine, lifecycle, and logging stack;
- pure value contracts are not forced to accept resources, streams, futures, or ambient WASI;
- witforge's 1.0 public API and generated-output promise do not become coupled to Wasmtime's release
  cadence or generated binding API.

`W11-001` is a different post-1.0 question: third-party **witforge emitter** extensibility. It does
not represent an application plugin runtime and should not be pulled before 1.0 to cover this
evaluation.

## Security and correctness requirements

A future host must make the following boundaries explicit rather than advertise “Wasm is safe” as
an unconditional property.

### Authority

WebAssembly code can interact with the outside world only through imports linked by the host, but
those imports are authority. A plugin manifest can request capabilities; only operator or
application configuration can grant them. The effective imports must be the intersection of what
the artifact requests and what the operator grants.

A pure contract does not require a broad WASI bundle. The Component Model and WASI are separate;
link only the named application imports needed by the selected contract.

### Resource limits

Fuel deterministically bounds guest instruction execution but is not a wall-clock timeout. Epoch
interruption supplies an external timing boundary but is nondeterministic. Neither automatically
limits a blocking or expensive host function.

Wasmtime's resource limiter covers guest instances, linear memories, and tables. It does not count
every runtime allocation, host allocation, lifted output, returned list, diagnostic, or callback.
Hosts must separately cap input bytes, output bytes, item counts, logs, and host-call work.

### Artifacts and provenance

Ordinary components should be validated and compiled locally. Wasmtime documents precompiled
module/component deserialization as unsafe for untrusted bytes because those artifacts contain
native code that cannot be fully validated. A precompiled cache must be created and controlled by
the host, integrity-protected, and keyed by runtime and engine configuration.

A digest proves that bytes match a known digest only when the expected digest arrived through a
trusted channel. A signature authenticates a publisher, not plugin safety or semantic correctness.

### Results and transport trust

All plugin results remain untrusted. Hosts must reject invalid paths, edits, headers, decisions,
counts, identifiers, control characters, oversized diagnostics, and values that violate domain
invariants.

Compiled-in native Rust is fully trusted. A process plugin has crash isolation, but without an OS
sandbox it normally retains the user's filesystem, network, and process privileges. Those paths
cannot honestly claim the Wasm host's security model even when they implement the same functional
contract.

## Validation gates before extraction

These are design gates, not a second task plan. Aplan remains authoritative for scheduled work.

### 1. Real alint component path

Use alint's actual draft rather than another calculator fixture:

- adopt the current published witforge release;
- finish the minimum production contract/API seam;
- implement one analyzer component with direct Wasmtime;
- expose no ambient WASI imports;
- invoke it through the same internal analyzer boundary as a native implementation;
- classify wrong world/version, malformed component, trap, timeout, memory limit, and invalid
  output distinctly and fail loudly.

### 2. Runtime comparison

Run the same analyzer through direct Wasmtime and `wasm-link`. Compare Extism only as an explicit
alternative ABI rather than pretending it consumes arbitrary WIT components.

Measure cold compile/load/instantiate, warm p50/p95/p99 latency, throughput at representative row
counts, nested conversion allocations, peak resident memory, instance reuse versus fresh
instances, concurrency/state reset, binary-size impact, and MSRV impact. Exercise small metadata,
64 KiB input, and 1 MiB input instead of quoting generic Wasm multipliers.

### 3. Process parity

After the component path works, add a process implementation with protobuf framing, exact contract
version in the handshake, request identifiers, bounds, cancellation, EOF behavior, and malformed
frame handling. Run the same semantic conformance cases against native, component, and process
implementations.

“One Rust implementation core with three wrappers” is a possible SDK goal. “One artifact runs in
all three hosts” is not: native libraries, executables, and components are different artifacts and
have different trust boundaries.

### 4. Extraction

Extract a reusable package only when a second real application repeats the same lifecycle or
protocol mechanics, the shared surface excludes application policy, and direct Wasmtime or an
existing project cannot provide the abstraction cleanly. arouter can test this condition when its
policy seam and M7 are real. A same-maintainer extraction still does not count as independent
witforge adoption.

## Implications for witforge 1.0

`W10-001` requires both alint and at least one independent consumer to use the 0.4 line
successfully, plus a clean release-criteria and conformance review. The prerequisite implementation
and release tasks are done; the independent-consumer observation is now the unresolved gate.

The current projects do not satisfy it:

- alint is the named first consumer and has not yet moved its pin from 0.4.1;
- arouter is a scaffold, not an operating consumer, and shares the maintainer;
- aplan rejects the runtime-plugin direction;
- a new sample, framework, or sibling project controlled by the same maintainer is not independent.

The release sequence therefore remains:

- complete the reachable `W10-001` work, including moving alint to 0.4.3, correcting stale adopter
  documentation, and running the candidate compatibility gates;
- keep `W10-001` open until genuine independent 0.4 adoption is observed;
- publish 0.4.x corrections if needed while the gate remains open;
- release 1.0 through `W10-002` only after the criterion is met, unless the project explicitly
  changes its release policy and records that risk rather than silently redefining “independent.”

Building a plugin framework is not a W10 prerequisite and would add unstable runtime scope before
the compiler API is frozen. The runtime experiment belongs with alint; a cross-project extraction
evaluation belongs after real duplicated implementation evidence exists.

## Observed verification

The following commands or scenarios were executed during the review:

- `aplan show W10-001` and `aplan ready`: every declared blocker is done and `W10-001` is ready;
  `W10-002` waits on it.
- alint's pinned 0.4.1 `ci/scripts/witforge-consumer.sh`: passed.
- witforge 0.4.3 against alint's draft: `check`, `rust --check`, `proto --check`, and
  `glue --check` all passed without writing output.
- a disposable Wasmtime 46.0.3 host loaded a Component Model value from WAT, performed a typed
  `u32 + u32` component call, and observed `42`. The scratch project was removed afterward.
- local crate metadata confirmed `wasm-link` 0.4.1 requires Rust 1.94, while witforge and alint
  declare 1.88 and arouter declares 1.85.

The Wasmtime smoke proves only that direct typed embedding works on the development host. It is not
a benchmark and does not prove the alint lifecycle, conversion cost, limits, or semantic parity.
