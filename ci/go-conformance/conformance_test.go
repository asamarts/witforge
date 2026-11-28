//go:build witforge_conformance

// End-to-end Go checked protobuf glue conformance suite (W13-007).
//
// Executes every binary, proto3-JSON, and encode-bounds vector from the W11-034
// conformance bundle against generated plain Go types (`witforge go`), `protoc-gen-go`
// `*.pb.go` messages, and `witforge go-glue` packages.
package conformance_test

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"math/big"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"testing"
	"unicode/utf8"

	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"

	g_glueerr "example.com/witforge/go-conformance/glue/glueerr"
	g_helpers "example.com/witforge/go-conformance/glue/helpers"
	g_api "example.com/witforge/go-conformance/glue/v0/api"
	g_types "example.com/witforge/go-conformance/glue/v0/types"
	g_dep_shared "example.com/witforge/go-conformance/glue/witforge_deps/witforge_conformance_dep/shared"
	p_types "example.com/witforge/go-conformance/plain/v0_1/types"
	p_dep_shared "example.com/witforge/go-conformance/plain/witforge_deps/witforge_conformance_dep/shared"
	p_wt "example.com/witforge/go-conformance/plain/witforge_types"
	pb_root "example.com/witforge/go-conformance/proto/witforge/conformance/v0"
	pb_dep "example.com/witforge/go-conformance/proto/witforge/conformance_dep/v0"
)

type vectorManifest struct {
	ConformanceVersion int      `json:"conformance_version"`
	ErrorKinds         []string `json:"error_kinds"`
	Vectors            []vector `json:"vectors"`
}

type vector struct {
	ID                string          `json:"id"`
	Direction         string          `json:"direction"`
	Expected          string          `json:"expected"`
	ProtoMessage      string          `json:"proto_message"`
	ProtoBinaryBase64 *string         `json:"proto_binary_base64"`
	Proto3JSON        *string         `json:"proto3_json"`
	PlainJSON         json.RawMessage `json:"plain_json"`
	ErrorKind         *string         `json:"error_kind"`
	ErrorTarget       *string         `json:"error_target"`
}

func decodeJSONValue(t *testing.T, raw json.RawMessage) any {
	t.Helper()
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	var v any
	if err := dec.Decode(&v); err != nil {
		t.Fatalf("failed to decode JSON %s: %v", string(raw), err)
	}
	return normalizeJSONNumbers(v)
}

func normalizeJSONNumbers(v any) any {
	switch x := v.(type) {
	case json.Number:
		if i, err := x.Int64(); err == nil {
			return i
		}
		if f, err := x.Float64(); err == nil {
			return f
		}
		return x.String()
	case map[string]any:
		out := make(map[string]any, len(x))
		for k, val := range x {
			out[k] = normalizeJSONNumbers(val)
		}
		return out
	case []any:
		out := make([]any, len(x))
		for i, val := range x {
			out[i] = normalizeJSONNumbers(val)
		}
		return out
	default:
		return v
	}
}

func assertGlueErrorDetails(t *testing.T, ctx string, err error, wantKind, wantTarget string) *g_glueerr.GlueError {
	t.Helper()
	if err == nil {
		t.Fatalf("%s: expected *GlueError(%s, %s), got nil", ctx, wantKind, wantTarget)
	}
	var ge *g_glueerr.GlueError
	if !errors.As(err, &ge) {
		t.Fatalf("%s: expected *GlueError, got %T (%v)", ctx, err, err)
	}
	if string(ge.Kind) != wantKind {
		t.Fatalf("%s: error kind mismatch: got %q, want %q (err=%v)", ctx, ge.Kind, wantKind, ge)
	}
	if ge.Error() == "" {
		t.Fatalf("%s: empty Error() string", ctx)
	}
	switch wantKind {
	case "MissingField":
		if ge.Field != wantTarget || ge.Enumeration != "" || ge.Message != "" ||
			ge.Value != 0 || ge.CodePoint != 0 || ge.Expected != 0 || ge.Actual != 0 {
			t.Fatalf("%s: unexpected MissingField details: %+v (want field=%q)", ctx, ge, wantTarget)
		}
	case "IntegerOutOfRange":
		if ge.Field != wantTarget || ge.Enumeration != "" || ge.Message != "" ||
			ge.Value == 0 || ge.CodePoint != 0 || ge.Expected != 0 || ge.Actual != 0 {
			t.Fatalf("%s: unexpected IntegerOutOfRange details: %+v (want field=%q, non-zero value)", ctx, ge, wantTarget)
		}
	case "UnknownEnumValue":
		if ge.Enumeration != wantTarget || ge.Field != "" || ge.Message != "" ||
			ge.CodePoint != 0 || ge.Expected != 0 || ge.Actual != 0 {
			t.Fatalf("%s: unexpected UnknownEnumValue details: %+v (want enum=%q)", ctx, ge, wantTarget)
		}
	case "UnknownCase", "InvalidState":
		if ge.Message != wantTarget || ge.Field != "" || ge.Enumeration != "" ||
			ge.Value != 0 || ge.CodePoint != 0 || ge.Expected != 0 || ge.Actual != 0 {
			t.Fatalf("%s: unexpected %s details: %+v (want message=%q)", ctx, wantKind, ge, wantTarget)
		}
	case "InvalidChar":
		invalidScalar := ge.CodePoint > 0x10FFFF || (ge.CodePoint >= 0xD800 && ge.CodePoint <= 0xDFFF)
		if ge.Field != wantTarget || ge.Enumeration != "" || ge.Message != "" ||
			ge.Value != 0 || !invalidScalar || ge.Expected != 0 || ge.Actual != 0 {
			t.Fatalf("%s: unexpected InvalidChar details: %+v (want field=%q, invalid scalar code point)", ctx, ge, wantTarget)
		}
	case "InvalidLength":
		if ge.Field != wantTarget || ge.Enumeration != "" || ge.Message != "" ||
			ge.Value != 0 || ge.CodePoint != 0 || ge.Expected <= 0 || ge.Actual < 0 || ge.Expected == ge.Actual {
			t.Fatalf("%s: unexpected InvalidLength details: %+v (want field=%q)", ctx, ge, wantTarget)
		}
	default:
		t.Fatalf("%s: unexpected error kind %q", ctx, wantKind)
	}
	return ge
}

// --- Plain Go -> canonical plain_json serializers ---

func bytesToJSON(b []byte) []any {
	out := make([]any, len(b))
	for i, v := range b {
		out[i] = int64(v)
	}
	return out
}

func pointToJSON(v p_types.Point) map[string]any {
	return map[string]any{
		"x": int64(v.X),
		"y": int64(v.Y),
	}
}

func sharedMetaToJSON(v p_dep_shared.SharedMeta) map[string]any {
	return map[string]any{
		"id":     strconv.FormatUint(v.Id, 10),
		"tag":    int64(v.Tag),
		"status": v.Status.String(),
	}
}

func integersToJSON(v p_types.Integers) map[string]any {
	return map[string]any{
		"a-u8":  int64(v.AU8),
		"a-u16": int64(v.AU16),
		"a-u32": int64(v.AU32),
		"a-u64": strconv.FormatUint(v.AU64, 10),
		"a-s8":  int64(v.AS8),
		"a-s16": int64(v.AS16),
		"a-s32": int64(v.AS32),
		"a-s64": strconv.FormatInt(v.AS64, 10),
	}
}

func optionalsToJSON(v p_types.Optionals) map[string]any {
	var optU8 any
	if v.OptU8 != nil {
		optU8 = int64(*v.OptU8)
	}
	var optS8 any
	if v.OptS8 != nil {
		optS8 = int64(*v.OptS8)
	}
	var optChar any
	if v.OptChar != nil {
		optChar = string(*v.OptChar)
	}
	var optString any
	if v.OptString != nil {
		optString = *v.OptString
	}
	var optBytes any
	if v.OptBytes != nil {
		optBytes = bytesToJSON(*v.OptBytes)
	}
	var optFixedBytes any
	if v.OptFixedBytes != nil {
		optFixedBytes = bytesToJSON((*v.OptFixedBytes)[:])
	}
	var optList any
	if v.OptList != nil {
		xs := make([]any, len(*v.OptList))
		for i, item := range *v.OptList {
			xs[i] = int64(item)
		}
		optList = xs
	}
	var optEnum any
	if v.OptEnum != nil {
		optEnum = v.OptEnum.String()
	}
	var optPoint any
	if v.OptPoint != nil {
		optPoint = pointToJSON(*v.OptPoint)
	}
	var nestedOpt any
	if v.NestedOpt.Some {
		if v.NestedOpt.Value == nil {
			nestedOpt = map[string]any{"none": nil}
		} else {
			nestedOpt = map[string]any{"some": int64(*v.NestedOpt.Value)}
		}
	}
	var optMap any
	if v.OptMap != nil {
		m := make(map[string]any, len(*v.OptMap))
		for k, val := range *v.OptMap {
			m[strconv.FormatUint(uint64(k), 10)] = int64(val)
		}
		optMap = m
	}
	return map[string]any{
		"opt-u8":          optU8,
		"opt-s8":          optS8,
		"opt-char":        optChar,
		"opt-string":      optString,
		"opt-bytes":       optBytes,
		"opt-fixed-bytes": optFixedBytes,
		"opt-list":        optList,
		"opt-enum":        optEnum,
		"opt-point":       optPoint,
		"nested-opt":      nestedOpt,
		"opt-map":         optMap,
	}
}

func collectionsToJSON(v p_types.Collections) map[string]any {
	u16Items := make([]any, len(v.U16Items))
	for i, x := range v.U16Items {
		u16Items[i] = int64(x)
	}
	fixedU16s := make([]any, len(v.FixedU16s))
	for i, x := range v.FixedU16s {
		fixedU16s[i] = int64(x)
	}
	fixedChars := make([]any, len(v.FixedChars))
	for i, x := range v.FixedChars {
		fixedChars[i] = string(x)
	}
	pointItems := make([]any, len(v.PointItems))
	for i, x := range v.PointItems {
		pointItems[i] = pointToJSON(x)
	}
	nestedLists := make([]any, len(v.NestedLists))
	for i, inner := range v.NestedLists {
		row := make([]any, len(inner))
		for j, x := range inner {
			row[j] = int64(x)
		}
		nestedLists[i] = row
	}
	nestedFixed := make([]any, len(v.NestedFixed))
	for i, inner := range v.NestedFixed {
		row := make([]any, len(inner))
		for j, x := range inner {
			row[j] = int64(x)
		}
		nestedFixed[i] = row
	}
	optItems := make([]any, len(v.OptItems))
	for i, item := range v.OptItems {
		if item == nil {
			optItems[i] = nil
		} else {
			optItems[i] = int64(*item)
		}
	}
	resultItems := make([]any, len(v.ResultItems))
	for i, item := range v.ResultItems {
		if item.IsOk {
			resultItems[i] = map[string]any{"ok": int64(item.Ok)}
		} else {
			resultItems[i] = map[string]any{"err": item.Err.String()}
		}
	}
	return map[string]any{
		"raw-bytes":    bytesToJSON(v.RawBytes),
		"fixed-bytes":  bytesToJSON(v.FixedBytes[:]),
		"u16-items":    u16Items,
		"fixed-u16s":   fixedU16s,
		"fixed-chars":  fixedChars,
		"point-items":  pointItems,
		"nested-lists": nestedLists,
		"nested-fixed": nestedFixed,
		"opt-items":    optItems,
		"result-items": resultItems,
	}
}

func mapsToJSON(v p_types.Maps) map[string]any {
	strToU8 := make(map[string]any, len(v.StrToU8))
	for k, val := range v.StrToU8 {
		strToU8[k] = int64(val)
	}
	u8ToStr := make(map[string]any, len(v.U8ToStr))
	for k, val := range v.U8ToStr {
		u8ToStr[strconv.FormatUint(uint64(k), 10)] = val
	}
	s8ToPoint := make(map[string]any, len(v.S8ToPoint))
	for k, val := range v.S8ToPoint {
		s8ToPoint[strconv.FormatInt(int64(k), 10)] = pointToJSON(val)
	}
	charToColor := make(map[string]any, len(v.CharToColor))
	for k, val := range v.CharToColor {
		charToColor[string(k)] = val.String()
	}
	boolToS16 := make(map[string]any, len(v.BoolToS16))
	for k, val := range v.BoolToS16 {
		if k {
			boolToS16["true"] = int64(val)
		} else {
			boolToS16["false"] = int64(val)
		}
	}
	u64ToS64 := make(map[string]any, len(v.U64ToS64))
	for k, val := range v.U64ToS64 {
		u64ToS64[strconv.FormatUint(k, 10)] = strconv.FormatInt(val, 10)
	}
	nestedMaps := make([]any, len(v.NestedMaps))
	for i, m := range v.NestedMaps {
		out := make(map[string]any, len(m))
		for k, val := range m {
			out[strconv.FormatUint(uint64(k), 10)] = int64(val)
		}
		nestedMaps[i] = out
	}
	return map[string]any{
		"str-to-u8":     strToU8,
		"u8-to-str":     u8ToStr,
		"s8-to-point":   s8ToPoint,
		"char-to-color": charToColor,
		"bool-to-s16":   boolToS16,
		"u64-to-s64":    u64ToS64,
		"nested-maps":   nestedMaps,
	}
}

func choiceToJSON(v p_types.Choice) map[string]any {
	switch c := v.(type) {
	case p_types.ChoiceUnitCase:
		return map[string]any{"case": "unit-case"}
	case p_types.ChoiceSmall:
		return map[string]any{"case": "small", "value": int64(c.Value)}
	case p_types.ChoiceGlyph:
		return map[string]any{"case": "glyph", "value": string(c.Value)}
	case p_types.ChoiceNested:
		return map[string]any{"case": "nested", "value": pointToJSON(c.Value)}
	case p_types.ChoiceFixed:
		xs := make([]any, len(c.Value))
		for i, x := range c.Value {
			xs[i] = int64(x)
		}
		return map[string]any{"case": "fixed", "value": xs}
	default:
		panic(fmt.Sprintf("unexpected choice %T", v))
	}
}

func outcomeToJSON(v p_types.Outcome) map[string]any {
	if v.IsOk {
		return map[string]any{"ok": int64(v.Ok)}
	}
	return map[string]any{"err": v.Err}
}

func resultsToJSON(v p_types.Results) map[string]any {
	onlyOk := map[string]any{"err": nil}
	if v.OnlyOk.IsOk {
		onlyOk = map[string]any{"ok": int64(v.OnlyOk.Ok)}
	}
	onlyErr := map[string]any{"ok": nil}
	if !v.OnlyErr.IsOk {
		onlyErr = map[string]any{"err": v.OnlyErr.Err.String()}
	}
	bare := map[string]any{"err": nil}
	if v.Bare.IsOk {
		bare = map[string]any{"ok": nil}
	}
	return map[string]any{
		"both":     outcomeToJSON(v.Both),
		"only-ok":  onlyOk,
		"only-err": onlyErr,
		"bare":     bare,
	}
}

func permissionsToJSON(v p_types.Permissions) map[string]any {
	return map[string]any{
		"read":    v.Read,
		"write":   v.Write,
		"execute": v.Execute,
	}
}

func compositesToJSON(v p_types.Composites) map[string]any {
	return map[string]any{
		"origin":         pointToJSON(v.Origin),
		"color":          v.Color.String(),
		"perms":          permissionsToJSON(v.Perms),
		"choice":         choiceToJSON(v.Choice),
		"outcome":        outcomeToJSON(v.Outcome),
		"foreign":        sharedMetaToJSON(v.Foreign),
		"foreign-status": v.ForeignStatus.String(),
	}
}

func float32MatchesJSON(t *testing.T, vecID, field string, got float32, rawVal string) {
	t.Helper()
	valRaw := strings.TrimSpace(rawVal)
	switch valRaw {
	case `"NaN"`:
		if !math.IsNaN(float64(got)) {
			t.Fatalf("%s.%s: got %v, want NaN", vecID, field, got)
		}
		return
	case `"Infinity"`:
		if !math.IsInf(float64(got), 1) {
			t.Fatalf("%s.%s: got %v, want +Inf", vecID, field, got)
		}
		return
	case `"-Infinity"`:
		if !math.IsInf(float64(got), -1) {
			t.Fatalf("%s.%s: got %v, want -Inf", vecID, field, got)
		}
		return
	}
	parsed, err := strconv.ParseFloat(valRaw, 32)
	if err != nil {
		t.Fatalf("%s.%s: parse float32 %q: %v", vecID, field, valRaw, err)
	}
	want := float32(parsed)
	if got != want {
		t.Fatalf("%s.%s: float32 mismatch: got %v, want %v", vecID, field, got, want)
	}
	if got == 0 && math.Signbit(float64(got)) != math.Signbit(float64(want)) {
		t.Fatalf("%s.%s: float32 zero sign mismatch: got signbit=%v, want signbit=%v",
			vecID, field, math.Signbit(float64(got)), math.Signbit(float64(want)))
	}
}

func float64MatchesJSON(t *testing.T, vecID, field string, got float64, rawVal string) {
	t.Helper()
	valRaw := strings.TrimSpace(rawVal)
	switch valRaw {
	case `"NaN"`:
		if !math.IsNaN(got) {
			t.Fatalf("%s.%s: got %v, want NaN", vecID, field, got)
		}
		return
	case `"Infinity"`:
		if !math.IsInf(got, 1) {
			t.Fatalf("%s.%s: got %v, want +Inf", vecID, field, got)
		}
		return
	case `"-Infinity"`:
		if !math.IsInf(got, -1) {
			t.Fatalf("%s.%s: got %v, want -Inf", vecID, field, got)
		}
		return
	}
	want, err := strconv.ParseFloat(valRaw, 64)
	if err != nil {
		t.Fatalf("%s.%s: parse float64 %q: %v", vecID, field, valRaw, err)
	}
	if got != want {
		t.Fatalf("%s.%s: float64 mismatch: got %v, want %v", vecID, field, got, want)
	}
	if got == 0 && math.Signbit(got) != math.Signbit(want) {
		t.Fatalf("%s.%s: float64 zero sign mismatch: got signbit=%v, want signbit=%v",
			vecID, field, math.Signbit(got), math.Signbit(want))
	}
}

func assertScalarsEqual(t *testing.T, vecID string, a, b p_types.Scalars) {
	t.Helper()
	checkF := func(name string, x, y float64) {
		if math.IsNaN(x) && math.IsNaN(y) {
			return
		}
		if x != y {
			t.Fatalf("%s.%s: roundtrip float mismatch: %v vs %v", vecID, name, x, y)
		}
		if x == 0 && math.Signbit(x) != math.Signbit(y) {
			t.Fatalf("%s.%s: roundtrip zero sign mismatch: %v vs %v", vecID, name, math.Signbit(x), math.Signbit(y))
		}
	}
	checkF("a-f32", float64(a.AF32), float64(b.AF32))
	checkF("a-f64", a.AF64, b.AF64)
	if a.ABool != b.ABool || a.AChar != b.AChar || a.AString != b.AString {
		t.Fatalf("%s: scalar roundtrip mismatch: %+v vs %+v", vecID, a, b)
	}
}

func assertScalarsPlainJSON(t *testing.T, vecID string, v p_types.Scalars, raw json.RawMessage) {
	t.Helper()
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil {
		t.Fatalf("%s: failed to unmarshal scalars plain_json: %v", vecID, err)
	}
	if len(fields) != 5 {
		t.Fatalf("%s: expected exactly 5 fields in scalars plain_json, got %d (%v)", vecID, len(fields), fields)
	}
	float32MatchesJSON(t, vecID, "a-f32", v.AF32, string(fields["a-f32"]))
	float64MatchesJSON(t, vecID, "a-f64", v.AF64, string(fields["a-f64"]))
	var wantBool bool
	if err := json.Unmarshal(fields["a-bool"], &wantBool); err != nil || v.ABool != wantBool {
		t.Fatalf("%s.a-bool: got %v, raw=%s", vecID, v.ABool, fields["a-bool"])
	}
	var wantChar string
	if err := json.Unmarshal(fields["a-char"], &wantChar); err != nil || string(v.AChar) != wantChar {
		t.Fatalf("%s.a-char: got %q, raw=%s", vecID, string(v.AChar), fields["a-char"])
	}
	var wantStr string
	if err := json.Unmarshal(fields["a-string"], &wantStr); err != nil || v.AString != wantStr {
		t.Fatalf("%s.a-string: got %q, raw=%s", vecID, v.AString, fields["a-string"])
	}
}

// --- Generic decode + round-trip runner ---

type decodeRunner struct {
	newMsg     func() proto.Message
	fromProto  func(proto.Message) (any, error)
	toProto    func(any) (proto.Message, error)
	checkPlain func(t *testing.T, vecID string, val any, wantRaw json.RawMessage)
	checkEqual func(t *testing.T, vecID string, a, b any)
}

func makeJSONRunner[M proto.Message, P any](
	newMsg func() M,
	fromProto func(M) (P, error),
	toProto func(P) (M, error),
	toJSON func(P) map[string]any,
) decodeRunner {
	return decodeRunner{
		newMsg: func() proto.Message { return newMsg() },
		fromProto: func(m proto.Message) (any, error) {
			return fromProto(m.(M))
		},
		toProto: func(v any) (proto.Message, error) {
			return toProto(v.(P))
		},
		checkPlain: func(t *testing.T, vecID string, val any, wantRaw json.RawMessage) {
			t.Helper()
			got := toJSON(val.(P))
			want := decodeJSONValue(t, wantRaw)
			if !reflect.DeepEqual(got, want) {
				t.Fatalf("%s: plain_json mismatch:\n  got:  %#v\n  want: %#v", vecID, got, want)
			}
		},
		checkEqual: func(t *testing.T, vecID string, a, b any) {
			t.Helper()
			if !reflect.DeepEqual(a, b) {
				t.Fatalf("%s: roundtrip value mismatch:\n  a: %#v\n  b: %#v", vecID, a, b)
			}
			ja := toJSON(a.(P))
			jb := toJSON(b.(P))
			if !reflect.DeepEqual(ja, jb) {
				t.Fatalf("%s: roundtrip JSON mismatch:\n  a: %#v\n  b: %#v", vecID, ja, jb)
			}
		},
	}
}

func (r decodeRunner) run(t *testing.T, vec vector, binErrors, jsonErrors map[string]bool) {
	t.Helper()
	if vec.ProtoBinaryBase64 == nil || vec.Proto3JSON == nil {
		t.Fatalf("%s: missing binary or proto3_json payload", vec.ID)
	}
	rawBytes, err := base64.StdEncoding.DecodeString(*vec.ProtoBinaryBase64)
	if err != nil {
		t.Fatalf("%s: decode base64: %v", vec.ID, err)
	}

	wireFromBin := r.newMsg()
	if err := proto.Unmarshal(rawBytes, wireFromBin); err != nil {
		t.Fatalf("%s: proto.Unmarshal failed: %v", vec.ID, err)
	}

	if vec.Expected == "accept" {
		// 1. Binary decode -> check -> encode -> binary -> decode -> compare.
		plainBin, err := r.fromProto(wireFromBin)
		if err != nil {
			t.Fatalf("%s (binary): unexpected FromProto error: %v", vec.ID, err)
		}
		r.checkPlain(t, vec.ID, plainBin, vec.PlainJSON)

		reMsgBin, err := r.toProto(plainBin)
		if err != nil {
			t.Fatalf("%s (binary): unexpected ToProto error: %v", vec.ID, err)
		}
		reBytes, err := proto.Marshal(reMsgBin)
		if err != nil {
			t.Fatalf("%s (binary): proto.Marshal failed: %v", vec.ID, err)
		}
		reParsedBin := r.newMsg()
		if err := proto.Unmarshal(reBytes, reParsedBin); err != nil {
			t.Fatalf("%s (binary): roundtrip proto.Unmarshal failed: %v", vec.ID, err)
		}
		roundtripBin, err := r.fromProto(reParsedBin)
		if err != nil {
			t.Fatalf("%s (binary): roundtrip FromProto error: %v", vec.ID, err)
		}
		r.checkEqual(t, vec.ID+" (bin-rt)", plainBin, roundtripBin)

		// 2. Proto3-JSON decode -> check -> encode -> proto3-JSON -> decode -> compare.
		wireFromJSON := r.newMsg()
		if err := protojson.Unmarshal([]byte(*vec.Proto3JSON), wireFromJSON); err != nil {
			t.Fatalf("%s (json): protojson.Unmarshal failed: %v\ninput: %s", vec.ID, err, *vec.Proto3JSON)
		}
		plainJSON, err := r.fromProto(wireFromJSON)
		if err != nil {
			t.Fatalf("%s (json): unexpected FromProto error: %v", vec.ID, err)
		}
		r.checkPlain(t, vec.ID, plainJSON, vec.PlainJSON)
		r.checkEqual(t, vec.ID+" (bin-vs-json)", plainBin, plainJSON)

		reMsgJSON, err := r.toProto(plainJSON)
		if err != nil {
			t.Fatalf("%s (json): unexpected ToProto error: %v", vec.ID, err)
		}
		reJSONBytes, err := protojson.Marshal(reMsgJSON)
		if err != nil {
			t.Fatalf("%s (json): protojson.Marshal failed: %v", vec.ID, err)
		}
		reParsedJSON := r.newMsg()
		if err := protojson.Unmarshal(reJSONBytes, reParsedJSON); err != nil {
			t.Fatalf("%s (json): roundtrip protojson.Unmarshal failed: %v\njson: %s", vec.ID, err, string(reJSONBytes))
		}
		roundtripJSON, err := r.fromProto(reParsedJSON)
		if err != nil {
			t.Fatalf("%s (json): roundtrip FromProto error: %v", vec.ID, err)
		}
		r.checkEqual(t, vec.ID+" (json-rt)", plainJSON, roundtripJSON)
		return
	}

	// Reject path
	if vec.Expected != "reject" {
		t.Fatalf("%s: unexpected expected=%q (want accept or reject)", vec.ID, vec.Expected)
	}
	if !bytes.Equal(bytes.TrimSpace(vec.PlainJSON), []byte("null")) {
		t.Fatalf("%s: decode-reject vector must have plain_json=null, got %s", vec.ID, string(vec.PlainJSON))
	}
	if vec.ErrorKind == nil || vec.ErrorTarget == nil {
		t.Fatalf("%s: missing error expectations", vec.ID)
	}
	wantKind := *vec.ErrorKind
	wantTarget := *vec.ErrorTarget

	// 1. Binary decode must fail with exact *GlueError
	_, binErr := r.fromProto(wireFromBin)
	geBin := assertGlueErrorDetails(t, vec.ID+" (binary)", binErr, wantKind, wantTarget)
	binErrors[string(geBin.Kind)] = true

	// 2. Proto3-JSON decode must parse cleanly and fail in FromProto with exact *GlueError
	wireFromJSON := r.newMsg()
	if err := protojson.Unmarshal([]byte(*vec.Proto3JSON), wireFromJSON); err != nil {
		t.Fatalf("%s (json-reject): protojson.Unmarshal failed: %v\ninput: %s", vec.ID, err, *vec.Proto3JSON)
	}
	_, jsonErr := r.fromProto(wireFromJSON)
	geJSON := assertGlueErrorDetails(t, vec.ID+" (json)", jsonErr, wantKind, wantTarget)
	jsonErrors[string(geJSON.Kind)] = true
}

func parseBigIntFromJSON(t *testing.T, ctx string, v any) *big.Int {
	t.Helper()
	switch x := v.(type) {
	case int64:
		return big.NewInt(x)
	case string:
		bi, ok := new(big.Int).SetString(x, 10)
		if !ok {
			t.Fatalf("%s: invalid decimal integer %q", ctx, x)
		}
		return bi
	default:
		t.Fatalf("%s: expected integer or decimal string, got %T (%#v)", ctx, v, v)
		return nil
	}
}

func parseColorFromJSON(t *testing.T, ctx string, v any) p_types.Color {
	t.Helper()
	switch x := v.(type) {
	case string:
		switch x {
		case "red":
			return p_types.ColorRed
		case "green":
			return p_types.ColorGreen
		case "blue":
			return p_types.ColorBlue
		default:
			t.Fatalf("%s: unexpected string color %q", ctx, x)
		}
	case int64:
		return p_types.Color(x)
	default:
		t.Fatalf("%s: unexpected color %T (%#v)", ctx, v, v)
	}
	return 0
}

func parseSharedStatusFromJSON(t *testing.T, ctx string, v any) p_dep_shared.SharedStatus {
	t.Helper()
	switch x := v.(type) {
	case string:
		switch x {
		case "active":
			return p_dep_shared.SharedStatusActive
		case "paused":
			return p_dep_shared.SharedStatusPaused
		default:
			t.Fatalf("%s: unexpected string shared-status %q", ctx, x)
		}
	case int64:
		return p_dep_shared.SharedStatus(x)
	default:
		t.Fatalf("%s: unexpected shared-status %T (%#v)", ctx, v, v)
	}
	return 0
}

func parsePointFromJSON(t *testing.T, ctx string, v any) p_types.Point {
	t.Helper()
	m := v.(map[string]any)
	return p_types.Point{
		X: int32(m["x"].(int64)),
		Y: int32(m["y"].(int64)),
	}
}

func runEncodeReject(t *testing.T, vec vector, encodeErrors map[string]bool) {
	t.Helper()
	if vec.Expected != "reject" || vec.ErrorKind == nil || vec.ErrorTarget == nil {
		t.Fatalf("%s: unexpected encode vector expectations", vec.ID)
	}
	wantKind := *vec.ErrorKind
	wantTarget := *vec.ErrorTarget

	raw := decodeJSONValue(t, vec.PlainJSON).(map[string]any)
	var err error
	var wantExactInt *int64
	var wantExactCodePoint *uint32
	var wantExactLen *[2]int

	switch vec.ProtoMessage {
	case ".witforge.conformance.v0.TypesScalars":
		var cp rune
		if s, ok := raw["a-char"].(string); ok {
			if len([]rune(s)) == 1 {
				t.Fatalf("%s: expected invalid multi/empty char string, got valid single rune %q", vec.ID, s)
			}
			// Go's `rune` statically requires a single code point; map non-1-rune strings to -1.
			cp = -1
		} else if n, ok := raw["a-char-codepoint"].(int64); ok {
			cp = rune(n)
			if utf8.ValidRune(cp) {
				t.Fatalf("%s: expected invalid scalar code point, got valid rune %d", vec.ID, n)
			}
			u := uint32(cp)
			wantExactCodePoint = &u
		} else {
			t.Fatalf("%s: TypesScalars encode vector missing a-char and a-char-codepoint", vec.ID)
		}
		f32Val := float32(0)
		switch x := raw["a-f32"].(type) {
		case float64:
			f32Val = float32(x)
		case int64:
			f32Val = float32(x)
		default:
			t.Fatalf("%s: unexpected a-f32 %T", vec.ID, raw["a-f32"])
		}
		f64Val := float64(0)
		switch x := raw["a-f64"].(type) {
		case float64:
			f64Val = x
		case int64:
			f64Val = float64(x)
		default:
			t.Fatalf("%s: unexpected a-f64 %T", vec.ID, raw["a-f64"])
		}
		_, err = g_types.ScalarsToProto(p_types.Scalars{
			AF32:    f32Val,
			AF64:    f64Val,
			ABool:   raw["a-bool"].(bool),
			AChar:   cp,
			AString: raw["a-string"].(string),
		})
	case ".witforge.conformance.v0.TypesComposites":
		origin := parsePointFromJSON(t, vec.ID+".origin", raw["origin"])
		colorVal := parseColorFromJSON(t, vec.ID+".color", raw["color"])
		permsMap := raw["perms"].(map[string]any)
		perms := p_types.Permissions{
			Read:    permsMap["read"].(bool),
			Write:   permsMap["write"].(bool),
			Execute: permsMap["execute"].(bool),
		}
		choiceMap := raw["choice"].(map[string]any)
		var choice p_types.Choice
		switch choiceMap["case"].(string) {
		case "unit-case":
			choice = p_types.ChoiceUnitCase{}
		default:
			t.Fatalf("%s: unhandled choice case %v", vec.ID, choiceMap)
		}
		outcomeMap := raw["outcome"].(map[string]any)
		var outcome p_types.Outcome
		if okVal, ok := outcomeMap["ok"]; ok {
			outcome = p_wt.NewOk[uint16, string](uint16(okVal.(int64)))
		} else {
			outcome = p_wt.NewErr[uint16, string](outcomeMap["err"].(string))
		}
		foreignMap := raw["foreign"].(map[string]any)
		foreignID, parseErr := strconv.ParseUint(foreignMap["id"].(string), 10, 64)
		if parseErr != nil {
			t.Fatalf("%s: parse foreign.id: %v", vec.ID, parseErr)
		}
		foreign := p_dep_shared.SharedMeta{
			Id:     foreignID,
			Tag:    uint8(foreignMap["tag"].(int64)),
			Status: parseSharedStatusFromJSON(t, vec.ID+".foreign.status", foreignMap["status"]),
		}
		statusVal := parseSharedStatusFromJSON(t, vec.ID+".foreign-status", raw["foreign-status"])
		_, err = g_types.CompositesToProto(p_types.Composites{
			Origin:        origin,
			Color:         colorVal,
			Perms:         perms,
			Choice:        choice,
			Outcome:       outcome,
			Foreign:       foreign,
			ForeignStatus: statusVal,
		})
	case ".witforge.conformance.v0.TypesIntegers":
		// Go enforces exact integer widths statically (`uint8..uint64`, `int8..int64`); parse the
		// exact violating integer from `vec.PlainJSON` (using `math/big.Int` for 64-bit overflows),
		// prove it lies outside Go's static type bounds, and validate narrow-width helpers directly
		// without wraparound casts.
		fieldKey := strings.TrimPrefix(wantTarget, "types/integers.")
		bi := parseBigIntFromJSON(t, vec.ID+"."+fieldKey, raw[fieldKey])
		var lo, hi *big.Int
		switch fieldKey {
		case "a-u8":
			lo, hi = big.NewInt(0), big.NewInt(math.MaxUint8)
		case "a-u16":
			lo, hi = big.NewInt(0), big.NewInt(math.MaxUint16)
		case "a-u32":
			lo, hi = big.NewInt(0), big.NewInt(math.MaxUint32)
		case "a-u64":
			lo, hi = big.NewInt(0), new(big.Int).SetUint64(math.MaxUint64)
		case "a-s8":
			lo, hi = big.NewInt(math.MinInt8), big.NewInt(math.MaxInt8)
		case "a-s16":
			lo, hi = big.NewInt(math.MinInt16), big.NewInt(math.MaxInt16)
		case "a-s32":
			lo, hi = big.NewInt(math.MinInt32), big.NewInt(math.MaxInt32)
		case "a-s64":
			lo, hi = big.NewInt(math.MinInt64), big.NewInt(math.MaxInt64)
		default:
			t.Fatalf("%s: unexpected TypesIntegers target %q", vec.ID, wantTarget)
		}
		if bi.Cmp(lo) >= 0 && bi.Cmp(hi) <= 0 {
			t.Fatalf("%s: parsed value %s is unexpectedly within [%s, %s]", vec.ID, bi, lo, hi)
		}
		switch fieldKey {
		case "a-u8":
			v := bi.Int64()
			wantExactInt = &v
			if v > math.MaxUint8 {
				_, err = g_helpers.DecodeU8(uint32(v), wantTarget)
			} else {
				err = g_glueerr.IntegerOutOfRange(wantTarget, v)
			}
		case "a-u16":
			v := bi.Int64()
			wantExactInt = &v
			if v > math.MaxUint16 {
				_, err = g_helpers.DecodeU16(uint32(v), wantTarget)
			} else {
				err = g_glueerr.IntegerOutOfRange(wantTarget, v)
			}
		case "a-s8":
			v := bi.Int64()
			wantExactInt = &v
			_, err = g_helpers.DecodeS8(int32(v), wantTarget)
		case "a-s16":
			v := bi.Int64()
			wantExactInt = &v
			_, err = g_helpers.DecodeS16(int32(v), wantTarget)
		case "a-u32", "a-s32":
			v := bi.Int64()
			wantExactInt = &v
			err = g_glueerr.IntegerOutOfRange(wantTarget, v)
		case "a-u64", "a-s64":
			// Exceeds Go's static uint64/int64 bounds (verified via big.Int above); clamp sentinel
			// sign for *GlueError.Value (int64) while preserving non-zero out-of-range value.
			v := int64(-1)
			if bi.Sign() > 0 {
				v = math.MaxInt64
			} else if bi.IsInt64() {
				v = bi.Int64()
			}
			wantExactInt = &v
			err = g_glueerr.IntegerOutOfRange(wantTarget, v)
		}
	case ".witforge.conformance.v0.TypesCollections":
		switch wantKind {
		case "InvalidLength":
			var actualLen, expectedLen int
			switch wantTarget {
			case "types/collections.fixed-bytes":
				expectedLen = 4
				actualLen = len(raw["fixed-bytes"].([]any))
			case "types/collections.fixed-u16s":
				expectedLen = 3
				actualLen = len(raw["fixed-u16s"].([]any))
			case "types/collections.fixed-chars":
				expectedLen = 2
				actualLen = len(raw["fixed-chars"].([]any))
			default:
				t.Fatalf("%s: unexpected fixed-length target %q", vec.ID, wantTarget)
			}
			if actualLen == expectedLen {
				t.Fatalf("%s: expected length mismatch for %s, got %d", vec.ID, wantTarget, actualLen)
			}
			wantExactLen = &[2]int{expectedLen, actualLen}
			err = g_helpers.CheckFixedBytes(make([]byte, actualLen), expectedLen, wantTarget)
		case "InvalidChar":
			charsRaw := raw["fixed-chars"].([]any)
			var arr [2]rune
			foundInvalid := false
			for i, item := range charsRaw {
				s, ok := item.(string)
				if !ok || len([]rune(s)) != 1 {
					arr[i] = -1
					foundInvalid = true
				} else {
					arr[i] = []rune(s)[0]
				}
			}
			if !foundInvalid {
				t.Fatalf("%s: expected invalid char in fixed-chars, got %v", vec.ID, charsRaw)
			}
			_, err = g_types.CollectionsToProto(p_types.Collections{
				FixedChars: arr,
			})
		case "IntegerOutOfRange":
			items := raw["fixed-u16s"].([]any)
			var badVal int64
			foundBad := false
			for _, item := range items {
				v := item.(int64)
				if v < 0 || v > math.MaxUint16 {
					badVal = v
					foundBad = true
					break
				}
			}
			if !foundBad {
				t.Fatalf("%s: expected out-of-range u16 in fixed-u16s, got %v", vec.ID, items)
			}
			wantExactInt = &badVal
			if badVal > math.MaxUint16 {
				_, err = g_helpers.DecodeU16(uint32(badVal), wantTarget)
			} else {
				err = g_glueerr.IntegerOutOfRange(wantTarget, badVal)
			}
		default:
			t.Fatalf("%s: unexpected TypesCollections encode reject kind %q", vec.ID, wantKind)
		}
	case ".witforge.conformance.v0.TypesMaps":
		switch wantKind {
		case "IntegerOutOfRange":
			switch wantTarget {
			case "types/maps.u8-to-str":
				m := raw["u8-to-str"].(map[string]any)
				var badKey int64
				found := false
				for k := range m {
					parsed, pErr := strconv.ParseInt(k, 10, 64)
					if pErr != nil {
						t.Fatalf("%s: parse u8 key %q: %v", vec.ID, k, pErr)
					}
					if parsed < 0 || parsed > math.MaxUint8 {
						badKey = parsed
						found = true
						break
					}
				}
				if !found {
					t.Fatalf("%s: expected out-of-range u8 key in %v", vec.ID, m)
				}
				wantExactInt = &badKey
				if badKey > math.MaxUint8 {
					_, err = g_helpers.DecodeU8(uint32(badKey), wantTarget)
				} else {
					err = g_glueerr.IntegerOutOfRange(wantTarget, badKey)
				}
			case "types/maps.s8-to-point":
				m := raw["s8-to-point"].(map[string]any)
				var badKey int64
				found := false
				for k := range m {
					parsed, pErr := strconv.ParseInt(k, 10, 64)
					if pErr != nil {
						t.Fatalf("%s: parse s8 key %q: %v", vec.ID, k, pErr)
					}
					if parsed < math.MinInt8 || parsed > math.MaxInt8 {
						badKey = parsed
						found = true
						break
					}
				}
				if !found {
					t.Fatalf("%s: expected out-of-range s8 key in %v", vec.ID, m)
				}
				wantExactInt = &badKey
				_, err = g_helpers.DecodeS8(int32(badKey), wantTarget)
			case "types/maps.str-to-u8":
				m := raw["str-to-u8"].(map[string]any)
				var badVal int64
				found := false
				for _, val := range m {
					v := val.(int64)
					if v < 0 || v > math.MaxUint8 {
						badVal = v
						found = true
						break
					}
				}
				if !found {
					t.Fatalf("%s: expected out-of-range u8 value in %v", vec.ID, m)
				}
				wantExactInt = &badVal
				if badVal > math.MaxUint8 {
					_, err = g_helpers.DecodeU8(uint32(badVal), wantTarget)
				} else {
					err = g_glueerr.IntegerOutOfRange(wantTarget, badVal)
				}
			default:
				t.Fatalf("%s: unexpected TypesMaps IntegerOutOfRange target %q", vec.ID, wantTarget)
			}
		case "InvalidChar":
			m := raw["char-to-color"].(map[string]any)
			parsedMap := make(map[rune]p_types.Color, len(m))
			foundBad := false
			for k, v := range m {
				runes := []rune(k)
				if len(runes) != 1 {
					parsedMap[-1] = parseColorFromJSON(t, vec.ID+".char-to-color", v)
					foundBad = true
				} else {
					parsedMap[runes[0]] = parseColorFromJSON(t, vec.ID+".char-to-color", v)
				}
			}
			if !foundBad {
				t.Fatalf("%s: expected invalid char key in %v", vec.ID, m)
			}
			_, err = g_types.MapsToProto(p_types.Maps{CharToColor: parsedMap})
		case "UnknownEnumValue":
			m := raw["char-to-color"].(map[string]any)
			parsedMap := make(map[rune]p_types.Color, len(m))
			for k, v := range m {
				runes := []rune(k)
				if len(runes) != 1 {
					t.Fatalf("%s: unexpected map char key %q", vec.ID, k)
				}
				parsedMap[runes[0]] = parseColorFromJSON(t, vec.ID+".char-to-color", v)
			}
			_, err = g_types.MapsToProto(p_types.Maps{CharToColor: parsedMap})
		default:
			t.Fatalf("%s: unexpected TypesMaps encode reject kind %q", vec.ID, wantKind)
		}
	default:
		t.Fatalf("%s: unhandled encode proto_message %q", vec.ID, vec.ProtoMessage)
	}
	ge := assertGlueErrorDetails(t, vec.ID+" (encode)", err, wantKind, wantTarget)
	if wantExactInt != nil && ge.Value != *wantExactInt {
		t.Fatalf("%s: IntegerOutOfRange Value mismatch: got %d, want %d", vec.ID, ge.Value, *wantExactInt)
	}
	if wantExactCodePoint != nil && ge.CodePoint != *wantExactCodePoint {
		t.Fatalf("%s: InvalidChar CodePoint mismatch: got %#x, want %#x", vec.ID, ge.CodePoint, *wantExactCodePoint)
	}
	if wantExactLen != nil && (ge.Expected != wantExactLen[0] || ge.Actual != wantExactLen[1]) {
		t.Fatalf("%s: InvalidLength mismatch: got (%d, %d), want (%d, %d)",
			vec.ID, ge.Expected, ge.Actual, wantExactLen[0], wantExactLen[1])
	}
	encodeErrors[string(ge.Kind)] = true
}

func TestGoProtobufGlueConformance(t *testing.T) {
	bundleDir := os.Getenv("WITFORGE_CONFORMANCE_BUNDLE")
	if bundleDir == "" {
		bundleDir = filepath.Join(".", "bundle")
	}
	rawManifest, err := os.ReadFile(filepath.Join(bundleDir, "vectors.json"))
	if err != nil {
		t.Fatalf("read vectors.json: %v", err)
	}
	var manifest vectorManifest
	if err := json.Unmarshal(rawManifest, &manifest); err != nil {
		t.Fatalf("parse vectors.json: %v", err)
	}
	if manifest.ConformanceVersion != 1 {
		t.Fatalf("unexpected conformance_version: %d", manifest.ConformanceVersion)
	}

	// Verify Method constants match generated pb_root enum values.
	if g_api.MethodEchoIntegers != pb_root.ApiMethod_API_METHOD_ECHO_INTEGERS {
		t.Fatalf("MethodEchoIntegers mismatch")
	}
	if g_api.MethodProcessForeign != pb_root.ApiMethod_API_METHOD_PROCESS_FOREIGN {
		t.Fatalf("MethodProcessForeign mismatch")
	}
	if g_api.MethodNotify != pb_root.ApiMethod_API_METHOD_NOTIFY {
		t.Fatalf("MethodNotify mismatch")
	}

	type processForeignArgs struct {
		meta   p_dep_shared.SharedMeta
		status p_dep_shared.SharedStatus
		code   uint16
	}
	type notifyArgs struct {
		label    string
		priority uint8
	}

	runners := map[string]decodeRunner{
		".witforge.conformance.v0.TypesIntegers": makeJSONRunner(
			func() *pb_root.TypesIntegers { return &pb_root.TypesIntegers{} },
			g_types.IntegersFromProto,
			g_types.IntegersToProto,
			integersToJSON,
		),
		".witforge.conformance.v0.TypesScalars": {
			newMsg: func() proto.Message { return &pb_root.TypesScalars{} },
			fromProto: func(m proto.Message) (any, error) {
				return g_types.ScalarsFromProto(m.(*pb_root.TypesScalars))
			},
			toProto: func(v any) (proto.Message, error) {
				return g_types.ScalarsToProto(v.(p_types.Scalars))
			},
			checkPlain: func(t *testing.T, vecID string, val any, wantRaw json.RawMessage) {
				assertScalarsPlainJSON(t, vecID, val.(p_types.Scalars), wantRaw)
			},
			checkEqual: func(t *testing.T, vecID string, a, b any) {
				assertScalarsEqual(t, vecID, a.(p_types.Scalars), b.(p_types.Scalars))
			},
		},
		".witforge.conformance.v0.TypesOptionals": makeJSONRunner(
			func() *pb_root.TypesOptionals { return &pb_root.TypesOptionals{} },
			g_types.OptionalsFromProto,
			g_types.OptionalsToProto,
			optionalsToJSON,
		),
		".witforge.conformance.v0.TypesCollections": makeJSONRunner(
			func() *pb_root.TypesCollections { return &pb_root.TypesCollections{} },
			g_types.CollectionsFromProto,
			g_types.CollectionsToProto,
			collectionsToJSON,
		),
		".witforge.conformance.v0.TypesMaps": makeJSONRunner(
			func() *pb_root.TypesMaps { return &pb_root.TypesMaps{} },
			g_types.MapsFromProto,
			g_types.MapsToProto,
			mapsToJSON,
		),
		".witforge.conformance.v0.TypesChoice": makeJSONRunner(
			func() *pb_root.TypesChoice { return &pb_root.TypesChoice{} },
			g_types.ChoiceFromProto,
			g_types.ChoiceToProto,
			choiceToJSON,
		),
		".witforge.conformance.v0.TypesResults": makeJSONRunner(
			func() *pb_root.TypesResults { return &pb_root.TypesResults{} },
			g_types.ResultsFromProto,
			g_types.ResultsToProto,
			resultsToJSON,
		),
		".witforge.conformance.v0.TypesPermissions": makeJSONRunner(
			func() *pb_root.TypesPermissions { return &pb_root.TypesPermissions{} },
			g_types.PermissionsFromProto,
			g_types.PermissionsToProto,
			permissionsToJSON,
		),
		".witforge.conformance.v0.TypesComposites": makeJSONRunner(
			func() *pb_root.TypesComposites { return &pb_root.TypesComposites{} },
			g_types.CompositesFromProto,
			g_types.CompositesToProto,
			compositesToJSON,
		),
		".witforge.conformance_dep.v0.SharedSharedMeta": makeJSONRunner(
			func() *pb_dep.SharedSharedMeta { return &pb_dep.SharedSharedMeta{} },
			g_dep_shared.SharedMetaFromProto,
			g_dep_shared.SharedMetaToProto,
			sharedMetaToJSON,
		),
		".witforge.conformance.v0.ApiEchoIntegersRequest": makeJSONRunner(
			func() *pb_root.ApiEchoIntegersRequest { return &pb_root.ApiEchoIntegersRequest{} },
			g_api.EchoIntegersFromRequest,
			g_api.EchoIntegersToRequest,
			func(v p_types.Integers) map[string]any {
				return map[string]any{"input": integersToJSON(v)}
			},
		),
		".witforge.conformance.v0.ApiEchoIntegersResponse": makeJSONRunner(
			func() *pb_root.ApiEchoIntegersResponse { return &pb_root.ApiEchoIntegersResponse{} },
			g_api.EchoIntegersFromResponse,
			g_api.EchoIntegersToResponse,
			func(v p_types.Integers) map[string]any {
				return map[string]any{"result": integersToJSON(v)}
			},
		),
		".witforge.conformance.v0.ApiProcessForeignRequest": makeJSONRunner(
			func() *pb_root.ApiProcessForeignRequest { return &pb_root.ApiProcessForeignRequest{} },
			func(m *pb_root.ApiProcessForeignRequest) (processForeignArgs, error) {
				meta, status, code, err := g_api.ProcessForeignFromRequest(m)
				return processForeignArgs{meta: meta, status: status, code: code}, err
			},
			func(a processForeignArgs) (*pb_root.ApiProcessForeignRequest, error) {
				return g_api.ProcessForeignToRequest(a.meta, a.status, a.code)
			},
			func(a processForeignArgs) map[string]any {
				return map[string]any{
					"meta":   sharedMetaToJSON(a.meta),
					"status": a.status.String(),
					"code":   int64(a.code),
				}
			},
		),
		".witforge.conformance.v0.ApiProcessForeignResponse": makeJSONRunner(
			func() *pb_root.ApiProcessForeignResponse { return &pb_root.ApiProcessForeignResponse{} },
			g_api.ProcessForeignFromResponse,
			g_api.ProcessForeignToResponse,
			func(res p_wt.Result[p_types.Point, p_types.Color]) map[string]any {
				if res.IsOk {
					return map[string]any{"result": map[string]any{"ok": pointToJSON(res.Ok)}}
				}
				return map[string]any{"result": map[string]any{"err": res.Err.String()}}
			},
		),
		".witforge.conformance.v0.ApiNotifyRequest": makeJSONRunner(
			func() *pb_root.ApiNotifyRequest { return &pb_root.ApiNotifyRequest{} },
			func(m *pb_root.ApiNotifyRequest) (notifyArgs, error) {
				label, priority, err := g_api.NotifyFromRequest(m)
				return notifyArgs{label: label, priority: priority}, err
			},
			func(a notifyArgs) (*pb_root.ApiNotifyRequest, error) {
				return g_api.NotifyToRequest(a.label, a.priority)
			},
			func(a notifyArgs) map[string]any {
				return map[string]any{
					"label":    a.label,
					"priority": int64(a.priority),
				}
			},
		),
		".witforge.conformance.v0.ApiNotifyResponse": makeJSONRunner(
			func() *pb_root.ApiNotifyResponse { return &pb_root.ApiNotifyResponse{} },
			func(m *pb_root.ApiNotifyResponse) (struct{}, error) {
				return struct{}{}, g_api.NotifyFromResponse(m)
			},
			func(_ struct{}) (*pb_root.ApiNotifyResponse, error) {
				return g_api.NotifyToResponse()
			},
			func(_ struct{}) map[string]any {
				return map[string]any{}
			},
		),
	}

	decodeAcceptCount := 0
	decodeRejectCount := 0
	encodeRejectCount := 0
	binErrors := map[string]bool{}
	jsonErrors := map[string]bool{}
	encodeErrors := map[string]bool{}

	for _, vec := range manifest.Vectors {
		switch vec.Direction {
		case "encode":
			runEncodeReject(t, vec, encodeErrors)
			encodeRejectCount++
		case "decode":
			runner, ok := runners[vec.ProtoMessage]
			if !ok {
				t.Fatalf("%s: missing runner for %q", vec.ID, vec.ProtoMessage)
			}
			runner.run(t, vec, binErrors, jsonErrors)
			if vec.Expected == "accept" {
				decodeAcceptCount++
			} else {
				decodeRejectCount++
			}
		default:
			t.Fatalf("%s: unexpected direction %q", vec.ID, vec.Direction)
		}
	}

	total := decodeAcceptCount + decodeRejectCount + encodeRejectCount
	if total != len(manifest.Vectors) {
		t.Fatalf("vector count mismatch: got %d, want %d", total, len(manifest.Vectors))
	}
	for _, kind := range manifest.ErrorKinds {
		if !binErrors[kind] {
			t.Fatalf("binary decode did not exercise error kind %q", kind)
		}
		if !jsonErrors[kind] {
			t.Fatalf("proto3-JSON decode did not exercise error kind %q", kind)
		}
	}
	for _, kind := range []string{"IntegerOutOfRange", "InvalidChar", "InvalidLength", "UnknownEnumValue"} {
		if !encodeErrors[kind] {
			t.Fatalf("encode bounds did not exercise error kind %q", kind)
		}
	}

	// Verify nil-message guards and invalid UTF-8 / nil-variant / nil-bytes encode checks.
	_, err = g_types.IntegersFromProto(nil)
	assertGlueErrorDetails(t, "IntegersFromProto(nil)", err, "InvalidState", "types/integers")
	_, err = g_types.ChoiceFromProto(nil)
	assertGlueErrorDetails(t, "ChoiceFromProto(nil)", err, "UnknownCase", "types/choice")
	_, err = g_types.PermissionsFromProto(nil)
	assertGlueErrorDetails(t, "PermissionsFromProto(nil)", err, "InvalidState", "types/permissions")
	_, err = g_api.EchoIntegersFromRequest(nil)
	assertGlueErrorDetails(t, "EchoIntegersFromRequest(nil)", err, "InvalidState", "api#echo-integers.request")
	err = g_api.NotifyFromResponse(nil)
	assertGlueErrorDetails(t, "NotifyFromResponse(nil)", err, "InvalidState", "api#notify.response")

	_, err = g_types.ChoiceToProto(nil)
	assertGlueErrorDetails(t, "ChoiceToProto(nil)", err, "UnknownCase", "types/choice")
	_, err = g_types.ChoiceToProto((*p_types.ChoiceSmall)(nil))
	assertGlueErrorDetails(t, "ChoiceToProto(typed-nil-payload)", err, "UnknownCase", "types/choice")
	_, err = g_types.ChoiceToProto((*p_types.ChoiceUnitCase)(nil))
	assertGlueErrorDetails(t, "ChoiceToProto(typed-nil-unit)", err, "UnknownCase", "types/choice")

	_, err = g_types.ScalarsToProto(p_types.Scalars{AString: string([]byte{0xff, 0xfe}), AChar: 'a'})
	assertGlueErrorDetails(t, "invalid-utf8", err, "InvalidState", "types/scalars.a-string")

	// Required nil []byte and present optional &nilSlice must preserve proto3 presence across wire.
	nilColMsg, err := g_types.CollectionsToProto(p_types.Collections{
		RawBytes:   nil,
		FixedBytes: [4]byte{1, 2, 3, 4},
		FixedU16s:  [3]uint16{1, 2, 3},
		FixedChars: [2]rune{'a', 'b'},
	})
	if err != nil {
		t.Fatalf("CollectionsToProto(nil RawBytes) failed: %v", err)
	}
	nilColWire, err := proto.Marshal(nilColMsg)
	if err != nil {
		t.Fatalf("marshal nil RawBytes: %v", err)
	}
	nilColParsed := &pb_root.TypesCollections{}
	if err := proto.Unmarshal(nilColWire, nilColParsed); err != nil {
		t.Fatalf("unmarshal nil RawBytes: %v", err)
	}
	nilColBack, err := g_types.CollectionsFromProto(nilColParsed)
	if err != nil || nilColBack.RawBytes == nil || len(nilColBack.RawBytes) != 0 {
		t.Fatalf("expected non-nil empty RawBytes roundtrip, got %#v, err=%v", nilColBack.RawBytes, err)
	}

	var nilSlice []byte
	nilOptMsg, err := g_types.OptionalsToProto(p_types.Optionals{OptBytes: &nilSlice})
	if err != nil {
		t.Fatalf("OptionalsToProto(&nilSlice) failed: %v", err)
	}
	nilOptWire, err := proto.Marshal(nilOptMsg)
	if err != nil {
		t.Fatalf("marshal &nilSlice: %v", err)
	}
	nilOptParsed := &pb_root.TypesOptionals{}
	if err := proto.Unmarshal(nilOptWire, nilOptParsed); err != nil {
		t.Fatalf("unmarshal &nilSlice: %v", err)
	}
	nilOptBack, err := g_types.OptionalsFromProto(nilOptParsed)
	if err != nil || nilOptBack.OptBytes == nil || *nilOptBack.OptBytes == nil || len(*nilOptBack.OptBytes) != 0 {
		t.Fatalf("expected present non-nil empty OptBytes roundtrip, got %#v, err=%v", nilOptBack.OptBytes, err)
	}

	// Pointer-receiver variant case value must also work identically to value case.
	ptrChoiceMsg, err := g_types.ChoiceToProto(&p_types.ChoiceSmall{Value: 7})
	if err != nil {
		t.Fatalf("ChoiceToProto(&ChoiceSmall) failed: %v", err)
	}
	ptrChoiceBack, err := g_types.ChoiceFromProto(ptrChoiceMsg)
	if err != nil || ptrChoiceBack != (p_types.ChoiceSmall{Value: 7}) {
		t.Fatalf("ChoiceFromProto roundtrip mismatch: %#v, err=%v", ptrChoiceBack, err)
	}

	t.Logf(
		"Go conformance passed: %d vectors (%d decode accept, %d decode reject, %d encode reject; all %d GlueError kinds verified)",
		total, decodeAcceptCount, decodeRejectCount, encodeRejectCount, len(binErrors),
	)
}
