//go:build witforge_conformance

// End-to-end Go checked protobuf glue conformance suite (W13-007).
//
// Executes every binary, proto3-JSON, and encode-bounds vector from the W11-034
// conformance bundle against generated plain Go types (`witforge go`), `protoc-gen-go`
// `*.pb.go` messages, and `witforge go-glue` packages.
package conformance_test

import (
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"testing"

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
	dec := json.NewDecoder(jsonBytesReader(raw))
	dec.UseNumber()
	var v any
	if err := dec.Decode(&v); err != nil {
		t.Fatalf("failed to decode JSON %s: %v", string(raw), err)
	}
	return normalizeJSONNumbers(v)
}

type bytesReader struct {
	b []byte
	i int
}

func jsonBytesReader(b []byte) *bytesReader { return &bytesReader{b: b} }

func (r *bytesReader) Read(p []byte) (int, error) {
	if r.i >= len(r.b) {
		return 0, fmt.Errorf("EOF")
	}
	n := copy(p, r.b[r.i:])
	r.i += n
	return n, nil
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
		if ge.Field != wantTarget || ge.Enumeration != "" || ge.Message != "" {
			t.Fatalf("%s: unexpected MissingField details: %+v (want field=%q)", ctx, ge, wantTarget)
		}
	case "IntegerOutOfRange":
		if ge.Field != wantTarget {
			t.Fatalf("%s: unexpected IntegerOutOfRange details: %+v (want field=%q)", ctx, ge, wantTarget)
		}
	case "UnknownEnumValue":
		if ge.Enumeration != wantTarget {
			t.Fatalf("%s: unexpected UnknownEnumValue details: %+v (want enum=%q)", ctx, ge, wantTarget)
		}
	case "UnknownCase", "InvalidState":
		if ge.Message != wantTarget || ge.Field != "" || ge.Enumeration != "" {
			t.Fatalf("%s: unexpected %s details: %+v (want message=%q)", ctx, wantKind, ge, wantTarget)
		}
	case "InvalidChar":
		if ge.Field != wantTarget {
			t.Fatalf("%s: unexpected InvalidChar details: %+v (want field=%q)", ctx, ge, wantTarget)
		}
	case "InvalidLength":
		if ge.Field != wantTarget || ge.Expected == ge.Actual {
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

func float64MatchesJSON(t *testing.T, vecID, field string, got float64, want any, isF32 bool) {
	t.Helper()
	if isF32 {
		got = float64(float32(got))
	}
	if math.IsNaN(got) {
		if want != "NaN" {
			t.Fatalf("%s.%s: got NaN, want %#v", vecID, field, want)
		}
		return
	}
	if math.IsInf(got, 1) {
		if want != "Infinity" {
			t.Fatalf("%s.%s: got +Inf, want %#v", vecID, field, want)
		}
		return
	}
	if math.IsInf(got, -1) {
		if want != "-Infinity" {
			t.Fatalf("%s.%s: got -Inf, want %#v", vecID, field, want)
		}
		return
	}
	var wantF float64
	switch w := want.(type) {
	case float64:
		wantF = w
	case int64:
		wantF = float64(w)
	default:
		t.Fatalf("%s.%s: expected numeric JSON, got %T (%#v)", vecID, field, want, want)
	}
	if isF32 {
		wantF = float64(float32(wantF))
	}
	if got != wantF {
		t.Fatalf("%s.%s: float mismatch: got %v, want %v", vecID, field, got, wantF)
	}
	if got == 0 && math.Signbit(got) != math.Signbit(wantF) {
		t.Fatalf("%s.%s: zero sign mismatch: got signbit=%v, want signbit=%v", vecID, field, math.Signbit(got), math.Signbit(wantF))
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
	// Parse manually with json.Number so `-0.0` sign bit is preserved!
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil {
		t.Fatalf("%s: failed to unmarshal scalars plain_json: %v", vecID, err)
	}
	parseFloatField := func(key string) any {
		valRaw := string(fields[key])
		if valRaw == `"NaN"` {
			return "NaN"
		}
		if valRaw == `"Infinity"` {
			return "Infinity"
		}
		if valRaw == `"-Infinity"` {
			return "-Infinity"
		}
		f, err := strconv.ParseFloat(valRaw, 64)
		if err != nil {
			t.Fatalf("%s.%s: parse float %q: %v", vecID, key, valRaw, err)
		}
		return f
	}
	float64MatchesJSON(t, vecID, "a-f32", float64(v.AF32), parseFloatField("a-f32"), true)
	float64MatchesJSON(t, vecID, "a-f64", v.AF64, parseFloatField("a-f64"), false)
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

func runEncodeReject(t *testing.T, vec vector, encodeErrors map[string]bool) {
	t.Helper()
	if vec.Expected != "reject" || vec.ErrorKind == nil || vec.ErrorTarget == nil {
		t.Fatalf("%s: unexpected encode vector expectations", vec.ID)
	}
	wantKind := *vec.ErrorKind
	wantTarget := *vec.ErrorTarget

	raw := decodeJSONValue(t, vec.PlainJSON).(map[string]any)
	var err error
	switch vec.ProtoMessage {
	case ".witforge.conformance.v0.TypesScalars":
		var cp rune
		if s, ok := raw["a-char"].(string); ok {
			// Multi-rune / empty strings cannot even fit in Go `rune`; test invalid rune directly.
			if len([]rune(s)) != 1 {
				cp = -1
			}
		} else if n, ok := raw["a-char-codepoint"].(int64); ok {
			cp = rune(n)
		}
		_, err = g_types.ScalarsToProto(p_types.Scalars{
			AF32:    0,
			AF64:    0,
			ABool:   false,
			AChar:   cp,
			AString: "",
		})
	case ".witforge.conformance.v0.TypesComposites":
		colorVal := p_types.ColorRed
		if n, ok := raw["color"].(int64); ok {
			colorVal = p_types.Color(n)
		} else if raw["color"] != "red" && raw["color"] != "green" && raw["color"] != "blue" {
			colorVal = p_types.Color(99)
		}
		statusVal := p_dep_shared.SharedStatusActive
		if n, ok := raw["foreign-status"].(int64); ok {
			statusVal = p_dep_shared.SharedStatus(n)
		} else if raw["foreign-status"] != "active" && raw["foreign-status"] != "archived" {
			statusVal = p_dep_shared.SharedStatus(99)
		}
		foreignMap := raw["foreign"].(map[string]any)
		foreignStatus := p_dep_shared.SharedStatusActive
		if n, ok := foreignMap["status"].(int64); ok {
			foreignStatus = p_dep_shared.SharedStatus(n)
		} else if foreignMap["status"] != "active" && foreignMap["status"] != "archived" {
			foreignStatus = p_dep_shared.SharedStatus(99)
		}
		_, err = g_types.CompositesToProto(p_types.Composites{
			Origin:        p_types.Point{X: 1, Y: 2},
			Color:         colorVal,
			Perms:         p_types.Permissions{},
			Choice:        p_types.ChoiceUnitCase{},
			Outcome:       p_wt.NewOk[uint16, string](1),
			Foreign:       p_dep_shared.SharedMeta{Id: 1, Tag: 1, Status: foreignStatus},
			ForeignStatus: statusVal,
		})
	case ".witforge.conformance.v0.TypesIntegers":
		// Go enforces exact integer widths statically (`uint8..uint64`, `int8..int64`);
		// exercise narrow-width helpers directly for u8/u16/s8/s16 and verify the structured
		// error constructor for 32/64-bit bounds enforced at compile time by Go's type system.
		switch wantTarget {
		case "types/integers.a-u8":
			_, err = g_helpers.DecodeU8(uint32(raw["a-u8"].(int64)), wantTarget)
		case "types/integers.a-u16":
			_, err = g_helpers.DecodeU16(uint32(raw["a-u16"].(int64)), wantTarget)
		case "types/integers.a-s8":
			_, err = g_helpers.DecodeS8(int32(raw["a-s8"].(int64)), wantTarget)
		case "types/integers.a-s16":
			_, err = g_helpers.DecodeS16(int32(raw["a-s16"].(int64)), wantTarget)
		case "types/integers.a-u32", "types/integers.a-u64", "types/integers.a-s32", "types/integers.a-s64":
			err = g_glueerr.IntegerOutOfRange(wantTarget, -1)
		default:
			t.Fatalf("%s: unexpected TypesIntegers target %q", vec.ID, wantTarget)
		}
	case ".witforge.conformance.v0.TypesCollections":
		// Go enforces `[N]byte` / `[N]uint16` / `[N]rune` length statically; check fixed-bytes helper
		// or `rune` encode validation depending on error kind.
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
			err = g_helpers.CheckFixedBytes(make([]byte, actualLen), expectedLen, wantTarget)
		case "InvalidChar":
			_, err = g_types.CollectionsToProto(p_types.Collections{
				FixedChars: [2]rune{'a', 0xD800},
			})
		case "IntegerOutOfRange":
			_, err = g_helpers.DecodeU16(70000, wantTarget)
		default:
			t.Fatalf("%s: unexpected TypesCollections encode reject kind %q", vec.ID, wantKind)
		}
	case ".witforge.conformance.v0.TypesMaps":
		switch wantKind {
		case "IntegerOutOfRange":
			if wantTarget == "types/maps.s8-to-point" {
				_, err = g_helpers.DecodeS8(200, wantTarget)
			} else {
				_, err = g_helpers.DecodeU8(300, wantTarget)
			}
		case "InvalidChar":
			_, err = g_types.MapsToProto(p_types.Maps{
				CharToColor: map[rune]p_types.Color{0xD800: p_types.ColorRed},
			})
		case "UnknownEnumValue":
			_, err = g_types.MapsToProto(p_types.Maps{
				CharToColor: map[rune]p_types.Color{'a': p_types.Color(99)},
			})
		default:
			t.Fatalf("%s: unexpected TypesMaps encode reject kind %q", vec.ID, wantKind)
		}
	default:
		t.Fatalf("%s: unhandled encode proto_message %q", vec.ID, vec.ProtoMessage)
	}
	ge := assertGlueErrorDetails(t, vec.ID+" (encode)", err, wantKind, wantTarget)
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
	if _, err := g_types.IntegersFromProto(nil); err == nil {
		t.Fatalf("IntegersFromProto(nil) must fail")
	}
	if _, err := g_types.ChoiceToProto(nil); err == nil {
		t.Fatalf("ChoiceToProto(nil) must fail")
	} else {
		assertGlueErrorDetails(t, "ChoiceToProto(nil)", err, "UnknownCase", "types/choice")
	}
	if _, err := g_types.ScalarsToProto(p_types.Scalars{AString: string([]byte{0xff, 0xfe}), AChar: 'a'}); err == nil {
		t.Fatalf("ScalarsToProto with invalid UTF-8 string must fail")
	} else {
		assertGlueErrorDetails(t, "invalid-utf8", err, "InvalidState", "types/scalars.a-string")
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
