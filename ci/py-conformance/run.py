"""End-to-end Python checked protobuf glue conformance runner (`W13-005`).

Executes every binary, proto3-JSON, and encode-bounds vector from the `W11-034`
conformance bundle against the generated Python plain types, `protoc --python_out`
`*_pb2.py` messages, and `witforge py-glue` modules.
"""

from __future__ import annotations

import base64
import inspect
import json
import math
import struct
import sys
import typing
from pathlib import Path
from typing import Any, Callable

if not __debug__:
    raise RuntimeError("conformance runner must not run under -O / PYTHONOPTIMIZE")

from google.protobuf import json_format

import glue as g
from glue import error as g_error
from glue import helpers as g_helpers
from plain.v0_1 import api as p_api
from plain.v0_1 import types as p_types
from plain.witforge_deps.witforge_conformance_dep import shared as p_dep_shared
from plain.witforge_types import NONE, Err, NoneValue, Ok, Some
from proto import v0_pb2 as proto_root


def _f32_round(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def _float_exact_eq(a: float, b: float, *, strict_zero_sign: bool, is_f32: bool) -> bool:
    assert isinstance(a, float) and not isinstance(a, bool), f"expected float, got {a!r}"
    assert isinstance(b, float) and not isinstance(b, bool), f"expected float, got {b!r}"
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
    got: Any,
    want: Any,
    *,
    is_f32: bool,
) -> None:
    assert isinstance(got, float) and not isinstance(got, bool), (
        f"{vec_id}.{field_name}: expected Python float, got {type(got).__name__}({got!r})"
    )
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


def _assert_strict_json_eq(ctx: str, got: Any, want: Any) -> None:
    """Assert exact structural AND runtime-type equality (rejecting Python bool==int and int==float)."""
    assert type(got) is type(want), (
        f"{ctx}: type mismatch (got {type(got).__name__}({got!r}), "
        f"want {type(want).__name__}({want!r}))"
    )
    if isinstance(want, dict):
        assert set(got.keys()) == set(want.keys()), (
            f"{ctx}: key mismatch (got {sorted(got.keys())!r}, want {sorted(want.keys())!r})"
        )
        for k in want:
            _assert_strict_json_eq(f"{ctx}.{k}", got[k], want[k])
    elif isinstance(want, list):
        assert len(got) == len(want), (
            f"{ctx}: length mismatch (got {len(got)}, want {len(want)})"
        )
        for idx, (g_item, w_item) in enumerate(zip(got, want)):
            _assert_strict_json_eq(f"{ctx}[{idx}]", g_item, w_item)
    else:
        assert got == want, f"{ctx}: value mismatch (got {got!r}, want {want!r})"


def _assert_glue_error_details(ctx: str, err: g.WitforgeGlueError, want_kind: str, want_target: str) -> None:
    assert isinstance(err, g.WitforgeGlueError), f"{ctx}: expected WitforgeGlueError, got {err!r}"
    assert isinstance(err, ValueError)
    assert err.kind == want_kind, f"{ctx}: error kind mismatch (got {err.kind!r}, want {want_kind!r})"
    assert err.target == want_target, f"{ctx}: error target mismatch (got {err.target!r}, want {want_target!r})"
    if want_kind == "MissingField":
        assert err.field == want_target and err.enumeration is None and err.message is None
    elif want_kind == "IntegerOutOfRange":
        assert err.field == want_target and err.value is not None
    elif want_kind == "UnknownEnumValue":
        assert err.enumeration == want_target and err.value is not None
    elif want_kind in ("UnknownCase", "InvalidState"):
        assert err.message == want_target and err.field is None and err.enumeration is None
    elif want_kind == "InvalidChar":
        assert err.field == want_target and err.value is not None
    elif want_kind == "InvalidLength":
        assert (
            err.field == want_target
            and isinstance(err.expected, int)
            and not isinstance(err.expected, bool)
            and isinstance(err.actual, int)
            and not isinstance(err.actual, bool)
            and err.expected != err.actual
        )
    else:
        raise AssertionError(f"{ctx}: unexpected error kind {want_kind!r}")


# Upstream `google.protobuf.internal.type_checkers._FLOAT_MAX` is `3.4028234663852886e+38`,
# which is strictly smaller than shortest-decimal IEEE-754 `f32::MAX` (`3.4028235e38`) and
# `MessageToJson`'s own `3.4028235e+38`. Normalize ONLY these two exact conformance vectors
# with their exact expected ParseError substring if `json_format.Parse` rejects them.
_UPSTREAM_F32_BOUNDARY_JSON_VECTORS: dict[str, tuple[str, str, str, str]] = {
    "scalars-valid-char-e000": (
        '"aF32":3.4028235e38',
        '"aF32": 3.4028235e+38',
        '"aF32": 3.4028234663852886e+38',
        "Float value too large",
    ),
    "scalars-valid-min-and-subnormals": (
        '"aF32":-3.4028235e38',
        '"aF32": -3.4028235e+38',
        '"aF32": -3.4028234663852886e+38',
        "Float value too small",
    ),
}


def _parse_proto3_json_accept(vec_id: str, raw_json: str, msg_cls: type[Any]) -> Any:
    try:
        return json_format.Parse(raw_json, msg_cls())
    except json_format.ParseError as exc:
        if vec_id in _UPSTREAM_F32_BOUNDARY_JSON_VECTORS:
            input_lit, _rt_lit, exact_lit, err_substr = _UPSTREAM_F32_BOUNDARY_JSON_VECTORS[vec_id]
            assert err_substr in str(exc), f"{vec_id}: unexpected ParseError: {exc}"
            patched = raw_json.replace(input_lit, exact_lit, 1)
            assert patched != raw_json, f"{vec_id}: expected `{input_lit}` in proto3_json"
            return json_format.Parse(patched, msg_cls())
        raise


def _reparse_proto3_json_roundtrip(vec_id: str, re_json: str, msg_cls: type[Any]) -> Any:
    try:
        return json_format.Parse(re_json, msg_cls())
    except json_format.ParseError as exc:
        if vec_id in _UPSTREAM_F32_BOUNDARY_JSON_VECTORS:
            _input_lit, rt_lit, exact_lit, err_substr = _UPSTREAM_F32_BOUNDARY_JSON_VECTORS[vec_id]
            assert err_substr in str(exc), f"{vec_id}: unexpected roundtrip ParseError: {exc}"
            patched = re_json.replace(rt_lit, exact_lit, 1)
            assert patched != re_json, f"{vec_id}: expected `{rt_lit}` in roundtrip proto3_json"
            return json_format.Parse(patched, msg_cls())
        raise


# --- Plain Python -> canonical `plain_json` serializers (with strict runtime type assertions) ---


def _require_bytes(ctx: str, val: Any) -> list[int]:
    assert type(val) is bytes, f"{ctx}: expected bytes, got {type(val).__name__}({val!r})"
    return list(val)


def _require_tuple(ctx: str, val: Any) -> tuple[Any, ...]:
    assert type(val) is tuple, f"{ctx}: expected tuple, got {type(val).__name__}({val!r})"
    return val


def _require_color(ctx: str, val: Any) -> str:
    assert isinstance(val, p_types.Color), f"{ctx}: expected Color, got {type(val).__name__}({val!r})"
    return str(val.value)


def _require_status(ctx: str, val: Any) -> str:
    assert isinstance(val, p_dep_shared.SharedStatus), (
        f"{ctx}: expected SharedStatus, got {type(val).__name__}({val!r})"
    )
    return str(val.value)


def _require_int_str(ctx: str, val: Any) -> str:
    assert type(val) is int, f"{ctx}: expected int, got {type(val).__name__}({val!r})"
    return str(val)


def point_to_json(v: p_types.Point) -> dict[str, Any]:
    assert isinstance(v, p_types.Point)
    return {"x": v.x, "y": v.y}


def shared_meta_to_json(v: p_dep_shared.SharedMeta) -> dict[str, Any]:
    assert isinstance(v, p_dep_shared.SharedMeta)
    return {
        "id": _require_int_str("SharedMeta.id", v.id),
        "tag": v.tag,
        "status": _require_status("SharedMeta.status", v.status),
    }


def integers_to_json(v: p_types.Integers) -> dict[str, Any]:
    assert isinstance(v, p_types.Integers)
    return {
        "a-u8": v.a_u8,
        "a-u16": v.a_u16,
        "a-u32": v.a_u32,
        "a-u64": _require_int_str("Integers.a-u64", v.a_u64),
        "a-s8": v.a_s8,
        "a-s16": v.a_s16,
        "a-s32": v.a_s32,
        "a-s64": _require_int_str("Integers.a-s64", v.a_s64),
    }


def optionals_to_json(v: p_types.Optionals) -> dict[str, Any]:
    assert isinstance(v, p_types.Optionals)
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
        "opt-bytes": None if v.opt_bytes is None else _require_bytes("Optionals.opt-bytes", v.opt_bytes),
        "opt-fixed-bytes": (
            None
            if v.opt_fixed_bytes is None
            else _require_bytes("Optionals.opt-fixed-bytes", v.opt_fixed_bytes)
        ),
        "opt-list": (
            None
            if v.opt_list is None
            else list(_require_tuple("Optionals.opt-list", v.opt_list))
        ),
        "opt-enum": None if v.opt_enum is None else _require_color("Optionals.opt-enum", v.opt_enum),
        "opt-point": None if v.opt_point is None else point_to_json(v.opt_point),
        "nested-opt": nested_opt,
        "opt-map": (
            None
            if v.opt_map is None
            else {_require_int_str("Optionals.opt-map.key", k): val for k, val in v.opt_map.items()}
        ),
    }


def collections_to_json(v: p_types.Collections) -> dict[str, Any]:
    assert isinstance(v, p_types.Collections)
    result_items: list[dict[str, Any]] = []
    for item in _require_tuple("Collections.result-items", v.result_items):
        if isinstance(item, Ok):
            result_items.append({"ok": item.value})
        elif isinstance(item, Err):
            result_items.append({"err": _require_color("Collections.result-items.err", item.error)})
        else:
            raise AssertionError(f"unexpected result item: {item!r}")

    return {
        "raw-bytes": _require_bytes("Collections.raw-bytes", v.raw_bytes),
        "fixed-bytes": _require_bytes("Collections.fixed-bytes", v.fixed_bytes),
        "u16-items": list(_require_tuple("Collections.u16-items", v.u16_items)),
        "fixed-u16s": list(_require_tuple("Collections.fixed-u16s", v.fixed_u16s)),
        "fixed-chars": list(_require_tuple("Collections.fixed-chars", v.fixed_chars)),
        "point-items": [
            point_to_json(pt) for pt in _require_tuple("Collections.point-items", v.point_items)
        ],
        "nested-lists": [
            list(_require_tuple("Collections.nested-lists[]", inner))
            for inner in _require_tuple("Collections.nested-lists", v.nested_lists)
        ],
        "nested-fixed": [
            list(_require_tuple("Collections.nested-fixed[]", inner))
            for inner in _require_tuple("Collections.nested-fixed", v.nested_fixed)
        ],
        "opt-items": list(_require_tuple("Collections.opt-items", v.opt_items)),
        "result-items": result_items,
    }


def _bool_key_str(k: Any) -> str:
    assert type(k) is bool, f"expected bool map key, got {type(k).__name__}({k!r})"
    return "true" if k else "false"


def maps_to_json(v: p_types.Maps) -> dict[str, Any]:
    assert isinstance(v, p_types.Maps)
    return {
        "str-to-u8": {k: val for k, val in v.str_to_u8.items()},
        "u8-to-str": {_require_int_str("Maps.u8-to-str.key", k): val for k, val in v.u8_to_str.items()},
        "s8-to-point": {
            _require_int_str("Maps.s8-to-point.key", k): point_to_json(val)
            for k, val in v.s8_to_point.items()
        },
        "char-to-color": {
            k: _require_color("Maps.char-to-color.val", val) for k, val in v.char_to_color.items()
        },
        "bool-to-s16": {_bool_key_str(k): val for k, val in v.bool_to_s16.items()},
        "u64-to-s64": {
            _require_int_str("Maps.u64-to-s64.key", k): _require_int_str("Maps.u64-to-s64.val", val)
            for k, val in v.u64_to_s64.items()
        },
        "nested-maps": [
            {_require_int_str("Maps.nested-maps.key", k): val for k, val in m.items()}
            for m in _require_tuple("Maps.nested-maps", v.nested_maps)
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
        return {"case": "fixed", "value": list(_require_tuple("Choice.fixed", v.value))}
    raise AssertionError(f"unexpected choice: {v!r}")


def outcome_to_json(v: p_types.Outcome) -> dict[str, Any]:
    if isinstance(v, Ok):
        return {"ok": v.value}
    if isinstance(v, Err):
        return {"err": v.error}
    raise AssertionError(f"unexpected outcome: {v!r}")


def _only_ok_to_json(v: p_types.OkOnly) -> dict[str, Any]:
    if isinstance(v, Ok):
        return {"ok": v.value}
    if isinstance(v, Err):
        assert v.error is None, f"expected unit Err(None), got Err({v.error!r})"
        return {"err": None}
    raise AssertionError(f"unexpected ok-only result: {v!r}")


def _only_err_to_json(v: p_types.ErrOnly) -> dict[str, Any]:
    if isinstance(v, Ok):
        assert v.value is None, f"expected unit Ok(None), got Ok({v.value!r})"
        return {"ok": None}
    if isinstance(v, Err):
        return {"err": _require_color("Results.only-err.err", v.error)}
    raise AssertionError(f"unexpected err-only result: {v!r}")


def _bare_result_to_json(v: p_types.BareResult) -> dict[str, Any]:
    if isinstance(v, Ok):
        assert v.value is None, f"expected unit Ok(None), got Ok({v.value!r})"
        return {"ok": None}
    if isinstance(v, Err):
        assert v.error is None, f"expected unit Err(None), got Err({v.error!r})"
        return {"err": None}
    raise AssertionError(f"unexpected bare result: {v!r}")


def results_to_json(v: p_types.Results) -> dict[str, Any]:
    assert isinstance(v, p_types.Results)
    return {
        "both": outcome_to_json(v.both),
        "only-ok": _only_ok_to_json(v.only_ok),
        "only-err": _only_err_to_json(v.only_err),
        "bare": _bare_result_to_json(v.bare),
    }


def permissions_to_json(v: p_types.Permissions) -> dict[str, Any]:
    assert isinstance(v, p_types.Permissions)
    return {"read": v.read, "write": v.write, "execute": v.execute}


def composites_to_json(v: p_types.Composites) -> dict[str, Any]:
    assert isinstance(v, p_types.Composites)
    return {
        "origin": point_to_json(v.origin),
        "color": _require_color("Composites.color", v.color),
        "perms": permissions_to_json(v.perms),
        "choice": choice_to_json(v.choice),
        "outcome": outcome_to_json(v.outcome),
        "foreign": shared_meta_to_json(v.foreign),
        "foreign-status": _require_status("Composites.foreign-status", v.foreign_status),
    }


def _process_foreign_response_to_json(res: Any) -> dict[str, Any]:
    if isinstance(res, Ok):
        return {"result": {"ok": point_to_json(res.value)}}
    if isinstance(res, Err):
        return {"result": {"err": _require_color("ApiProcessForeignResponse.err", res.error)}}
    raise AssertionError(f"unexpected process-foreign response: {res!r}")


def _notify_response_to_json(res: Any) -> dict[str, Any]:
    assert res is None, f"expected None from notify.from_response, got {res!r}"
    return {}


def _notify_to_response(arg: Any) -> Any:
    assert arg is None, f"expected None for notify.to_response, got {arg!r}"
    return g.v0.api.functions.notify.to_response()


def assert_scalars_eq(
    vec_id: str,
    a: p_types.Scalars,
    b: p_types.Scalars,
    *,
    strict_zero_sign: bool,
) -> None:
    assert isinstance(a, p_types.Scalars) and isinstance(b, p_types.Scalars)
    assert _float_exact_eq(a.a_f32, b.a_f32, strict_zero_sign=strict_zero_sign, is_f32=True), (
        f"{vec_id}: f32 mismatch ({a.a_f32!r} vs {b.a_f32!r})"
    )
    assert _float_exact_eq(a.a_f64, b.a_f64, strict_zero_sign=strict_zero_sign, is_f32=False), (
        f"{vec_id}: f64 mismatch ({a.a_f64!r} vs {b.a_f64!r})"
    )
    assert type(a.a_bool) is bool and a.a_bool is b.a_bool, f"{vec_id}: bool mismatch"
    assert type(a.a_char) is str and a.a_char == b.a_char, f"{vec_id}: char mismatch"
    assert type(a.a_string) is str and a.a_string == b.a_string, f"{vec_id}: string mismatch"


def assert_scalars_plain_json(vec_id: str, val: p_types.Scalars, want: Any) -> None:
    assert isinstance(val, p_types.Scalars)
    assert isinstance(want, dict) and len(want) == 5, f"{vec_id}: expected 5 scalar keys"
    _assert_float_matches(vec_id, "a-f32", val.a_f32, want["a-f32"], is_f32=True)
    _assert_float_matches(vec_id, "a-f64", val.a_f64, want["a-f64"], is_f32=False)
    assert type(val.a_bool) is bool and val.a_bool is want["a-bool"], f"{vec_id}.a-bool"
    assert type(val.a_char) is str and val.a_char == want["a-char"], f"{vec_id}.a-char"
    assert type(val.a_string) is str and val.a_string == want["a-string"], f"{vec_id}.a-string"


# --- Encode-vector `plain_json` -> plain Python builders (preserving invalid inputs) ---


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
            Ok(item["ok"]) if "ok" in item else Err(_color_or_raw(item["err"]))
            for item in raw["result-items"]
        ),
    )


def maps_from_json(raw: dict[str, Any]) -> p_types.Maps:
    return p_types.Maps(
        str_to_u8=dict(raw["str-to-u8"]),
        u8_to_str={int(k): val for k, val in raw["u8-to-str"].items()},
        s8_to_point={int(k): point_from_json(val) for k, val in raw["s8-to-point"].items()},
        char_to_color={k: _color_or_raw(val) for k, val in raw["char-to-color"].items()},
        bool_to_s16={k == "true": val for k, val in raw["bool-to-s16"].items()},
        u64_to_s64={int(k): int(val) for k, val in raw["u64-to-s64"].items()},
        nested_maps=tuple(
            {int(k): val for k, val in m.items()} for m in raw["nested-maps"]
        ),
    )


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


# --- Exhaustive runtime `typing.get_type_hints()` verifier ---


def verify_all_type_hints(modules: list[Any]) -> int:
    checked = 0
    for mod in modules:
        exported = getattr(mod, "__all__", None)
        assert isinstance(exported, tuple), f"{mod.__name__}: missing __all__ tuple"
        for name in exported:
            obj = getattr(mod, name)
            if inspect.isclass(obj):
                typing.get_type_hints(obj)
                checked += 1
                for attr_name, raw_attr in obj.__dict__.items():
                    if attr_name.startswith("__") and attr_name != "__init__":
                        continue
                    if isinstance(raw_attr, staticmethod):
                        typing.get_type_hints(raw_attr.__func__)
                        checked += 1
                    elif inspect.isfunction(raw_attr):
                        typing.get_type_hints(raw_attr)
                        checked += 1
                    elif inspect.isclass(raw_attr):
                        typing.get_type_hints(raw_attr)
                        checked += 1
                        for fn_attr in raw_attr.__dict__.values():
                            if isinstance(fn_attr, staticmethod):
                                typing.get_type_hints(fn_attr.__func__)
                                checked += 1
            elif inspect.isfunction(obj):
                typing.get_type_hints(obj)
                checked += 1
    assert checked > 0
    return checked


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
        assert vec["expected"] in ("accept", "reject"), (
            f"{vec_id}: unexpected expected={vec['expected']!r}"
        )
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
            self.assert_equal(vec_id, plain_from_json, roundtrip_json, True)
            return

        # Reject path
        assert vec["plain_json"] is None, f"{vec_id}: reject vector must have null plain_json"
        want_kind = vec["error_kind"]
        want_target = vec["error_target"]
        assert want_kind is not None and want_target is not None, (
            f"{vec_id}: missing error expectations"
        )

        # 1. Binary decode must raise WitforgeGlueError (ValueError subclass) with exact kind/target/details
        bin_err: BaseException | None = None
        try:
            self.from_proto(wire_from_bin)
        except BaseException as exc:
            bin_err = exc
        assert isinstance(bin_err, g.WitforgeGlueError), (
            f"{vec_id}: expected binary WitforgeGlueError, got {bin_err!r}"
        )
        _assert_glue_error_details(f"{vec_id} (binary)", bin_err, want_kind, want_target)
        bin_errors.add(bin_err.kind)

        # 2. Proto3-JSON decode must parse cleanly and reject in `from_proto` with exact kind/target/details
        parsed_from_json = json_format.Parse(raw_json, self.msg_cls())
        json_err: BaseException | None = None
        try:
            self.from_proto(parsed_from_json)
        except BaseException as exc:
            json_err = exc
        assert isinstance(json_err, g.WitforgeGlueError), (
            f"{vec_id}: expected proto3-JSON WitforgeGlueError, got {json_err!r}"
        )
        _assert_glue_error_details(f"{vec_id} (json)", json_err, want_kind, want_target)
        json_errors.add(json_err.kind)


def make_json_runner(
    msg_cls: type[Any],
    from_proto: Callable[[Any], Any],
    to_proto: Callable[[Any], Any],
    to_json: Callable[[Any], Any],
) -> DecodeRunner:
    def check(vec_id: str, val: Any, want: Any) -> None:
        got = to_json(val)
        _assert_strict_json_eq(f"{vec_id} (plain_json)", got, want)

    def eq(vec_id: str, a: Any, b: Any, _strict_zero: bool) -> None:
        assert a == b, f"{vec_id}: plain value round-trip mismatch\n  a: {a!r}\n  b: {b!r}"
        ja = to_json(a)
        jb = to_json(b)
        _assert_strict_json_eq(f"{vec_id} (round-trip)", ja, jb)

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
    _assert_glue_error_details(f"{vec_id} (encode)", caught, want_kind, want_target)
    encode_errors.add(caught.kind)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: python3 run.py <bundle-dir>")
    bundle_dir = Path(sys.argv[1])

    # Exhaustively verify PEP 561 type hints resolve cleanly across EVERY emitted module & symbol.
    checked_hints = verify_all_type_hints(
        [
            g_error,
            g_helpers,
            g.v0.types,
            g.v0.api,
            g.witforge_deps.witforge_conformance_dep.shared,
            p_types,
            p_api,
            p_dep_shared,
        ]
    )
    assert checked_hints >= 80, f"expected >=80 resolved type-hint targets, got {checked_hints}"

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
                "status": _require_status("ApiProcessForeignRequest.status", args[1]),
                "code": args[2],
            },
        ),
        ".witforge.conformance.v0.ApiProcessForeignResponse": make_json_runner(
            g.v0.api.functions.process_foreign.response_type,
            g.v0.api.functions.process_foreign.from_response,
            g.v0.api.functions.process_foreign.to_response,
            _process_foreign_response_to_json,
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
            _notify_to_response,
            _notify_response_to_json,
        ),
    }

    decode_accept_count = 0
    decode_reject_count = 0
    encode_reject_count = 0
    bin_errors: set[str] = set()
    json_errors: set[str] = set()
    encode_errors: set[str] = set()

    for vec in manifest["vectors"]:
        direction = vec["direction"]
        assert direction in ("decode", "encode"), f"{vec['id']}: unexpected direction={direction!r}"
        if direction == "encode":
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
        f"{encode_reject_count} encode reject; all {len(bin_errors)} GlueError kinds and "
        f"{checked_hints} type-hint targets verified)"
    )


if __name__ == "__main__":
    main()
