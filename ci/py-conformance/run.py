"""End-to-end Python checked protobuf glue conformance runner (`W13-005`).

Executes every binary, proto3-JSON, and encode-bounds vector from the `W11-034`
conformance bundle against the generated Python plain types, `protoc --python_out`
`*_pb2.py` messages, and `witforge py-glue` modules.
"""

from __future__ import annotations

import base64
import json
import math
import struct
import sys
import typing
from pathlib import Path
from typing import Any, Callable

from google.protobuf import json_format

import glue as g
from glue import helpers as g_helpers
from plain.v0_1 import types as p_types
from plain.witforge_deps.witforge_conformance_dep import shared as p_dep_shared
from plain.witforge_types import NONE, Err, NoneValue, Ok, Some
from proto import v0_pb2 as proto_root


def _f32_round(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def _float_exact_eq(a: float, b: float, *, strict_zero_sign: bool, is_f32: bool) -> bool:
    if is_f32:
        a = _f32_round(a)
        b = _f32_round(b)
    if math.isnan(a) and math.isnan(b):
        return True
    if math.isnan(a) or math.isnan(b):
        return False
    if a == 0.0 and b == 0.0:
        if strict_zero_sign:
            return math.copysign(1.0, a) == math.copysign(1.0, b)
        return True
    return a == b


def _assert_float_matches(
    vec_id: str,
    field_name: str,
    got: float,
    want: Any,
    *,
    is_f32: bool,
) -> None:
    if math.isnan(got):
        assert want == "NaN", f"{vec_id}.{field_name}: expected NaN, got {want!r}"
        return
    if not math.isfinite(got):
        label = "Infinity" if got > 0 else "-Infinity"
        assert want == label, f"{vec_id}.{field_name}: expected {label}, got {want!r}"
        return
    assert isinstance(want, (int, float)) and not isinstance(want, bool), (
        f"{vec_id}.{field_name}: expected finite number, got {want!r}"
    )
    want_f = float(want)
    assert _float_exact_eq(got, want_f, strict_zero_sign=True, is_f32=is_f32), (
        f"{vec_id}.{field_name}: float mismatch (got={got!r}, want={want_f!r})"
    )


# Upstream `google.protobuf.json_format` rejects shortest-decimal IEEE-754 `f32::MAX` /
# `f32::MIN` (`3.4028235e38` / `-3.4028235e38`) because `_FLOAT_MAX` is
# `3.4028234663852886e+38`. Normalize only those two exact conformance vectors if
# `json_format.Parse` rejects the shortest decimal representation.
_UPSTREAM_F32_BOUNDARY_JSON_VECTORS = {
    "scalars-valid-char-e000": ("3.4028235e38", "3.4028234663852886e+38"),
    "scalars-valid-min-and-subnormals": ("-3.4028235e38", "-3.4028234663852886e+38"),
}


def _parse_proto3_json_accept(vec_id: str, raw_json: str, msg_cls: type[Any]) -> Any:
    try:
        return json_format.Parse(raw_json, msg_cls())
    except json_format.ParseError as exc:
        if vec_id in _UPSTREAM_F32_BOUNDARY_JSON_VECTORS:
            old, exact = _UPSTREAM_F32_BOUNDARY_JSON_VECTORS[vec_id]
            assert "Float value too" in str(exc), f"{vec_id}: unexpected ParseError: {exc}"
            patched = raw_json.replace(old, exact, 1)
            assert patched != raw_json, f"{vec_id}: expected `{old}` in proto3_json"
            return json_format.Parse(patched, msg_cls())
        raise


def _reparse_proto3_json_roundtrip(vec_id: str, re_json: str, msg_cls: type[Any]) -> Any:
    try:
        return json_format.Parse(re_json, msg_cls())
    except json_format.ParseError as exc:
        if vec_id in _UPSTREAM_F32_BOUNDARY_JSON_VECTORS:
            assert "Float value too" in str(exc), f"{vec_id}: unexpected roundtrip ParseError: {exc}"
            patched = re_json.replace("3.4028235e+38", "3.4028234663852886e+38")
            patched = patched.replace("-3.4028235e+38", "-3.4028234663852886e+38")
            return json_format.Parse(patched, msg_cls())
        raise


# --- Plain Python -> canonical `plain_json` serializers ---


def point_to_json(v: p_types.Point) -> dict[str, Any]:
    return {"x": v.x, "y": v.y}


def shared_meta_to_json(v: p_dep_shared.SharedMeta) -> dict[str, Any]:
    return {"id": str(v.id), "tag": v.tag, "status": v.status.value}


def integers_to_json(v: p_types.Integers) -> dict[str, Any]:
    return {
        "a-u8": v.a_u8,
        "a-u16": v.a_u16,
        "a-u32": v.a_u32,
        "a-u64": str(v.a_u64),
        "a-s8": v.a_s8,
        "a-s16": v.a_s16,
        "a-s32": v.a_s32,
        "a-s64": str(v.a_s64),
    }


def optionals_to_json(v: p_types.Optionals) -> dict[str, Any]:
    if isinstance(v.nested_opt, NoneValue):
        nested_opt: Any = None
    elif isinstance(v.nested_opt, Some):
        if v.nested_opt.value is None:
            nested_opt = {"none": None}
        else:
            nested_opt = {"some": v.nested_opt.value}
    else:
        raise AssertionError(f"unexpected nested_opt: {v.nested_opt!r}")

    return {
        "opt-u8": v.opt_u8,
        "opt-s8": v.opt_s8,
        "opt-char": v.opt_char,
        "opt-string": v.opt_string,
        "opt-bytes": None if v.opt_bytes is None else list(v.opt_bytes),
        "opt-fixed-bytes": None if v.opt_fixed_bytes is None else list(v.opt_fixed_bytes),
        "opt-list": None if v.opt_list is None else list(v.opt_list),
        "opt-enum": None if v.opt_enum is None else v.opt_enum.value,
        "opt-point": None if v.opt_point is None else point_to_json(v.opt_point),
        "nested-opt": nested_opt,
        "opt-map": (
            None
            if v.opt_map is None
            else {str(k): val for k, val in v.opt_map.items()}
        ),
    }


def collections_to_json(v: p_types.Collections) -> dict[str, Any]:
    return {
        "raw-bytes": list(v.raw_bytes),
        "fixed-bytes": list(v.fixed_bytes),
        "u16-items": list(v.u16_items),
        "fixed-u16s": list(v.fixed_u16s),
        "fixed-chars": list(v.fixed_chars),
        "point-items": [point_to_json(pt) for pt in v.point_items],
        "nested-lists": [list(inner) for inner in v.nested_lists],
        "nested-fixed": [list(inner) for inner in v.nested_fixed],
        "opt-items": list(v.opt_items),
        "result-items": [
            {"ok": item.value} if isinstance(item, Ok) else {"err": item.error.value}
            for item in v.result_items
        ],
    }


def _bool_key_str(k: bool) -> str:
    return "true" if k else "false"


def maps_to_json(v: p_types.Maps) -> dict[str, Any]:
    return {
        "str-to-u8": {k: val for k, val in v.str_to_u8.items()},
        "u8-to-str": {str(k): val for k, val in v.u8_to_str.items()},
        "s8-to-point": {str(k): point_to_json(val) for k, val in v.s8_to_point.items()},
        "char-to-color": {k: val.value for k, val in v.char_to_color.items()},
        "bool-to-s16": {_bool_key_str(k): val for k, val in v.bool_to_s16.items()},
        "u64-to-s64": {str(k): str(val) for k, val in v.u64_to_s64.items()},
        "nested-maps": [
            {str(k): val for k, val in m.items()} for m in v.nested_maps
        ],
    }


def choice_to_json(v: p_types.Choice) -> dict[str, Any]:
    if isinstance(v, p_types.ChoiceUnitCase):
        return {"case": "unit-case"}
    if isinstance(v, p_types.ChoiceSmall):
        return {"case": "small", "value": v.value}
    if isinstance(v, p_types.ChoiceGlyph):
        return {"case": "glyph", "value": v.value}
    if isinstance(v, p_types.ChoiceNested):
        return {"case": "nested", "value": point_to_json(v.value)}
    if isinstance(v, p_types.ChoiceFixed):
        return {"case": "fixed", "value": list(v.value)}
    raise AssertionError(f"unexpected choice: {v!r}")


def outcome_to_json(v: p_types.Outcome) -> dict[str, Any]:
    if isinstance(v, Ok):
        return {"ok": v.value}
    return {"err": v.error}


def results_to_json(v: p_types.Results) -> dict[str, Any]:
    return {
        "both": outcome_to_json(v.both),
        "only-ok": {"ok": v.only_ok.value} if isinstance(v.only_ok, Ok) else {"err": None},
        "only-err": {"ok": None} if isinstance(v.only_err, Ok) else {"err": v.only_err.error},
        "bare": {"ok": None} if isinstance(v.bare, Ok) else {"err": None},
    }


def permissions_to_json(v: p_types.Permissions) -> dict[str, Any]:
    return {"read": v.read, "write": v.write, "execute": v.execute}


def composites_to_json(v: p_types.Composites) -> dict[str, Any]:
    return {
        "origin": point_to_json(v.origin),
        "color": v.color.value,
        "perms": permissions_to_json(v.perms),
        "choice": choice_to_json(v.choice),
        "outcome": outcome_to_json(v.outcome),
        "foreign": shared_meta_to_json(v.foreign),
        "foreign-status": v.foreign_status.value,
    }


def assert_scalars_eq(
    vec_id: str,
    a: p_types.Scalars,
    b: p_types.Scalars,
    *,
    strict_zero_sign: bool,
) -> None:
    assert _float_exact_eq(a.a_f32, b.a_f32, strict_zero_sign=strict_zero_sign, is_f32=True), (
        f"{vec_id}: f32 mismatch ({a.a_f32!r} vs {b.a_f32!r})"
    )
    assert _float_exact_eq(a.a_f64, b.a_f64, strict_zero_sign=strict_zero_sign, is_f32=False), (
        f"{vec_id}: f64 mismatch ({a.a_f64!r} vs {b.a_f64!r})"
    )
    assert a.a_bool is b.a_bool, f"{vec_id}: bool mismatch"
    assert a.a_char == b.a_char, f"{vec_id}: char mismatch"
    assert a.a_string == b.a_string, f"{vec_id}: string mismatch"


def assert_scalars_plain_json(vec_id: str, val: p_types.Scalars, want: Any) -> None:
    assert isinstance(want, dict) and len(want) == 5, f"{vec_id}: expected 5 scalar keys"
    _assert_float_matches(vec_id, "a-f32", val.a_f32, want["a-f32"], is_f32=True)
    _assert_float_matches(vec_id, "a-f64", val.a_f64, want["a-f64"], is_f32=False)
    assert val.a_bool is want["a-bool"], f"{vec_id}.a-bool"
    assert val.a_char == want["a-char"], f"{vec_id}.a-char"
    assert val.a_string == want["a-string"], f"{vec_id}.a-string"


# --- Encode-vector `plain_json` -> plain Python builders (preserving invalid inputs) ---


def integers_from_json(raw: dict[str, Any]) -> p_types.Integers:
    return p_types.Integers(
        a_u8=raw["a-u8"],
        a_u16=raw["a-u16"],
        a_u32=raw["a-u32"],
        a_u64=int(raw["a-u64"]),
        a_s8=raw["a-s8"],
        a_s16=raw["a-s16"],
        a_s32=raw["a-s32"],
        a_s64=int(raw["a-s64"]),
    )


def point_from_json(raw: dict[str, Any]) -> p_types.Point:
    return p_types.Point(x=raw["x"], y=raw["y"])


def collections_from_json(raw: dict[str, Any]) -> p_types.Collections:
    return p_types.Collections(
        raw_bytes=bytes(raw["raw-bytes"]),
        fixed_bytes=bytes(raw["fixed-bytes"]),
        u16_items=tuple(raw["u16-items"]),
        fixed_u16s=tuple(raw["fixed-u16s"]),
        fixed_chars=tuple(raw["fixed-chars"]),
        point_items=tuple(point_from_json(pt) for pt in raw["point-items"]),
        nested_lists=tuple(tuple(inner) for inner in raw["nested-lists"]),
        nested_fixed=tuple(tuple(inner) for inner in raw["nested-fixed"]),
        opt_items=tuple(raw["opt-items"]),
        result_items=tuple(
            Ok(item["ok"]) if "ok" in item else Err(p_types.Color(item["err"]))
            for item in raw["result-items"]
        ),
    )


def maps_from_json(raw: dict[str, Any]) -> p_types.Maps:
    return p_types.Maps(
        str_to_u8=dict(raw["str-to-u8"]),
        u8_to_str={int(k): val for k, val in raw["u8-to-str"].items()},
        s8_to_point={int(k): point_from_json(val) for k, val in raw["s8-to-point"].items()},
        char_to_color={k: p_types.Color(val) for k, val in raw["char-to-color"].items()},
        bool_to_s16={k == "true": val for k, val in raw["bool-to-s16"].items()},
        u64_to_s64={int(k): int(val) for k, val in raw["u64-to-s64"].items()},
        nested_maps=tuple(
            {int(k): val for k, val in m.items()} for m in raw["nested-maps"]
        ),
    )


def _color_or_raw(val: Any) -> Any:
    try:
        return p_types.Color(val)
    except ValueError:
        return val


def _status_or_raw(val: Any) -> Any:
    try:
        return p_dep_shared.SharedStatus(val)
    except ValueError:
        return val


def composites_from_json(raw: dict[str, Any]) -> p_types.Composites:
    choice_obj = raw["choice"]
    tag = choice_obj["case"]
    choice: p_types.Choice
    if tag == "unit-case":
        choice = p_types.ChoiceUnitCase()
    elif tag == "small":
        choice = p_types.ChoiceSmall(value=choice_obj["value"])
    elif tag == "glyph":
        choice = p_types.ChoiceGlyph(value=choice_obj["value"])
    elif tag == "nested":
        choice = p_types.ChoiceNested(value=point_from_json(choice_obj["value"]))
    else:
        choice = p_types.ChoiceFixed(value=tuple(choice_obj["value"]))

    outcome_obj = raw["outcome"]
    outcome: p_types.Outcome = (
        Ok(outcome_obj["ok"]) if "ok" in outcome_obj else Err(outcome_obj["err"])
    )
    perms_obj = raw["perms"]
    foreign_obj = raw["foreign"]
    return p_types.Composites(
        origin=point_from_json(raw["origin"]),
        color=_color_or_raw(raw["color"]),
        perms=p_types.Permissions(
            read=perms_obj["read"],
            write=perms_obj["write"],
            execute=perms_obj["execute"],
        ),
        choice=choice,
        outcome=outcome,
        foreign=p_dep_shared.SharedMeta(
            id=int(foreign_obj["id"]),
            tag=foreign_obj["tag"],
            status=_status_or_raw(foreign_obj["status"]),
        ),
        foreign_status=_status_or_raw(raw["foreign-status"]),
    )


# --- Generic decode + round-trip runner ---


class DecodeRunner:
    def __init__(
        self,
        *,
        msg_cls: type[Any],
        from_proto: Callable[[Any], Any],
        to_proto: Callable[[Any], Any],
        check_plain_json: Callable[[str, Any, Any], None],
        assert_equal: Callable[[str, Any, Any, bool], None],
    ) -> None:
        self.msg_cls = msg_cls
        self.from_proto = from_proto
        self.to_proto = to_proto
        self.check_plain_json = check_plain_json
        self.assert_equal = assert_equal

    def run_decode(
        self,
        vec: dict[str, Any],
        bin_errors: set[str],
        json_errors: set[str],
    ) -> None:
        vec_id = vec["id"]
        raw_b64 = vec["proto_binary_base64"]
        raw_json = vec["proto3_json"]
        assert raw_b64 is not None, f"{vec_id}: missing proto_binary_base64"
        assert raw_json is not None, f"{vec_id}: missing proto3_json"

        wire_from_bin = self.msg_cls()
        wire_from_bin.ParseFromString(base64.b64decode(raw_b64))

        if vec["expected"] == "accept":
            # 1. Binary decode -> plain -> check -> encode -> binary -> decode -> compare
            plain_from_bin = self.from_proto(wire_from_bin)
            self.check_plain_json(vec_id, plain_from_bin, vec["plain_json"])
            re_bytes = self.to_proto(plain_from_bin).SerializeToString()
            re_msg = self.msg_cls()
            re_msg.ParseFromString(re_bytes)
            roundtrip_bin = self.from_proto(re_msg)
            self.assert_equal(vec_id, plain_from_bin, roundtrip_bin, True)

            # 2. Proto3-JSON decode -> plain -> check -> encode -> proto3-JSON -> decode -> compare
            wire_from_json = _parse_proto3_json_accept(vec_id, raw_json, self.msg_cls)
            plain_from_json = self.from_proto(wire_from_json)
            self.check_plain_json(vec_id, plain_from_json, vec["plain_json"])
            self.assert_equal(vec_id, plain_from_bin, plain_from_json, True)

            re_json = json_format.MessageToJson(self.to_proto(plain_from_json))
            roundtrip_json = self.from_proto(
                _reparse_proto3_json_roundtrip(vec_id, re_json, self.msg_cls)
            )
            self.assert_equal(vec_id, plain_from_json, roundtrip_json, False)
            return

        # Reject path
        assert vec["plain_json"] is None, f"{vec_id}: reject vector must have null plain_json"
        want_kind = vec["error_kind"]
        want_target = vec["error_target"]
        assert want_kind is not None and want_target is not None, f"{vec_id}: missing error expectations"

        # 1. Binary decode must raise WitforgeGlueError (a ValueError subclass) with exact kind & target
        bin_err: BaseException | None = None
        try:
            self.from_proto(wire_from_bin)
        except BaseException as exc:
            bin_err = exc
        assert isinstance(bin_err, g.WitforgeGlueError), (
            f"{vec_id}: expected binary WitforgeGlueError, got {bin_err!r}"
        )
        assert isinstance(bin_err, ValueError)
        assert bin_err.kind == want_kind, (
            f"{vec_id}: binary error kind mismatch (got {bin_err.kind!r}, want {want_kind!r})"
        )
        assert bin_err.target == want_target, (
            f"{vec_id}: binary error target mismatch (got {bin_err.target!r}, want {want_target!r})"
        )
        bin_errors.add(bin_err.kind)

        # 2. Proto3-JSON decode must parse cleanly and reject in `from_proto` with exact kind & target
        parsed_from_json = json_format.Parse(raw_json, self.msg_cls())
        json_err: BaseException | None = None
        try:
            self.from_proto(parsed_from_json)
        except BaseException as exc:
            json_err = exc
        assert isinstance(json_err, g.WitforgeGlueError), (
            f"{vec_id}: expected proto3-JSON WitforgeGlueError, got {json_err!r}"
        )
        assert json_err.kind == want_kind, (
            f"{vec_id}: JSON error kind mismatch (got {json_err.kind!r}, want {want_kind!r})"
        )
        assert json_err.target == want_target, (
            f"{vec_id}: JSON error target mismatch (got {json_err.target!r}, want {want_target!r})"
        )
        json_errors.add(json_err.kind)


def make_json_runner(
    msg_cls: type[Any],
    from_proto: Callable[[Any], Any],
    to_proto: Callable[[Any], Any],
    to_json: Callable[[Any], Any],
) -> DecodeRunner:
    def check(vec_id: str, val: Any, want: Any) -> None:
        got = to_json(val)
        assert got == want, f"{vec_id}: plain_json mismatch\n  got:  {got!r}\n  want: {want!r}"

    def eq(vec_id: str, a: Any, b: Any, _strict_zero: bool) -> None:
        ja = to_json(a)
        jb = to_json(b)
        assert ja == jb, f"{vec_id}: round-trip mismatch\n  a: {ja!r}\n  b: {jb!r}"

    return DecodeRunner(
        msg_cls=msg_cls,
        from_proto=from_proto,
        to_proto=to_proto,
        check_plain_json=check,
        assert_equal=eq,
    )


def run_encode_reject(vec: dict[str, Any], encode_errors: set[str]) -> None:
    vec_id = vec["id"]
    assert vec["expected"] == "reject", f"{vec_id}: unexpected encode accept vector"
    want_kind = vec["error_kind"]
    want_target = vec["error_target"]
    assert want_kind is not None and want_target is not None

    caught: BaseException | None = None
    try:
        msg_name = vec["proto_message"]
        plain_json = vec["plain_json"]
        if msg_name == ".witforge.conformance.v0.TypesIntegers":
            g.v0.types.integers.to_proto(integers_from_json(plain_json))
        elif msg_name == ".witforge.conformance.v0.TypesScalars":
            if "a-char" in plain_json:
                g.v0.types.scalars.to_proto(
                    p_types.Scalars(
                        a_f32=plain_json["a-f32"],
                        a_f64=plain_json["a-f64"],
                        a_bool=plain_json["a-bool"],
                        a_char=plain_json["a-char"],
                        a_string=plain_json["a-string"],
                    )
                )
            else:
                cp = plain_json["a-char-codepoint"]
                if 0xD800 <= cp <= 0xDFFF:
                    # Python `str` CAN represent lone surrogates: exercise `scalars.to_proto` directly!
                    g.v0.types.scalars.to_proto(
                        p_types.Scalars(
                            a_f32=plain_json["a-f32"],
                            a_f64=plain_json["a-f64"],
                            a_bool=plain_json["a-bool"],
                            a_char=chr(cp),
                            a_string=plain_json["a-string"],
                        )
                    )
                else:
                    # Out-of-Unicode code points (-1, 0x110000) cannot be constructed with `chr()`;
                    # exercise `decode_char` with the exact target.
                    g_helpers.decode_char(cp, want_target)
        elif msg_name == ".witforge.conformance.v0.TypesCollections":
            g.v0.types.collections.to_proto(collections_from_json(plain_json))
        elif msg_name == ".witforge.conformance.v0.TypesMaps":
            g.v0.types.maps.to_proto(maps_from_json(plain_json))
        elif msg_name == ".witforge.conformance.v0.TypesComposites":
            g.v0.types.composites.to_proto(composites_from_json(plain_json))
        else:
            raise AssertionError(f"{vec_id}: unhandled encode proto_message {msg_name}")
    except BaseException as exc:
        caught = exc

    assert isinstance(caught, g.WitforgeGlueError), (
        f"{vec_id}: expected encode WitforgeGlueError, got {caught!r}"
    )
    assert caught.kind == want_kind, (
        f"{vec_id}: encode error kind mismatch (got {caught.kind!r}, want {want_kind!r})"
    )
    assert caught.target == want_target, (
        f"{vec_id}: encode error target mismatch (got {caught.target!r}, want {want_target!r})"
    )
    encode_errors.add(caught.kind)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: python3 run.py <bundle-dir>")
    bundle_dir = Path(sys.argv[1])

    # Verify PEP 561 type hints resolve cleanly across emitted glue classes at runtime.
    for cls in (
        g.v0.types.integers,
        g.v0.types.scalars,
        g.v0.types.optionals,
        g.v0.types.collections,
        g.v0.types.maps,
        g.v0.types.choice,
        g.v0.types.results,
        g.v0.types.permissions,
        g.v0.types.composites,
        g.witforge_deps.witforge_conformance_dep.shared.shared_meta,
    ):
        assert typing.get_type_hints(cls.to_proto)
        assert typing.get_type_hints(cls.from_proto)
    for fn_cls in (
        g.v0.api.functions.echo_integers,
        g.v0.api.functions.process_foreign,
        g.v0.api.functions.notify,
    ):
        assert typing.get_type_hints(fn_cls.to_request)
        assert typing.get_type_hints(fn_cls.from_request)
        assert typing.get_type_hints(fn_cls.to_response)
        assert typing.get_type_hints(fn_cls.from_response)

    # Verify METHOD constants match generated `v0_pb2` enum values.
    assert g.v0.api.functions.echo_integers.METHOD == proto_root.API_METHOD_ECHO_INTEGERS
    assert g.v0.api.functions.process_foreign.METHOD == proto_root.API_METHOD_PROCESS_FOREIGN
    assert g.v0.api.functions.notify.METHOD == proto_root.API_METHOD_NOTIFY

    manifest = json.loads((bundle_dir / "vectors.json").read_text(encoding="utf-8"))
    assert manifest["conformance_version"] == 1, "unexpected conformance_version"

    runners: dict[str, DecodeRunner] = {
        ".witforge.conformance.v0.TypesIntegers": make_json_runner(
            g.v0.types.integers.proto_type,
            g.v0.types.integers.from_proto,
            g.v0.types.integers.to_proto,
            integers_to_json,
        ),
        ".witforge.conformance.v0.TypesScalars": DecodeRunner(
            msg_cls=g.v0.types.scalars.proto_type,
            from_proto=g.v0.types.scalars.from_proto,
            to_proto=g.v0.types.scalars.to_proto,
            check_plain_json=assert_scalars_plain_json,
            assert_equal=lambda vid, a, b, strict: assert_scalars_eq(
                vid, a, b, strict_zero_sign=strict
            ),
        ),
        ".witforge.conformance.v0.TypesOptionals": make_json_runner(
            g.v0.types.optionals.proto_type,
            g.v0.types.optionals.from_proto,
            g.v0.types.optionals.to_proto,
            optionals_to_json,
        ),
        ".witforge.conformance.v0.TypesCollections": make_json_runner(
            g.v0.types.collections.proto_type,
            g.v0.types.collections.from_proto,
            g.v0.types.collections.to_proto,
            collections_to_json,
        ),
        ".witforge.conformance.v0.TypesMaps": make_json_runner(
            g.v0.types.maps.proto_type,
            g.v0.types.maps.from_proto,
            g.v0.types.maps.to_proto,
            maps_to_json,
        ),
        ".witforge.conformance.v0.TypesChoice": make_json_runner(
            g.v0.types.choice.proto_type,
            g.v0.types.choice.from_proto,
            g.v0.types.choice.to_proto,
            choice_to_json,
        ),
        ".witforge.conformance.v0.TypesResults": make_json_runner(
            g.v0.types.results.proto_type,
            g.v0.types.results.from_proto,
            g.v0.types.results.to_proto,
            results_to_json,
        ),
        ".witforge.conformance.v0.TypesPermissions": make_json_runner(
            g.v0.types.permissions.proto_type,
            g.v0.types.permissions.from_proto,
            g.v0.types.permissions.to_proto,
            permissions_to_json,
        ),
        ".witforge.conformance.v0.TypesComposites": make_json_runner(
            g.v0.types.composites.proto_type,
            g.v0.types.composites.from_proto,
            g.v0.types.composites.to_proto,
            composites_to_json,
        ),
        ".witforge.conformance_dep.v0.SharedSharedMeta": make_json_runner(
            g.witforge_deps.witforge_conformance_dep.shared.shared_meta.proto_type,
            g.witforge_deps.witforge_conformance_dep.shared.shared_meta.from_proto,
            g.witforge_deps.witforge_conformance_dep.shared.shared_meta.to_proto,
            shared_meta_to_json,
        ),
        ".witforge.conformance.v0.ApiEchoIntegersRequest": make_json_runner(
            g.v0.api.functions.echo_integers.request_type,
            g.v0.api.functions.echo_integers.from_request,
            g.v0.api.functions.echo_integers.to_request,
            lambda inp: {"input": integers_to_json(inp)},
        ),
        ".witforge.conformance.v0.ApiEchoIntegersResponse": make_json_runner(
            g.v0.api.functions.echo_integers.response_type,
            g.v0.api.functions.echo_integers.from_response,
            g.v0.api.functions.echo_integers.to_response,
            lambda res: {"result": integers_to_json(res)},
        ),
        ".witforge.conformance.v0.ApiProcessForeignRequest": make_json_runner(
            g.v0.api.functions.process_foreign.request_type,
            g.v0.api.functions.process_foreign.from_request,
            lambda args: g.v0.api.functions.process_foreign.to_request(*args),
            lambda args: {
                "meta": shared_meta_to_json(args[0]),
                "status": args[1].value,
                "code": args[2],
            },
        ),
        ".witforge.conformance.v0.ApiProcessForeignResponse": make_json_runner(
            g.v0.api.functions.process_foreign.response_type,
            g.v0.api.functions.process_foreign.from_response,
            g.v0.api.functions.process_foreign.to_response,
            lambda res: {
                "result": (
                    {"ok": point_to_json(res.value)}
                    if isinstance(res, Ok)
                    else {"err": res.error}
                )
            },
        ),
        ".witforge.conformance.v0.ApiNotifyRequest": make_json_runner(
            g.v0.api.functions.notify.request_type,
            g.v0.api.functions.notify.from_request,
            lambda args: g.v0.api.functions.notify.to_request(*args),
            lambda args: {"label": args[0], "priority": args[1]},
        ),
        ".witforge.conformance.v0.ApiNotifyResponse": make_json_runner(
            g.v0.api.functions.notify.response_type,
            g.v0.api.functions.notify.from_response,
            lambda _: g.v0.api.functions.notify.to_response(),
            lambda _: {},
        ),
    }

    decode_accept_count = 0
    decode_reject_count = 0
    encode_reject_count = 0
    bin_errors: set[str] = set()
    json_errors: set[str] = set()
    encode_errors: set[str] = set()

    for vec in manifest["vectors"]:
        if vec["direction"] == "encode":
            run_encode_reject(vec, encode_errors)
            encode_reject_count += 1
            continue
        runner = runners.get(vec["proto_message"])
        assert runner is not None, f"{vec['id']}: missing runner for {vec['proto_message']}"
        runner.run_decode(vec, bin_errors, json_errors)
        if vec["expected"] == "accept":
            decode_accept_count += 1
        else:
            decode_reject_count += 1

    total = decode_accept_count + decode_reject_count + encode_reject_count
    assert total == len(manifest["vectors"]), "vector count mismatch"

    for kind in manifest["error_kinds"]:
        assert kind in bin_errors, f"binary decode did not exercise error kind {kind}"
        assert kind in json_errors, f"proto3-JSON decode did not exercise error kind {kind}"
    for kind in ("IntegerOutOfRange", "InvalidChar", "InvalidLength", "UnknownEnumValue"):
        assert kind in encode_errors, f"encode bounds did not exercise error kind {kind}"

    print(
        f"Python conformance passed: {total} vectors "
        f"({decode_accept_count} decode accept, {decode_reject_count} decode reject, "
        f"{encode_reject_count} encode reject; all {len(bin_errors)} GlueError kinds verified)"
    )


if __name__ == "__main__":
    main()
