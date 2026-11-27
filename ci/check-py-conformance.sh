#!/usr/bin/env bash
# End-to-end Python checked protobuf glue conformance harness (`W13-005`).
#
# Generates the `W11-034` conformance bundle, emits plain Python (`witforge py`),
# compiles `.proto` files with `protoc --python_out`, emits checked Python glue
# (`witforge py-glue`), bytecode-compiles every emitted module, verifies runtime
# `typing.get_type_hints()` resolution, and executes every binary, proto3-JSON,
# and encode-bounds conformance vector plus a multi-major disambiguated `deps/`,
# relative-root, keyword-escaping, and bool/int edge-case smoke contract.
#
# Usage: [WITFORGE=path/to/witforge] [PROTOC=path/to/protoc] [PYTHON=path/to/python3] ci/check-py-conformance.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HARNESS_DIR="${ROOT}/ci/py-conformance"
PROTOC_BIN="${PROTOC:-protoc}"
PYTHON_BIN="${PYTHON:-python3}"

if ! command -v "${PROTOC_BIN}" >/dev/null 2>&1; then
    echo "error: protoc not found (set PROTOC or install protoc)" >&2
    exit 1
fi
if ! command -v "${PYTHON_BIN}" >/dev/null 2>&1; then
    echo "error: python3 not found (set PYTHON or install python3 3.11+)" >&2
    exit 1
fi

"${PYTHON_BIN}" -c 'import sys; assert sys.version_info >= (3, 11), f"need Python 3.11+, got {sys.version}"'
"${PYTHON_BIN}" -c 'import google.protobuf'

if [[ -n "${WITFORGE:-}" ]]; then
    RUN=("${WITFORGE}")
else
    cargo build --manifest-path "${ROOT}/Cargo.toml" --locked -p witforge-cli
    RUN=("${ROOT}/target/debug/witforge")
fi

scratch="${TMPDIR:-${HOME}/.tmp}"
mkdir -p "${scratch}"
WORK="$(mktemp -d "${scratch%/}/witforge-py-conformance.XXXXXX")"
trap 'rm -rf "${WORK}"' EXIT

# ---------------------------------------------------------------------------
# 1. Canonical W11-034 conformance bundle (absolute module roots: plain / proto)
# ---------------------------------------------------------------------------
CONF="${WORK}/conformance"
mkdir -p "${CONF}/src/proto"

"${RUN[@]}" conformance --out "${CONF}/bundle"
"${RUN[@]}" py "${CONF}/bundle/wit" --out "${CONF}/src/plain"
"${RUN[@]}" py "${CONF}/bundle/wit" --out "${CONF}/src/plain" --check
"${RUN[@]}" py-glue "${CONF}/bundle/wit" --out "${CONF}/src/glue" \
    --plain-root plain --proto-root proto
"${RUN[@]}" py-glue "${CONF}/bundle/wit" --out "${CONF}/src/glue" \
    --plain-root plain --proto-root proto --check

"${PROTOC_BIN}" \
    -I "${CONF}/bundle/proto" \
    --python_out="${CONF}/src/proto" \
    "${CONF}/bundle/proto/v0.proto" \
    "${CONF}/bundle/proto/witforge.conformance_dep.v0.proto"

"${PYTHON_BIN}" -m compileall -q -f "${CONF}/src"

PYTHONPATH="${CONF}/src:${CONF}/src/proto" \
    "${PYTHON_BIN}" "${HARNESS_DIR}/run.py" "${CONF}/bundle"

# ---------------------------------------------------------------------------
# 2. Multi-major + disambiguated deps + relative roots + keyword/mro/builtin edge smoke
# ---------------------------------------------------------------------------
EDGE="${WORK}/edge"
mkdir -p "${EDGE}/wit/v0.1/deps/shared" "${EDGE}/wit/v0.1/deps/kw" \
    "${EDGE}/wit/v1.0/deps/shared" "${EDGE}/wit/v1.0/deps/kw" \
    "${EDGE}/src/pkg/proto"

cat >"${EDGE}/src/pkg/__init__.py" <<'EOF'
EOF

cat >"${EDGE}/wit/v0.1/deps/shared/dep.wit" <<'EOF'
package acme:shared@0.1.0;

interface core {
    record tag {
        id: u8,
        label: string,
    }
    type maybe-tags = option<list<tag>>;
    type byte-alias = u8;
    type foreign-bytes = list<byte-alias>;
}
EOF

cat >"${EDGE}/wit/v0.1/deps/kw/kw.wit" <<'EOF'
package %from:cls@0.1.0;

interface items {
    record entry {
        code: u16,
    }
}
EOF

cat >"${EDGE}/wit/v0.1/contract.wit" <<'EOF'
package acme:edge@0.1.0;

interface empty-iface {}

interface types {
    use acme:shared/core@0.1.0.{tag, maybe-tags, foreign-bytes};
    use %from:cls/items@0.1.0.{entry};

    /// Nominal type named `value` verifies zero parameter shadowing in `to_proto` / `from_proto`.
    record value {
        n: u8,
    }

    type value-alias = value;
    type local-bytes = foreign-bytes;

    record item {
        %from: u16,
        class: string,
        self: bool,
        mro: u8,
        tag: tag,
        maybe-tags: maybe-tags,
        kw-entry: entry,
        v: value,
        va: value-alias,
        raw: local-bytes,
    }

    flags tricky-flags {
        %from,
        class,
        mro,
    }

    type opt-list = option<list<u8>>;
    type opt-nested = option<option<u16>>;
    type opt-of-named = option<opt-list>;
    type opt-map = option<map<u8, string>>;

    variant tricky {
        %from(u8),
        class,
    }

    record holder {
        a: opt-list,
        b: opt-nested,
        c: tricky,
        d: opt-map,
        e: tricky-flags,
        g: opt-of-named,
        h: list<opt-list>,
        hn: list<opt-nested>,
        m: map<u8, list<u8, 4>>,
    }
}

interface api {
    use types.{item, holder, value};

    record request {
        id: u32,
        v: value,
    }

    /// Parameter names colliding with builtins (`len`, `isinstance`, `getattr`, `setattr`),
    /// keywords (`from`), and same-interface type names (`request`).
    send: func(request: request, len: list<u8, 2>, isinstance: option<option<u8>>, getattr: u8, setattr: u8, %from: u16) -> request;
    mro: func(%from: item, h: holder, item: string) -> option<item>;
    ping: func();
}
EOF

cat >"${EDGE}/wit/v1.0/deps/shared/dep.wit" <<'EOF'
package acme:shared@0.1.0;

interface core {
    record tag {
        id: u16,
        label: string,
        extra: bool,
    }
    type maybe-tags = option<list<tag>>;
    type byte-alias = u8;
    type foreign-bytes = list<byte-alias>;
}
EOF

cat >"${EDGE}/wit/v1.0/deps/kw/kw.wit" <<'EOF'
package %from:cls@0.1.0;

interface items {
    record entry {
        code: u16,
    }
}
EOF

cat >"${EDGE}/wit/v1.0/contract.wit" <<'EOF'
package acme:edge@1.0.0;

interface types {
    use acme:shared/core@0.1.0.{tag, maybe-tags};
    use %from:cls/items@0.1.0.{entry};

    record item {
        %from: u32,
        class: string,
        tag: tag,
        maybe-tags: maybe-tags,
        kw-entry: entry,
    }
}
EOF

"${RUN[@]}" lock "${EDGE}/wit/v0.1" --first >/dev/null
"${RUN[@]}" lock "${EDGE}/wit/v1.0" --first >/dev/null

"${RUN[@]}" proto "${EDGE}/wit" --out "${EDGE}/proto-src"
"${RUN[@]}" py "${EDGE}/wit" --out "${EDGE}/src/pkg/plain"
"${RUN[@]}" py-glue "${EDGE}/wit" --out "${EDGE}/src/pkg/glue" \
    --plain-root ..plain --proto-root ..proto
"${RUN[@]}" py-glue "${EDGE}/wit" --out "${EDGE}/src/pkg/glue" \
    --plain-root ..plain --proto-root ..proto --check

shopt -s nullglob
proto_files=("${EDGE}/proto-src/"*.proto)
shopt -u nullglob
if [[ ${#proto_files[@]} -eq 0 ]]; then
    echo "error: no edge proto files generated" >&2
    exit 1
fi

"${PROTOC_BIN}" \
    -I "${EDGE}/proto-src" \
    --python_out="${EDGE}/src/pkg/proto" \
    "${proto_files[@]}"

"${PYTHON_BIN}" -m compileall -q -f "${EDGE}/src"

cat >"${EDGE}/smoke.py" <<'EOF'
from __future__ import annotations

import typing

from pkg import glue as g
from pkg.plain import v0_1 as p0, v1_0 as p1
from pkg.plain.witforge_deps.from_cls import items as kw_items
from pkg.plain.witforge_deps.v0.acme_shared import core as dep_v0
from pkg.plain.witforge_deps.v1.acme_shared import core as dep_v1
from pkg.plain.witforge_types import NONE, Some

assert typing.get_type_hints(g.v0.types.item.to_proto)
assert typing.get_type_hints(g.v0.types.holder.from_proto)
assert typing.get_type_hints(g.v0.api.functions.send.to_request)
assert typing.get_type_hints(g.v0.api.functions.mro_.to_request)

v0_item = p0.types.Item(
    from_=42,
    class_="hello",
    self_=True,
    mro_=7,
    tag=dep_v0.Tag(id=7, label="v0"),
    maybe_tags=(dep_v0.Tag(id=1, label="a"),),
    kw_entry=kw_items.Entry(code=55),
    v=p0.types.Value(n=5),
    va=p0.types.Value(n=6),
    raw=b"\x09\x08\x07",
)
v0_bytes = g.v0.types.item.to_proto(v0_item).SerializeToString()
v0_msg = g.v0.types.item.proto_type()
v0_msg.ParseFromString(v0_bytes)
assert g.v0.types.item.from_proto(v0_msg) == v0_item

v1_item = p1.types.Item(
    from_=99999,
    class_="world",
    tag=dep_v1.Tag(id=500, label="v1", extra=True),
    maybe_tags=None,
    kw_entry=kw_items.Entry(code=88),
)
v1_bytes = g.v1.types.item.to_proto(v1_item).SerializeToString()
v1_msg = g.v1.types.item.proto_type()
v1_msg.ParseFromString(v1_bytes)
assert g.v1.types.item.from_proto(v1_msg) == v1_item

holder = p0.types.Holder(
    a=b"\x01\x02\x03",
    b=Some(None),
    c=p0.types.TrickyFrom(value=9),
    d={7: "seven"},
    e=p0.types.TrickyFlags(from_=True, class_=False, mro_=True),
    g=Some(b"\x04\x05"),
    h=(None, b"\x06"),
    hn=(NONE, Some(None), Some(77)),
    m={1: b"\x0a\x14\x1e\x28"},
)
h_bytes = g.v0.types.holder.to_proto(holder).SerializeToString()
h_msg = g.v0.types.holder.proto_type()
h_msg.ParseFromString(h_bytes)
assert g.v0.types.holder.from_proto(h_msg) == holder

# Builtin-shadowing parameter names + keyword parameter + same-interface `request` type
send_arg = p0.api.Request(id=77, v=p0.types.Value(n=3))
send_req = g.v0.api.functions.send.to_request(send_arg, b"\x01\x02", Some(9), 1, 2, 300)
send_req_msg = g.v0.api.functions.send.request_type()
send_req_msg.ParseFromString(send_req.SerializeToString())
assert g.v0.api.functions.send.from_request(send_req_msg) == (
    send_arg,
    b"\x01\x02",
    Some(9),
    1,
    2,
    300,
)

# `mro` method escaped as `mro_` on `functions`
mro_req = g.v0.api.functions.mro_.to_request(v0_item, holder, "label")
mro_req_msg = g.v0.api.functions.mro_.request_type()
mro_req_msg.ParseFromString(mro_req.SerializeToString())
assert g.v0.api.functions.mro_.from_request(mro_req_msg) == (v0_item, holder, "label")

mro_resp = g.v0.api.functions.mro_.to_response(v0_item)
mro_resp_msg = g.v0.api.functions.mro_.response_type()
mro_resp_msg.ParseFromString(mro_resp.SerializeToString())
assert g.v0.api.functions.mro_.from_response(mro_resp_msg) == v0_item

ping_req = g.v0.api.functions.ping.to_request()
assert g.v0.api.functions.ping.from_request(ping_req) is None
ping_resp = g.v0.api.functions.ping.to_response()
assert g.v0.api.functions.ping.from_response(ping_resp) is None

# `bool` is a subclass of `int` in Python and MUST be rejected for integer fields!
try:
    g.v0.types.value.to_proto(p0.types.Value(n=True))  # type: ignore[arg-type]
    raise AssertionError("expected bool to be rejected for u8")
except g.WitforgeGlueError as err:
    assert err.kind == "IntegerOutOfRange"
    assert err.target == "types/value.n"

# Map key validation runs BEFORE value validation on encode AND decode!
bad_holder = p0.types.Holder(
    a=None,
    b=NONE,
    c=p0.types.TrickyClass(),
    d=None,
    e=p0.types.TrickyFlags(),
    g=NONE,
    h=(),
    hn=(),
    m={300: b"\x01\x02"},
)
try:
    g.v0.types.holder.to_proto(bad_holder)
    raise AssertionError("expected map key error before value length error on encode")
except g.WitforgeGlueError as err:
    assert err.kind == "IntegerOutOfRange"
    assert err.target == "types/holder.m"

bad_proto = g.v0.types.holder.to_proto(holder)
bad_proto.m[300] = b"\x01\x02"
try:
    g.v0.types.holder.from_proto(bad_proto)
    raise AssertionError("expected map key error before value length error on decode")
except g.WitforgeGlueError as err:
    assert err.kind == "IntegerOutOfRange"
    assert err.target == "types/holder.m"

print("Python multi-major, relative-root, keyword-dep, and bool-guard edge-case smoke passed")
EOF

PYTHONPATH="${EDGE}/src:${EDGE}/src/pkg/proto" \
    "${PYTHON_BIN}" "${EDGE}/smoke.py"
