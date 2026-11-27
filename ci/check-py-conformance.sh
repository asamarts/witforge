#!/usr/bin/env bash
# End-to-end Python checked protobuf glue conformance harness (`W13-005`).
#
# Generates the `W11-034` conformance bundle, emits plain Python (`witforge py`),
# compiles `.proto` files with `protoc --python_out`, emits checked Python glue
# (`witforge py-glue`), bytecode-compiles every emitted module, verifies runtime
# `typing.get_type_hints()` resolution, and executes every binary, proto3-JSON,
# and encode-bounds conformance vector plus a multi-minor, multi-major,
# canonical-owner re-export, disambiguated `deps/`, relative-root, keyword-escaping,
# builtin-shadowing, and strict type-guard edge-case smoke contract.
#
# Usage: [WITFORGE=path/to/witforge] [PROTOC=path/to/protoc] [PYTHON=path/to/python3] ci/check-py-conformance.sh
set -euo pipefail

# Never allow `PYTHONOPTIMIZE` (`-O`) to strip conformance assertions silently.
unset PYTHONOPTIMIZE

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

PYTHONPATH="${CONF}/src:${CONF}/src/proto${PYTHONPATH:+:${PYTHONPATH}}" \
    "${PYTHON_BIN}" "${HARNESS_DIR}/run.py" "${CONF}/bundle"

# ---------------------------------------------------------------------------
# 2. Multi-minor + multi-major + canonical-owner re-exports + disambiguated deps
#    + relative roots + keyword/mro/builtin-shadowing + proto3-JSON edge smoke
# ---------------------------------------------------------------------------
EDGE="${WORK}/edge"
mkdir -p \
    "${EDGE}/wit/v0.1/deps/shared" "${EDGE}/wit/v0.1/deps/kw" \
    "${EDGE}/wit/v0.2/deps/shared" "${EDGE}/wit/v0.2/deps/kw" \
    "${EDGE}/wit/v1.0/deps/shared" "${EDGE}/wit/v1.0/deps/kw" \
    "${EDGE}/wit/v2.0/deps/shared" "${EDGE}/wit/v2.0/deps/kw" \
    "${EDGE}/src/pkg/proto"

cat >"${EDGE}/src/pkg/__init__.py" <<'EOF'
EOF

for ver_dir in v0.1 v0.2 v1.0; do
    cat >"${EDGE}/wit/${ver_dir}/deps/shared/dep.wit" <<'EOF'
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
done

cat >"${EDGE}/wit/v2.0/deps/shared/dep.wit" <<'EOF'
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

for ver_dir in v0.1 v0.2 v1.0 v2.0; do
    cat >"${EDGE}/wit/${ver_dir}/deps/kw/kw.wit" <<'EOF'
package %from:cls@0.1.0;

interface items {
    record entry {
        code: u16,
    }
}
EOF
done

cat >"${EDGE}/wit/v0.1/contract.wit" <<'EOF'
package acme:edge@0.1.0;

interface empty-iface {}

interface types {
    use acme:shared/core@0.1.0.{tag, maybe-tags, foreign-bytes};
    use %from:cls/items@0.1.0.{entry};

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
}
EOF

cat >"${EDGE}/wit/v0.2/contract.wit" <<'EOF'
package acme:edge@0.2.0;

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
        added-in-v0-2: option<u16>,
    }

    flags tricky-flags {
        %from,
        class,
        mro,
    }

    type opt-bytes = option<list<u8>>;
    type opt-u16-list = option<list<u16>>;
    type opt-fixed-u16s = option<list<u16, 2>>;
    type opt-nested = option<option<u16>>;
    type opt-triple = option<option<option<u8>>>;
    type opt-of-named = option<opt-bytes>;
    type opt-map = option<map<u8, string>>;
    type opt-msg-map = option<map<string, value>>;

    variant tricky {
        %from(u8),
        class,
    }

    record holder {
        a: opt-bytes,
        al: opt-u16-list,
        af: opt-fixed-u16s,
        b: opt-nested,
        bt: opt-triple,
        c: tricky,
        d: opt-map,
        dm: opt-msg-map,
        e: tricky-flags,
        g: opt-of-named,
        h: list<opt-bytes>,
        hn: list<opt-nested>,
        m: map<u8, list<u8, 4>>,
    }
}

/// Interface intentionally declaring Python builtin names as WIT type names to verify
/// module-level export shadowing protection across runtime calls and type annotations.
interface shadowed {
    record int { x: u32 }
    record %bool { b: bool }
    record float { f: f64 }
    record str { s: string }
    record bytes { raw: list<u8> }
    record %tuple { xs: list<u32> }
    record %list { xs: list<u16, 2> }
    record dict { m: map<string, u8> }
    record len { n: u32 }
    record isinstance { ok: bool }
    record getattr { n: u8 }
    record setattr { n: u8 }
    record repr { s: string }

    enum shade {
        light,
        dark,
    }

    flags bits {
        %from,
        active,
    }

    variant choice {
        num(u16),
        none-case,
    }

    record bundle {
        xs: list<u16, 2>,
        bs: list<u8, 2>,
        m: map<u8, shade>,
        f: f64,
        s: string,
        c: shade,
        bits: bits,
        ch: choice,
        opt: option<option<u8>>,
        res: result<u8, string>,
    }

    echo: func(%list: list<u16, 2>, dict: map<string, u8>, int: option<u16>, %tuple: %tuple) -> bundle;
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

cat >"${EDGE}/wit/v1.0/contract.wit" <<'EOF'
package acme:edge@1.0.0;

interface types {
    use acme:shared/core@0.1.0.{tag, maybe-tags};
    use %from:cls/items@0.1.0.{entry};

    record item {
        %from: u16,
        class: string,
        tag: tag,
        maybe-tags: maybe-tags,
        kw-entry: entry,
    }
}
EOF

cat >"${EDGE}/wit/v2.0/contract.wit" <<'EOF'
package acme:edge@2.0.0;

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
cp "${EDGE}/wit/v0.1/wire.lock" "${EDGE}/wit/v0.2/wire.lock"
"${RUN[@]}" lock "${EDGE}/wit/v0.2" --against "${EDGE}/wit/v0.1/wire.lock" >/dev/null
"${RUN[@]}" lock "${EDGE}/wit/v1.0" --first >/dev/null
"${RUN[@]}" lock "${EDGE}/wit/v2.0" --first >/dev/null

"${RUN[@]}" proto "${EDGE}/wit" --out "${EDGE}/proto-src"
"${RUN[@]}" py "${EDGE}/wit" --out "${EDGE}/src/pkg/plain"
"${RUN[@]}" py "${EDGE}/wit" --out "${EDGE}/src/pkg/plain" --check
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

import inspect
import typing

if not __debug__:
    raise RuntimeError("smoke test must not run under -O / PYTHONOPTIMIZE")

from google.protobuf import json_format

from pkg import glue as g
from pkg.glue import error as g_error, helpers as g_helpers
from pkg.plain import v0_2 as p0, v1_0 as p1, v2_0 as p2
from pkg.plain.witforge_deps.from_cls import items as kw_items
from pkg.plain.witforge_deps.v0.acme_shared import core as dep_v0
from pkg.plain.witforge_deps.v1.acme_shared import core as dep_v1
from pkg.plain.witforge_deps.v2.acme_shared import core as dep_v2
from pkg.plain.witforge_types import NONE, Err, Ok, Some


def check_module_hints(mod: object) -> int:
    count = 0
    exported = getattr(mod, "__all__")
    assert isinstance(exported, tuple)
    for name in exported:
        obj = getattr(mod, name)
        if inspect.isclass(obj):
            typing.get_type_hints(obj)
            count += 1
            for attr_name, raw_attr in obj.__dict__.items():
                if attr_name.startswith("__") and attr_name != "__init__":
                    continue
                if isinstance(raw_attr, staticmethod):
                    typing.get_type_hints(raw_attr.__func__)
                    count += 1
                elif inspect.isfunction(raw_attr):
                    typing.get_type_hints(raw_attr)
                    count += 1
                elif inspect.isclass(raw_attr):
                    typing.get_type_hints(raw_attr)
                    count += 1
                    for fn_attr in raw_attr.__dict__.values():
                        if isinstance(fn_attr, staticmethod):
                            typing.get_type_hints(fn_attr.__func__)
                            count += 1
        elif inspect.isfunction(obj):
            typing.get_type_hints(obj)
            count += 1
    return count


# Exhaustive type-hint check across every emitted edge module (including builtin-shadowed & keyword deps).
hint_count = 0
for mod in (
    g_error,
    g_helpers,
    g.v0.empty_iface,
    g.v0.types,
    g.v0.shadowed,
    g.v0.api,
    g.v1.types,
    g.v2.types,
    g.witforge_deps.from_cls.items,
    g.witforge_deps.v0.acme_shared.core,
    g.witforge_deps.v1.acme_shared.core,
    g.witforge_deps.v2.acme_shared.core,
):
    hint_count += check_module_hints(mod)
assert hint_count >= 120, f"expected >=120 edge hint targets, got {hint_count}"

# Empty interface exports empty `__all__`.
assert g.v0.empty_iface.__all__ == ()

# Canonical-owner re-export identity: v0 and v1 share identical `acme:shared`, while v2 diverges.
assert dep_v1.Tag is dep_v0.Tag
assert g.witforge_deps.v1.acme_shared.core.tag is g.witforge_deps.v0.acme_shared.core.tag
assert dep_v2.Tag is not dep_v0.Tag
assert g.witforge_deps.v2.acme_shared.core.tag is not g.witforge_deps.v0.acme_shared.core.tag

# v0 targets highest minor (v0.2 with `added_in_v0_2`), plus proto3-JSON round-trip on keyword fields.
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
    added_in_v0_2=321,
)
v0_proto = g.v0.types.item.to_proto(v0_item)
v0_msg = g.v0.types.item.proto_type()
v0_msg.ParseFromString(v0_proto.SerializeToString())
assert g.v0.types.item.from_proto(v0_msg) == v0_item

v0_json = json_format.MessageToJson(v0_proto)
v0_msg_json = json_format.Parse(v0_json, g.v0.types.item.proto_type())
assert g.v0.types.item.from_proto(v0_msg_json) == v0_item

# v1 (canonical-owner shared dep) & v2 (disambiguated divergent dep)
v1_item = p1.types.Item(
    from_=123,
    class_="v1-shared",
    tag=dep_v1.Tag(id=9, label="shared"),
    maybe_tags=(dep_v1.Tag(id=2, label="b"),),
    kw_entry=kw_items.Entry(code=66),
)
v1_msg = g.v1.types.item.proto_type()
v1_msg.ParseFromString(g.v1.types.item.to_proto(v1_item).SerializeToString())
assert g.v1.types.item.from_proto(v1_msg) == v1_item

v2_item = p2.types.Item(
    from_=99999,
    class_="world",
    tag=dep_v2.Tag(id=500, label="v2", extra=True),
    maybe_tags=None,
    kw_entry=kw_items.Entry(code=88),
)
v2_msg = g.v2.types.item.proto_type()
v2_msg.ParseFromString(g.v2.types.item.to_proto(v2_item).SerializeToString())
assert g.v2.types.item.from_proto(v2_msg) == v2_item

# Named option<list<u16>>, option<list<u16, 2>>, option<map<string, msg>>, triple option<option<option<u8>>>
for triple_val in (NONE, Some(NONE), Some(Some(None)), Some(Some(42))):
    holder = p0.types.Holder(
        a=b"\x01\x02\x03",
        al=(10, 20, 30),
        af=(7, 8),
        b=Some(None),
        bt=triple_val,
        c=p0.types.TrickyFrom(value=9),
        d={7: "seven"},
        dm={"k": p0.types.Value(n=12)},
        e=p0.types.TrickyFlags(from_=True, class_=False, mro_=True),
        g=Some(b"\x04\x05"),
        h=(None, b"\x06"),
        hn=(NONE, Some(None), Some(77)),
        m={1: b"\x0a\x14\x1e\x28"},
    )
    h_proto = g.v0.types.holder.to_proto(holder)
    h_msg = g.v0.types.holder.proto_type()
    h_msg.ParseFromString(h_proto.SerializeToString())
    assert g.v0.types.holder.from_proto(h_msg) == holder
    h_json_msg = json_format.Parse(json_format.MessageToJson(h_proto), g.v0.types.holder.proto_type())
    assert g.v0.types.holder.from_proto(h_json_msg) == holder

# Builtin-shadowing interface (`record int`, `record bool`, `record tuple`, `record float`, ...)
sh_bundle = p0.shadowed.Bundle(
    xs=(10, 20),
    bs=b"\xaa\xbb",
    m={1: p0.shadowed.Shade.LIGHT, 2: p0.shadowed.Shade.DARK},
    f=3.25,
    s="clean",
    c=p0.shadowed.Shade.DARK,
    bits=p0.shadowed.Bits(from_=True, active=False),
    ch=p0.shadowed.ChoiceNum(value=500),
    opt=Some(9),
    res=Ok(42),
)
sh_proto = g.v0.shadowed.bundle.to_proto(sh_bundle)
sh_msg = g.v0.shadowed.bundle.proto_type()
sh_msg.ParseFromString(sh_proto.SerializeToString())
assert g.v0.shadowed.bundle.from_proto(sh_msg) == sh_bundle

# Shadowed `shade` enum must still reject `bool` (`True` == 1) and accept valid `1`!
assert g.v0.shadowed.shade.from_proto(1) is p0.shadowed.Shade.LIGHT
try:
    g.v0.shadowed.shade.from_proto(True)  # type: ignore[arg-type]
    raise AssertionError("expected bool True to be rejected by shadowed enum.from_proto")
except g.WitforgeGlueError as err:
    assert err.kind == "UnknownEnumValue"
    assert err.enumeration == "shadowed/shade"

echo_req = g.v0.shadowed.functions.echo.to_request((1, 2), {"a": 7}, 99, p0.shadowed.Tuple(xs=(3, 4)))
echo_msg = g.v0.shadowed.functions.echo.request_type()
echo_msg.ParseFromString(echo_req.SerializeToString())
assert g.v0.shadowed.functions.echo.from_request(echo_msg) == (
    (1, 2),
    {"a": 7},
    99,
    p0.shadowed.Tuple(xs=(3, 4)),
)

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

# Strict encode/decode type guards:
# 1. `bool` rejected for integers (encode + decode) and floats
try:
    g.v0.types.value.to_proto(p0.types.Value(n=True))  # type: ignore[arg-type]
    raise AssertionError("expected bool to be rejected for u8")
except g.WitforgeGlueError as err:
    assert err.kind == "IntegerOutOfRange"
    assert err.target == "types/value.n"

try:
    g_helpers.decode_char(True, "test.char")
    raise AssertionError("expected bool to be rejected by decode_char")
except g.WitforgeGlueError as err:
    assert err.kind == "InvalidChar"
    assert err.field == "test.char"

try:
    g.v0.shadowed.float.to_proto(p0.shadowed.Float(f=True))  # type: ignore[arg-type]
    raise AssertionError("expected bool to be rejected for f64")
except g.WitforgeGlueError as err:
    assert err.kind == "InvalidState"
    assert err.message == "shadowed/float.f"

# 2. Non-bool rejected for `flags` on encode (no silent `bool("false")` coercion!)
try:
    g.v0.types.tricky_flags.to_proto(
        p0.types.TrickyFlags(from_="false", class_=False, mro_=False)  # type: ignore[arg-type]
    )
    raise AssertionError("expected non-bool to be rejected in flags.to_proto")
except g.WitforgeGlueError as err:
    assert err.kind == "InvalidState"
    assert err.message == "types/tricky-flags.from"

# 3. Lone UTF-16 surrogate rejected for `string` on encode with WitforgeGlueError (not raw UnicodeEncodeError)
try:
    g.v0.shadowed.str.to_proto(p0.shadowed.Str(s="bad\ud800str"))
    raise AssertionError("expected lone surrogate string to be rejected")
except g.WitforgeGlueError as err:
    assert err.kind == "InvalidState"
    assert err.message == "shadowed/str.s"

# 4. Non-bool int rejected by `check_bool`
try:
    g_helpers.check_bool(1, "test.flag")
    raise AssertionError("expected int 1 to be rejected by check_bool")
except g.WitforgeGlueError as err:
    assert err.kind == "InvalidState"
    assert err.message == "test.flag"

# 5. Map key validation runs BEFORE value validation on encode AND decode!
bad_holder = p0.types.Holder(
    a=None,
    al=None,
    af=None,
    b=NONE,
    bt=NONE,
    c=p0.types.TrickyClass(),
    d=None,
    dm=None,
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

print(
    f"Python multi-minor, multi-major, canonical-owner, builtin-shadowing, and strict type-guard "
    f"edge smoke passed ({hint_count} type-hint targets verified)"
)
EOF

PYTHONPATH="${EDGE}/src:${EDGE}/src/pkg/proto${PYTHONPATH:+:${PYTHONPATH}}" \
    "${PYTHON_BIN}" "${EDGE}/smoke.py"
