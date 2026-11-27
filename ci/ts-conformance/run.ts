import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";

import {
  type DescMessage,
  type MessageShape,
  fromBinary,
  fromJsonString,
  toBinary,
  toJsonString,
} from "@bufbuild/protobuf";

import * as g from "./glue/index.js";
import * as gHelpers from "./glue/helpers.js";
import * as p from "./plain/v0_1/index.js";
import * as pDep from "./plain/witforge_deps/witforge_conformance_dep/index.js";
import { NONE, some, type Option } from "./plain/witforge_types.js";
import * as protoRoot from "./proto/v0_pb.js";

type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

interface ConformanceVector {
  readonly id: string;
  readonly category: string;
  readonly direction: "decode" | "encode";
  readonly target_kind: "type" | "request" | "response";
  readonly target: string;
  readonly proto_message: string;
  readonly expected: "accept" | "reject";
  readonly error_kind: string | null;
  readonly error_target: string | null;
  readonly proto_binary_base64: string | null;
  readonly proto3_json: string | null;
  readonly plain_json: JsonValue;
  readonly description: string;
}

interface ConformanceManifest {
  readonly conformance_version: number;
  readonly error_kinds: readonly string[];
  readonly vectors: readonly ConformanceVector[];
}

function asObj(value: JsonValue, ctx: string): Record<string, JsonValue> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${ctx}: expected JSON object, got ${JSON.stringify(value)}`);
  }
  return value as Record<string, JsonValue>;
}

function asArr(value: JsonValue, ctx: string): JsonValue[] {
  if (!Array.isArray(value)) {
    throw new Error(`${ctx}: expected JSON array, got ${JSON.stringify(value)}`);
  }
  return value;
}

function field(obj: Record<string, JsonValue>, key: string, ctx: string): JsonValue {
  const val = obj[key];
  if (val === undefined) {
    throw new Error(`${ctx}: missing JSON key "${key}"`);
  }
  return val;
}

function decodeBase64(b64: string): Uint8Array {
  const buf = Buffer.from(b64, "base64");
  return new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
}

// --- Plain TS -> canonical `plain_json` serializers ---

function pointToJson(v: p.types.Point): JsonValue {
  return { x: v.x, y: v.y };
}

function sharedMetaToJson(v: pDep.shared.SharedMeta): JsonValue {
  return { id: v.id.toString(), tag: v.tag, status: v.status };
}

function integersToJson(v: p.types.Integers): JsonValue {
  return {
    "a-u8": v.aU8,
    "a-u16": v.aU16,
    "a-u32": v.aU32,
    "a-u64": v.aU64.toString(),
    "a-s8": v.aS8,
    "a-s16": v.aS16,
    "a-s32": v.aS32,
    "a-s64": v.aS64.toString(),
  };
}

function optionToJson<T>(opt: Option<T>, fn: (x: T) => JsonValue): JsonValue {
  return opt.tag === "none" ? null : fn(opt.value);
}

function optionalsToJson(v: p.types.Optionals): JsonValue {
  let nestedOpt: JsonValue;
  if (v.nestedOpt.tag === "none") {
    nestedOpt = null;
  } else if (v.nestedOpt.value.tag === "none") {
    nestedOpt = { none: null };
  } else {
    nestedOpt = { some: v.nestedOpt.value.value };
  }
  return {
    "opt-u8": optionToJson(v.optU8, (n) => n),
    "opt-s8": optionToJson(v.optS8, (n) => n),
    "opt-char": optionToJson(v.optChar, (s) => s),
    "opt-string": optionToJson(v.optString, (s) => s),
    "opt-bytes": optionToJson(v.optBytes, (b) => Array.from(b)),
    "opt-fixed-bytes": optionToJson(v.optFixedBytes, (b) => Array.from(b)),
    "opt-list": optionToJson(v.optList, (l) => Array.from(l)),
    "opt-enum": optionToJson(v.optEnum, (c) => c),
    "opt-point": optionToJson(v.optPoint, pointToJson),
    "nested-opt": nestedOpt,
    "opt-map": optionToJson(v.optMap, (m) => {
      const obj: Record<string, JsonValue> = {};
      for (const [k, val] of m.entries()) {
        obj[String(k)] = val;
      }
      return obj;
    }),
  };
}

function collectionsToJson(v: p.types.Collections): JsonValue {
  return {
    "raw-bytes": Array.from(v.rawBytes),
    "fixed-bytes": Array.from(v.fixedBytes),
    "u16-items": Array.from(v.u16Items),
    "fixed-u16s": Array.from(v.fixedU16s),
    "fixed-chars": Array.from(v.fixedChars),
    "point-items": v.pointItems.map(pointToJson),
    "nested-lists": v.nestedLists.map((inner) => Array.from(inner)),
    "nested-fixed": v.nestedFixed.map((inner) => Array.from(inner)),
    "opt-items": v.optItems.map((o) => optionToJson(o, (n) => n)),
    "result-items": v.resultItems.map((r) =>
      r.tag === "ok" ? { ok: r.value } : { err: r.error },
    ),
  };
}

function mapToJson<K, V>(
  m: ReadonlyMap<K, V>,
  kFn: (k: K) => string,
  vFn: (v: V) => JsonValue,
): JsonValue {
  const obj: Record<string, JsonValue> = {};
  for (const [k, v] of m.entries()) {
    Object.defineProperty(obj, kFn(k), {
      value: vFn(v),
      enumerable: true,
      writable: true,
      configurable: true,
    });
  }
  return obj;
}

function mapsToJson(v: p.types.Maps): JsonValue {
  return {
    "str-to-u8": mapToJson(v.strToU8, (k) => k, (n) => n),
    "u8-to-str": mapToJson(v.u8ToStr, (k) => String(k), (s) => s),
    "s8-to-point": mapToJson(v.s8ToPoint, (k) => String(k), pointToJson),
    "char-to-color": mapToJson(v.charToColor, (k) => k, (c) => c),
    "bool-to-s16": mapToJson(v.boolToS16, (k) => String(k), (n) => n),
    "u64-to-s64": mapToJson(v.u64ToS64, (k) => k.toString(), (val) => val.toString()),
    "nested-maps": v.nestedMaps.map((m) =>
      mapToJson(m, (k) => String(k), (n) => n),
    ),
  };
}

function choiceToJson(v: p.types.Choice): JsonValue {
  switch (v.tag) {
    case "unit-case":
      return { case: "unit-case" };
    case "small":
      return { case: "small", value: v.value };
    case "glyph":
      return { case: "glyph", value: v.value };
    case "nested":
      return { case: "nested", value: pointToJson(v.value) };
    case "fixed":
      return { case: "fixed", value: Array.from(v.value) };
  }
}

function outcomeToJson(v: p.types.Outcome): JsonValue {
  return v.tag === "ok" ? { ok: v.value } : { err: v.error };
}

function resultsToJson(v: p.types.Results): JsonValue {
  return {
    both: outcomeToJson(v.both),
    "only-ok": v.onlyOk.tag === "ok" ? { ok: v.onlyOk.value } : { err: null },
    "only-err": v.onlyErr.tag === "ok" ? { ok: null } : { err: v.onlyErr.error },
    bare: v.bare.tag === "ok" ? { ok: null } : { err: null },
  };
}

function permissionsToJson(v: p.types.Permissions): JsonValue {
  return { read: v.read, write: v.write, execute: v.execute };
}

function compositesToJson(v: p.types.Composites): JsonValue {
  return {
    origin: pointToJson(v.origin),
    color: v.color,
    perms: permissionsToJson(v.perms),
    choice: choiceToJson(v.choice),
    outcome: outcomeToJson(v.outcome),
    foreign: sharedMetaToJson(v.foreign),
    "foreign-status": v.foreignStatus,
  };
}

function assertFloatMatches(
  id: string,
  fieldName: string,
  got: number,
  want: JsonValue,
  isF32: boolean,
): void {
  if (Number.isNaN(got)) {
    assert.equal(want, "NaN", `${id}.${fieldName}: expected NaN`);
    return;
  }
  if (!Number.isFinite(got)) {
    const label = got > 0 ? "Infinity" : "-Infinity";
    assert.equal(want, label, `${id}.${fieldName}: expected ${label}`);
    return;
  }
  assert.equal(typeof want, "number", `${id}.${fieldName}: expected finite number`);
  const wantNum = isF32 ? Math.fround(want as number) : (want as number);
  const gotNum = isF32 ? Math.fround(got) : got;
  assert.ok(
    Object.is(gotNum, wantNum),
    `${id}.${fieldName}: float mismatch (got=${String(gotNum)}, want=${String(wantNum)})`,
  );
}

function assertScalarsEq(
  id: string,
  a: p.types.Scalars,
  b: p.types.Scalars,
  strictZeroSign: boolean,
): void {
  const floatEq = (x: number, y: number): boolean =>
    strictZeroSign ? Object.is(x, y) : x === y;
  if (Number.isNaN(a.aF32)) {
    assert.ok(Number.isNaN(b.aF32), `${id}: f32 NaN mismatch`);
  } else {
    assert.ok(
      floatEq(Math.fround(a.aF32), Math.fround(b.aF32)),
      `${id}: f32 mismatch (${String(a.aF32)} vs ${String(b.aF32)})`,
    );
  }
  if (Number.isNaN(a.aF64)) {
    assert.ok(Number.isNaN(b.aF64), `${id}: f64 NaN mismatch`);
  } else {
    assert.ok(
      floatEq(a.aF64, b.aF64),
      `${id}: f64 mismatch (${String(a.aF64)} vs ${String(b.aF64)})`,
    );
  }
  assert.equal(a.aBool, b.aBool, `${id}: bool mismatch`);
  assert.equal(a.aChar, b.aChar, `${id}: char mismatch`);
  assert.equal(a.aString, b.aString, `${id}: string mismatch`);
}

function assertScalarsPlainJson(
  id: string,
  val: p.types.Scalars,
  expected: JsonValue,
): void {
  const obj = asObj(expected, id);
  assert.equal(Object.keys(obj).length, 5, `${id}: expected 5 scalar keys`);
  assertFloatMatches(id, "a-f32", val.aF32, field(obj, "a-f32", id), true);
  assertFloatMatches(id, "a-f64", val.aF64, field(obj, "a-f64", id), false);
  assert.equal(val.aBool, field(obj, "a-bool", id), `${id}.a-bool`);
  assert.equal(val.aChar, field(obj, "a-char", id), `${id}.a-char`);
  assert.equal(val.aString, field(obj, "a-string", id), `${id}.a-string`);
}

// --- Encode-vector `plain_json` -> plain TS builders (preserving intentionally invalid inputs) ---

function integersFromJson(raw: JsonValue, id: string): p.types.Integers {
  const o = asObj(raw, id);
  return {
    aU8: field(o, "a-u8", id) as number,
    aU16: field(o, "a-u16", id) as number,
    aU32: field(o, "a-u32", id) as number,
    aU64: BigInt(field(o, "a-u64", id) as string),
    aS8: field(o, "a-s8", id) as number,
    aS16: field(o, "a-s16", id) as number,
    aS32: field(o, "a-s32", id) as number,
    aS64: BigInt(field(o, "a-s64", id) as string),
  };
}

function pointFromJson(raw: JsonValue, id: string): p.types.Point {
  const o = asObj(raw, id);
  return { x: field(o, "x", id) as number, y: field(o, "y", id) as number };
}

function collectionsFromJson(raw: JsonValue, id: string): p.types.Collections {
  const o = asObj(raw, id);
  return {
    rawBytes: Uint8Array.from(asArr(field(o, "raw-bytes", id), id) as number[]),
    fixedBytes: Uint8Array.from(asArr(field(o, "fixed-bytes", id), id) as number[]),
    u16Items: asArr(field(o, "u16-items", id), id) as number[],
    fixedU16s: asArr(field(o, "fixed-u16s", id), id) as number[],
    fixedChars: asArr(field(o, "fixed-chars", id), id) as string[],
    pointItems: asArr(field(o, "point-items", id), id).map((x) => pointFromJson(x, id)),
    nestedLists: asArr(field(o, "nested-lists", id), id).map(
      (inner) => asArr(inner, id) as number[],
    ),
    nestedFixed: asArr(field(o, "nested-fixed", id), id).map(
      (inner) => asArr(inner, id) as number[],
    ),
    optItems: asArr(field(o, "opt-items", id), id).map((item) =>
      item === null ? NONE : some(item as number),
    ),
    resultItems: asArr(field(o, "result-items", id), id).map((item) => {
      const r = asObj(item, id);
      if (r["ok"] !== undefined) {
        return { tag: "ok" as const, value: r["ok"] as number };
      }
      return { tag: "err" as const, error: r["err"] as p.types.Color };
    }),
  };
}

function mapsFromJson(raw: JsonValue, id: string): p.types.Maps {
  const o = asObj(raw, id);
  const strToU8 = new Map<string, number>();
  for (const [k, v] of Object.entries(asObj(field(o, "str-to-u8", id), id))) {
    strToU8.set(k, v as number);
  }
  const u8ToStr = new Map<number, string>();
  for (const [k, v] of Object.entries(asObj(field(o, "u8-to-str", id), id))) {
    u8ToStr.set(Number(k), v as string);
  }
  const s8ToPoint = new Map<number, p.types.Point>();
  for (const [k, v] of Object.entries(asObj(field(o, "s8-to-point", id), id))) {
    s8ToPoint.set(Number(k), pointFromJson(v, id));
  }
  const charToColor = new Map<string, p.types.Color>();
  for (const [k, v] of Object.entries(asObj(field(o, "char-to-color", id), id))) {
    charToColor.set(k, v as p.types.Color);
  }
  const boolToS16 = new Map<boolean, number>();
  for (const [k, v] of Object.entries(asObj(field(o, "bool-to-s16", id), id))) {
    boolToS16.set(k === "true", v as number);
  }
  const u64ToS64 = new Map<bigint, bigint>();
  for (const [k, v] of Object.entries(asObj(field(o, "u64-to-s64", id), id))) {
    u64ToS64.set(BigInt(k), BigInt(v as string));
  }
  const nestedMaps = asArr(field(o, "nested-maps", id), id).map((entry) => {
    const m = new Map<number, number>();
    for (const [k, v] of Object.entries(asObj(entry, id))) {
      m.set(Number(k), v as number);
    }
    return m;
  });
  return { strToU8, u8ToStr, s8ToPoint, charToColor, boolToS16, u64ToS64, nestedMaps };
}

function compositesFromJson(raw: JsonValue, id: string): p.types.Composites {
  const o = asObj(raw, id);
  const permsObj = asObj(field(o, "perms", id), id);
  const choiceObj = asObj(field(o, "choice", id), id);
  const outcomeObj = asObj(field(o, "outcome", id), id);
  const foreignObj = asObj(field(o, "foreign", id), id);

  let choice: p.types.Choice;
  const tag = field(choiceObj, "case", id);
  if (tag === "unit-case") {
    choice = { tag: "unit-case" };
  } else if (tag === "small") {
    choice = { tag: "small", value: field(choiceObj, "value", id) as number };
  } else if (tag === "glyph") {
    choice = { tag: "glyph", value: field(choiceObj, "value", id) as string };
  } else if (tag === "nested") {
    choice = { tag: "nested", value: pointFromJson(field(choiceObj, "value", id), id) };
  } else {
    choice = { tag: "fixed", value: asArr(field(choiceObj, "value", id), id) as number[] };
  }

  const outcome: p.types.Outcome =
    outcomeObj["ok"] !== undefined
      ? { tag: "ok", value: outcomeObj["ok"] as number }
      : { tag: "err", error: outcomeObj["err"] as string };

  return {
    origin: pointFromJson(field(o, "origin", id), id),
    color: field(o, "color", id) as unknown as p.types.Color,
    perms: {
      read: field(permsObj, "read", id) as boolean,
      write: field(permsObj, "write", id) as boolean,
      execute: field(permsObj, "execute", id) as boolean,
    },
    choice,
    outcome,
    foreign: {
      id: BigInt(field(foreignObj, "id", id) as string),
      tag: field(foreignObj, "tag", id) as number,
      status: field(foreignObj, "status", id) as pDep.shared.SharedStatus,
    },
    foreignStatus: field(o, "foreign-status", id) as pDep.shared.SharedStatus,
  };
}

// --- Generic decode + round-trip runner ---

interface DecodeRunner {
  runDecode(
    v: ConformanceVector,
    binErrors: Set<string>,
    jsonErrors: Set<string>,
  ): void;
}

function makeRunner<Desc extends DescMessage, Plain>(opts: {
  readonly schema: Desc;
  readonly fromProto: (msg: MessageShape<Desc>) => Plain;
  readonly toProto: (val: Plain) => MessageShape<Desc>;
  readonly checkPlainJson: (id: string, val: Plain, want: JsonValue) => void;
  readonly assertEqual: (id: string, a: Plain, b: Plain, strictZeroSign: boolean) => void;
}): DecodeRunner {
  return {
    runDecode(v, binErrors, jsonErrors) {
      if (v.proto_binary_base64 === null) {
        throw new Error(`${v.id}: missing proto_binary_base64`);
      }
      if (v.proto3_json === null) {
        throw new Error(`${v.id}: missing proto3_json`);
      }

      const wireFromBin = fromBinary(opts.schema, decodeBase64(v.proto_binary_base64));

      if (v.expected === "accept") {
        // 1. Binary decode -> plain -> check -> encode -> binary -> decode -> compare
        const plainFromBin = opts.fromProto(wireFromBin);
        opts.checkPlainJson(v.id, plainFromBin, v.plain_json);
        const reBytes = toBinary(opts.schema, opts.toProto(plainFromBin));
        const roundTripBin = opts.fromProto(fromBinary(opts.schema, reBytes));
        opts.assertEqual(v.id, plainFromBin, roundTripBin, true);

        // 2. Proto3-JSON decode -> plain -> check -> encode -> proto3-JSON -> decode -> compare
        const wireFromJson = fromJsonString(opts.schema, v.proto3_json);
        const plainFromJson = opts.fromProto(wireFromJson);
        opts.checkPlainJson(v.id, plainFromJson, v.plain_json);
        opts.assertEqual(v.id, plainFromBin, plainFromJson, true);

        const reJson = toJsonString(opts.schema, opts.toProto(plainFromJson));
        const roundTripJson = opts.fromProto(fromJsonString(opts.schema, reJson));
        // ECMA-262 JSON.stringify(-0) emits "0", so JSON re-serialization normalizes -0 to +0.
        opts.assertEqual(v.id, plainFromJson, roundTripJson, false);
        return;
      }

      // Reject path
      assert.equal(v.plain_json, null, `${v.id}: reject vector must have null plain_json`);
      if (v.error_kind === null) {
        throw new Error(`${v.id}: missing error_kind`);
      }
      if (v.error_target === null) {
        throw new Error(`${v.id}: missing error_target`);
      }

      // 1. Binary decode must throw WitforgeGlueError with exact kind and target
      let binErr: unknown = undefined;
      try {
        opts.fromProto(wireFromBin);
      } catch (err) {
        binErr = err;
      }
      if (!(binErr instanceof g.WitforgeGlueError)) {
        throw new Error(`${v.id}: expected binary WitforgeGlueError, got ${String(binErr)}`);
      }
      assert.equal(binErr.kind, v.error_kind, `${v.id}: binary error kind mismatch`);
      assert.equal(binErr.target, v.error_target, `${v.id}: binary error target mismatch`);
      binErrors.add(binErr.kind);

      // 2. Proto3-JSON decode must reach `fromProto` and reject with the exact `WitforgeGlueError`
      //    kind and target (do not mask unexpected `fromJsonString` parse failures).
      let parsedFromJson: MessageShape<Desc>;
      try {
        parsedFromJson = fromJsonString(opts.schema, v.proto3_json);
      } catch (err) {
        throw new Error(
          `${v.id}: unexpected fromJsonString failure (expected WitforgeGlueError ${v.error_kind} on ${v.error_target}): ${String(err)}`,
        );
      }
      let jsonErr: unknown = undefined;
      try {
        opts.fromProto(parsedFromJson);
      } catch (err) {
        jsonErr = err;
      }
      if (!(jsonErr instanceof g.WitforgeGlueError)) {
        throw new Error(
          `${v.id}: expected proto3-JSON WitforgeGlueError, got ${String(jsonErr)}`,
        );
      }
      assert.equal(jsonErr.kind, v.error_kind, `${v.id}: JSON error kind mismatch`);
      assert.equal(jsonErr.target, v.error_target, `${v.id}: JSON error target mismatch`);
      jsonErrors.add(jsonErr.kind);
    },
  };
}

function makeJsonRunner<Desc extends DescMessage, Plain>(
  schema: Desc,
  fromProto: (msg: MessageShape<Desc>) => Plain,
  toProto: (val: Plain) => MessageShape<Desc>,
  toJson: (val: Plain) => JsonValue,
): DecodeRunner {
  return makeRunner({
    schema,
    fromProto,
    toProto,
    checkPlainJson: (id, val, want) => {
      assert.deepStrictEqual(toJson(val), want, `${id}: plain_json mismatch`);
    },
    assertEqual: (id, a, b, _strictZeroSign) => {
      assert.deepStrictEqual(toJson(a), toJson(b), `${id}: round-trip mismatch`);
    },
  });
}

function runEncodeReject(v: ConformanceVector, encodeErrors: Set<string>): void {
  assert.equal(v.expected, "reject", `${v.id}: unexpected encode accept vector`);
  if (v.error_kind === null) {
    throw new Error(`${v.id}: missing error_kind`);
  }
  if (v.error_target === null) {
    throw new Error(`${v.id}: missing error_target`);
  }

  let caught: unknown = undefined;
  try {
    switch (v.proto_message) {
      case ".witforge.conformance.v0.TypesIntegers": {
        g.v0.types.integers.toProto(integersFromJson(v.plain_json, v.id));
        break;
      }
      case ".witforge.conformance.v0.TypesScalars": {
        const o = asObj(v.plain_json, v.id);
        if (o["a-char"] !== undefined) {
          g.v0.types.scalars.toProto({
            aF32: field(o, "a-f32", v.id) as number,
            aF64: field(o, "a-f64", v.id) as number,
            aBool: field(o, "a-bool", v.id) as boolean,
            aChar: field(o, "a-char", v.id) as string,
            aString: field(o, "a-string", v.id) as string,
          });
        } else {
          const cp = field(o, "a-char-codepoint", v.id) as number;
          if (cp >= 0xd800 && cp <= 0xdfff) {
            // UTF-16 lone surrogate IS representable in a JS string: exercise `scalars.toProto` directly!
            g.v0.types.scalars.toProto({
              aF32: field(o, "a-f32", v.id) as number,
              aF64: field(o, "a-f64", v.id) as number,
              aBool: field(o, "a-bool", v.id) as boolean,
              aChar: String.fromCharCode(cp),
              aString: field(o, "a-string", v.id) as string,
            });
          } else {
            // Out-of-UTF-16 code points (-1, 0x110000) cannot be represented as JS strings;
            // verify the scalar validator (`decodeChar`) rejects them with the exact target.
            gHelpers.decodeChar(cp, v.error_target);
          }
        }
        break;
      }
      case ".witforge.conformance.v0.TypesCollections": {
        g.v0.types.collections.toProto(collectionsFromJson(v.plain_json, v.id));
        break;
      }
      case ".witforge.conformance.v0.TypesMaps": {
        g.v0.types.maps.toProto(mapsFromJson(v.plain_json, v.id));
        break;
      }
      case ".witforge.conformance.v0.TypesComposites": {
        g.v0.types.composites.toProto(compositesFromJson(v.plain_json, v.id));
        break;
      }
      default:
        throw new Error(`${v.id}: unhandled encode proto_message ${v.proto_message}`);
    }
  } catch (err) {
    caught = err;
  }

  if (!(caught instanceof g.WitforgeGlueError)) {
    throw new Error(`${v.id}: expected encode WitforgeGlueError, got ${String(caught)}`);
  }
  assert.equal(caught.kind, v.error_kind, `${v.id}: encode error kind mismatch`);
  assert.equal(caught.target, v.error_target, `${v.id}: encode error target mismatch`);
  encodeErrors.add(caught.kind);
}

function main(): void {
  const bundleDir = process.argv[2];
  if (!bundleDir) {
    throw new Error("usage: node run.js <bundle-dir>");
  }

  // Verify METHOD enum constants match generated protobuf-es enum values.
  assert.equal(
    g.v0.api.functions.echoIntegers.METHOD,
    protoRoot.ApiMethod.ECHO_INTEGERS,
  );
  assert.equal(
    g.v0.api.functions.processForeign.METHOD,
    protoRoot.ApiMethod.PROCESS_FOREIGN,
  );
  assert.equal(
    g.v0.api.functions.notify.METHOD,
    protoRoot.ApiMethod.NOTIFY,
  );

  const manifestPath = path.join(bundleDir, "vectors.json");
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8")) as ConformanceManifest;
  assert.equal(manifest.conformance_version, 1, "unexpected conformance_version");

  const runners: Record<string, DecodeRunner> = {
    ".witforge.conformance.v0.TypesIntegers": makeJsonRunner(
      g.v0.types.integers.schema,
      g.v0.types.integers.fromProto,
      g.v0.types.integers.toProto,
      integersToJson,
    ),
    ".witforge.conformance.v0.TypesScalars": makeRunner({
      schema: g.v0.types.scalars.schema,
      fromProto: g.v0.types.scalars.fromProto,
      toProto: g.v0.types.scalars.toProto,
      checkPlainJson: assertScalarsPlainJson,
      assertEqual: assertScalarsEq,
    }),
    ".witforge.conformance.v0.TypesOptionals": makeJsonRunner(
      g.v0.types.optionals.schema,
      g.v0.types.optionals.fromProto,
      g.v0.types.optionals.toProto,
      optionalsToJson,
    ),
    ".witforge.conformance.v0.TypesCollections": makeJsonRunner(
      g.v0.types.collections.schema,
      g.v0.types.collections.fromProto,
      g.v0.types.collections.toProto,
      collectionsToJson,
    ),
    ".witforge.conformance.v0.TypesMaps": makeJsonRunner(
      g.v0.types.maps.schema,
      g.v0.types.maps.fromProto,
      g.v0.types.maps.toProto,
      mapsToJson,
    ),
    ".witforge.conformance.v0.TypesChoice": makeJsonRunner(
      g.v0.types.choice.schema,
      g.v0.types.choice.fromProto,
      g.v0.types.choice.toProto,
      choiceToJson,
    ),
    ".witforge.conformance.v0.TypesResults": makeJsonRunner(
      g.v0.types.results.schema,
      g.v0.types.results.fromProto,
      g.v0.types.results.toProto,
      resultsToJson,
    ),
    ".witforge.conformance.v0.TypesPermissions": makeJsonRunner(
      g.v0.types.permissions.schema,
      g.v0.types.permissions.fromProto,
      g.v0.types.permissions.toProto,
      permissionsToJson,
    ),
    ".witforge.conformance.v0.TypesComposites": makeJsonRunner(
      g.v0.types.composites.schema,
      g.v0.types.composites.fromProto,
      g.v0.types.composites.toProto,
      compositesToJson,
    ),
    ".witforge.conformance_dep.v0.SharedSharedMeta": makeJsonRunner(
      g.witforge_deps.witforge_conformance_dep.shared.sharedMeta.schema,
      g.witforge_deps.witforge_conformance_dep.shared.sharedMeta.fromProto,
      g.witforge_deps.witforge_conformance_dep.shared.sharedMeta.toProto,
      sharedMetaToJson,
    ),
    ".witforge.conformance.v0.ApiEchoIntegersRequest": makeJsonRunner(
      g.v0.api.functions.echoIntegers.requestSchema,
      g.v0.api.functions.echoIntegers.fromRequest,
      g.v0.api.functions.echoIntegers.toRequest,
      (input) => ({ input: integersToJson(input) }),
    ),
    ".witforge.conformance.v0.ApiEchoIntegersResponse": makeJsonRunner(
      g.v0.api.functions.echoIntegers.responseSchema,
      g.v0.api.functions.echoIntegers.fromResponse,
      g.v0.api.functions.echoIntegers.toResponse,
      (res) => ({ result: integersToJson(res) }),
    ),
    ".witforge.conformance.v0.ApiProcessForeignRequest": makeJsonRunner(
      g.v0.api.functions.processForeign.requestSchema,
      g.v0.api.functions.processForeign.fromRequest,
      ([meta, status, code]) => g.v0.api.functions.processForeign.toRequest(meta, status, code),
      ([meta, status, code]) => ({
        meta: sharedMetaToJson(meta),
        status,
        code,
      }),
    ),
    ".witforge.conformance.v0.ApiProcessForeignResponse": makeJsonRunner(
      g.v0.api.functions.processForeign.responseSchema,
      g.v0.api.functions.processForeign.fromResponse,
      g.v0.api.functions.processForeign.toResponse,
      (res) => ({
        result: res.tag === "ok" ? { ok: pointToJson(res.value) } : { err: res.error },
      }),
    ),
    ".witforge.conformance.v0.ApiNotifyRequest": makeJsonRunner(
      g.v0.api.functions.notify.requestSchema,
      g.v0.api.functions.notify.fromRequest,
      ([label, priority]) => g.v0.api.functions.notify.toRequest(label, priority),
      ([label, priority]) => ({ label, priority }),
    ),
    ".witforge.conformance.v0.ApiNotifyResponse": makeJsonRunner(
      g.v0.api.functions.notify.responseSchema,
      g.v0.api.functions.notify.fromResponse,
      () => g.v0.api.functions.notify.toResponse(),
      () => ({}),
    ),
  };

  let decodeAcceptCount = 0;
  let decodeRejectCount = 0;
  let encodeRejectCount = 0;
  const binErrors = new Set<string>();
  const jsonErrors = new Set<string>();
  const encodeErrors = new Set<string>();

  for (const v of manifest.vectors) {
    if (v.direction === "encode") {
      runEncodeReject(v, encodeErrors);
      encodeRejectCount += 1;
      continue;
    }
    const runner = runners[v.proto_message];
    if (!runner) {
      throw new Error(`${v.id}: missing runner for proto_message ${v.proto_message}`);
    }
    runner.runDecode(v, binErrors, jsonErrors);
    if (v.expected === "accept") {
      decodeAcceptCount += 1;
    } else {
      decodeRejectCount += 1;
    }
  }

  const total = decodeAcceptCount + decodeRejectCount + encodeRejectCount;
  assert.equal(total, manifest.vectors.length, "vector count mismatch");

  for (const kind of manifest.error_kinds) {
    assert.ok(binErrors.has(kind), `binary decode did not exercise error kind ${kind}`);
    assert.ok(jsonErrors.has(kind), `proto3-JSON decode did not exercise error kind ${kind}`);
  }
  for (const kind of ["IntegerOutOfRange", "InvalidChar", "InvalidLength", "UnknownEnumValue"]) {
    assert.ok(encodeErrors.has(kind), `encode bounds did not exercise error kind ${kind}`);
  }

  console.log(
    `TypeScript conformance passed: ${String(total)} vectors ` +
      `(${String(decodeAcceptCount)} decode accept, ${String(decodeRejectCount)} decode reject, ` +
      `${String(encodeRejectCount)} encode reject; all ${String(binErrors.size)} GlueError kinds verified)`,
  );
}

main();
