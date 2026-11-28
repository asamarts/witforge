#!/usr/bin/env bash
# End-to-end Go checked protobuf glue conformance harness (`W13-007`).
#
# Builds pinned `protoc-gen-go` from `ci/go-conformance/go.mod`, generates the `W11-034`
# conformance bundle with `--polyglot-proto-options --go-package ...`, emits plain Go
# (`witforge go`), compiles `.proto` files with `protoc --go_out`, emits checked Go glue
# (`witforge go-glue`), verifies zero-diff `gofmt`, runs `go build ./...`, `go vet ./...`, and
# `go test -v ./...` against every binary, proto3-JSON, and encode-bounds conformance vector,
# plus a multi-minor, multi-major, canonical-owner forwarding, disambiguated `deps/`, special
# package name (`main`/`init`/`documentation`), `protoc-gen-go` reserved field name
# (`string`/`reset`/`marshal`/`descriptor`), triple `option`, deterministic map-key error order,
# and nil-bytes edge-case smoke contract.
#
# Usage: [WITFORGE=path/to/witforge] [PROTOC=path/to/protoc] [GO=path/to/go] ci/check-go-conformance.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HARNESS_DIR="${ROOT}/ci/go-conformance"
PROTOC_BIN="${PROTOC:-protoc}"
GO_BIN="${GO:-go}"

if ! command -v "${PROTOC_BIN}" >/dev/null 2>&1; then
    echo "error: protoc not found (set PROTOC or install protoc)" >&2
    exit 1
fi
if ! command -v "${GO_BIN}" >/dev/null 2>&1; then
    echo "error: go not found (set GO or install Go 1.22+)" >&2
    exit 1
fi
if ! command -v gofmt >/dev/null 2>&1; then
    echo "error: gofmt not found" >&2
    exit 1
fi

if [[ -n "${WITFORGE:-}" ]]; then
    RUN=("${WITFORGE}")
else
    cargo build --manifest-path "${ROOT}/Cargo.toml" --locked -p witforge-cli
    RUN=("${ROOT}/target/debug/witforge")
fi

scratch="${TMPDIR:-${HOME}/.tmp}"
mkdir -p "${scratch}"
WORK="$(mktemp -d "${scratch%/}/witforge-go-conformance.XXXXXX")"
trap 'rm -rf "${WORK}"' EXIT

export GOCACHE="${WORK}/gocache"
mkdir -p "${GOCACHE}"

# Build pinned `protoc-gen-go` from checksum-locked `ci/go-conformance/go.mod` / `go.sum`.
BIN_DIR="${WORK}/bin"
mkdir -p "${BIN_DIR}"
(
    cd "${HARNESS_DIR}"
    "${GO_BIN}" build -o "${BIN_DIR}/protoc-gen-go" google.golang.org/protobuf/cmd/protoc-gen-go
)
export PATH="${BIN_DIR}:${PATH}"

# ---------------------------------------------------------------------------
# 1. Canonical W11-034 conformance bundle
# ---------------------------------------------------------------------------
CONF="${WORK}/conformance"
mkdir -p "${CONF}/proto"
cp "${HARNESS_DIR}/go.mod" "${HARNESS_DIR}/go.sum" "${HARNESS_DIR}/conformance_test.go" "${CONF}/"

"${RUN[@]}" conformance --out "${CONF}/bundle" \
    --polyglot-proto-options \
    --go-package example.com/witforge/go-conformance/proto
"${RUN[@]}" go "${CONF}/bundle/wit" --out "${CONF}/plain" \
    --go-module example.com/witforge/go-conformance/plain
"${RUN[@]}" go "${CONF}/bundle/wit" --out "${CONF}/plain" \
    --go-module example.com/witforge/go-conformance/plain --check
"${RUN[@]}" go-glue "${CONF}/bundle/wit" --out "${CONF}/glue" \
    --go-module example.com/witforge/go-conformance/glue \
    --plain-root example.com/witforge/go-conformance/plain \
    --proto-root example.com/witforge/go-conformance/proto
"${RUN[@]}" go-glue "${CONF}/bundle/wit" --out "${CONF}/glue" \
    --go-module example.com/witforge/go-conformance/glue \
    --plain-root example.com/witforge/go-conformance/plain \
    --proto-root example.com/witforge/go-conformance/proto --check

gofmt_diff="$(gofmt -d "${CONF}/plain" "${CONF}/glue")"
if [[ -n "${gofmt_diff}" ]]; then
    echo "gofmt diff on conformance Go output:" >&2
    echo "${gofmt_diff}" >&2
    exit 1
fi

"${PROTOC_BIN}" \
    -I "${CONF}/bundle/proto" \
    --go_out="${CONF}/proto" \
    --go_opt=module=example.com/witforge/go-conformance/proto \
    "${CONF}/bundle/proto/v0.proto" \
    "${CONF}/bundle/proto/witforge.conformance_dep.v0.proto"

(
    cd "${CONF}"
    "${GO_BIN}" build ./...
    "${GO_BIN}" vet -tags witforge_conformance ./...
    WITFORGE_CONFORMANCE_BUNDLE="${CONF}/bundle" "${GO_BIN}" test -tags witforge_conformance -v ./...
)

# ---------------------------------------------------------------------------
# 2. Multi-minor + multi-major + canonical-owner forwarding + disambiguated deps
#    + special package names + protoc-gen-go reserved fields + triple option
#    + deterministic sorted-key map error order + optional bytes presence
# ---------------------------------------------------------------------------
EDGE="${WORK}/edge"
mkdir -p \
    "${EDGE}/wit/v0.1/deps/shared" "${EDGE}/wit/v0.1/deps/kw" \
    "${EDGE}/wit/v0.2/deps/shared" "${EDGE}/wit/v0.2/deps/kw" \
    "${EDGE}/wit/v1.0/deps/shared" "${EDGE}/wit/v1.0/deps/kw" \
    "${EDGE}/wit/v2.0/deps/shared" "${EDGE}/wit/v2.0/deps/kw" \
    "${EDGE}/proto"
cp "${HARNESS_DIR}/go.mod" "${HARNESS_DIR}/go.sum" "${EDGE}/"

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
package %func:select@0.1.0;

interface main {
    record entry {
        code: u16,
    }
}
EOF
done

cat >"${EDGE}/wit/v0.1/contract.wit" <<'EOF'
package acme:edge@0.1.0;

interface empty-iface {}

interface main {
    use acme:shared/core@0.1.0.{tag, maybe-tags, foreign-bytes};
    use %func:select/main@0.1.0.{entry};

    record item {
        %string: string,
        reset: u8,
        marshal: bool,
        descriptor: list<u16, 2>,
        tag: tag,
        maybe-tags: maybe-tags,
        kw-entry: entry,
        raw: foreign-bytes,
    }
}
EOF

cat >"${EDGE}/wit/v0.2/contract.wit" <<'EOF'
package acme:edge@0.2.0;

interface empty-iface {}

interface main {
    use acme:shared/core@0.1.0.{tag, maybe-tags, foreign-bytes};
    use %func:select/main@0.1.0.{entry};

    /// +build linux
    /// Record exercising protoc-gen-go reserved method-name collisions.
    record item {
        %string: string,
        reset: u8,
        marshal: bool,
        descriptor: list<u16, 2>,
        tag: tag,
        maybe-tags: maybe-tags,
        kw-entry: entry,
        raw: foreign-bytes,
        added-in-v0-2: option<u16>,
    }

    type opt-bytes = option<list<u8>>;
    type opt-u16-list = option<list<u16>>;
    type opt-fixed-u16s = option<list<u16, 2>>;
    type opt-nested = option<option<u16>>;
    type opt-triple = option<option<option<u8>>>;
    type opt-of-named = option<opt-bytes>;
    type opt-map = option<map<u8, string>>;

    variant tricky {
        %string(u8),
        reset,
    }

    record holder {
        a: opt-bytes,
        al: opt-u16-list,
        af: opt-fixed-u16s,
        b: opt-nested,
        bt: opt-triple,
        c: tricky,
        d: opt-map,
        g: opt-of-named,
        h: list<opt-bytes>,
        hn: list<opt-nested>,
        m: map<u8, list<u8, 4>>,
    }

    call: func(it: item, h: holder, %type: string) -> option<item>;
    ping: func();
}

interface init {
    record marker {
        id: u8,
    }
}

interface documentation {
    record note {
        body: string,
    }
}
EOF

cat >"${EDGE}/wit/v1.0/contract.wit" <<'EOF'
package acme:edge@1.0.0;

interface main {
    use acme:shared/core@0.1.0.{tag, maybe-tags};
    use %func:select/main@0.1.0.{entry};

    record item {
        %string: string,
        tag: tag,
        maybe-tags: maybe-tags,
        kw-entry: entry,
    }
}
EOF

cat >"${EDGE}/wit/v2.0/contract.wit" <<'EOF'
package acme:edge@2.0.0;

interface main {
    use acme:shared/core@0.1.0.{tag, maybe-tags};
    use %func:select/main@0.1.0.{entry};

    record item {
        %string: string,
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

"${RUN[@]}" proto "${EDGE}/wit" --out "${EDGE}/proto-src" \
    --go-package example.com/witforge/go-conformance/proto
"${RUN[@]}" go "${EDGE}/wit" --out "${EDGE}/plain" \
    --go-module example.com/witforge/go-conformance/plain
"${RUN[@]}" go "${EDGE}/wit" --out "${EDGE}/plain" \
    --go-module example.com/witforge/go-conformance/plain --check
"${RUN[@]}" go-glue "${EDGE}/wit" --out "${EDGE}/glue" \
    --go-module example.com/witforge/go-conformance/glue \
    --plain-root example.com/witforge/go-conformance/plain \
    --proto-root example.com/witforge/go-conformance/proto
"${RUN[@]}" go-glue "${EDGE}/wit" --out "${EDGE}/glue" \
    --go-module example.com/witforge/go-conformance/glue \
    --plain-root example.com/witforge/go-conformance/plain \
    --proto-root example.com/witforge/go-conformance/proto --check

edge_gofmt_diff="$(gofmt -d "${EDGE}/plain" "${EDGE}/glue")"
if [[ -n "${edge_gofmt_diff}" ]]; then
    echo "gofmt diff on edge Go output:" >&2
    echo "${edge_gofmt_diff}" >&2
    exit 1
fi

shopt -s nullglob
edge_protos=("${EDGE}/proto-src/"*.proto)
shopt -u nullglob
if [[ ${#edge_protos[@]} -eq 0 ]]; then
    echo "error: no edge proto files generated" >&2
    exit 1
fi

"${PROTOC_BIN}" \
    -I "${EDGE}/proto-src" \
    --go_out="${EDGE}/proto" \
    --go_opt=module=example.com/witforge/go-conformance/proto \
    "${edge_protos[@]}"

cat >"${EDGE}/edge_test.go" <<'EOF'
package edge_test

import (
	"errors"
	"reflect"
	"testing"

	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"

	g_err "example.com/witforge/go-conformance/glue/glueerr"
	g_v0_doc "example.com/witforge/go-conformance/glue/v0/documentation_"
	_ "example.com/witforge/go-conformance/glue/v0/empty_iface"
	g_v0_init "example.com/witforge/go-conformance/glue/v0/init_"
	g_v0_main "example.com/witforge/go-conformance/glue/v0/main_"
	g_v1_main "example.com/witforge/go-conformance/glue/v1/main_"
	g_v2_main "example.com/witforge/go-conformance/glue/v2/main_"
	g_dep_v0 "example.com/witforge/go-conformance/glue/witforge_deps/v0/acme_shared/core"
	g_dep_v1 "example.com/witforge/go-conformance/glue/witforge_deps/v1/acme_shared/core"
	g_dep_v2 "example.com/witforge/go-conformance/glue/witforge_deps/v2/acme_shared/core"
	p_v0_doc "example.com/witforge/go-conformance/plain/v0_2/documentation_"
	p_v0_init "example.com/witforge/go-conformance/plain/v0_2/init_"
	p_v0_main "example.com/witforge/go-conformance/plain/v0_2/main_"
	p_v1_main "example.com/witforge/go-conformance/plain/v1_0/main_"
	p_v2_main "example.com/witforge/go-conformance/plain/v2_0/main_"
	p_kw "example.com/witforge/go-conformance/plain/witforge_deps/func_select/main_"
	p_dep_v0 "example.com/witforge/go-conformance/plain/witforge_deps/v0/acme_shared/core"
	p_dep_v1 "example.com/witforge/go-conformance/plain/witforge_deps/v1/acme_shared/core"
	p_dep_v2 "example.com/witforge/go-conformance/plain/witforge_deps/v2/acme_shared/core"
	p_wt "example.com/witforge/go-conformance/plain/witforge_types"
	pb_v0 "example.com/witforge/go-conformance/proto/acme/edge/v0"
	pb_v1 "example.com/witforge/go-conformance/proto/acme/edge/v1"
	pb_v2 "example.com/witforge/go-conformance/proto/acme/edge/v2"
)

func ptr[T any](v T) *T { return &v }

func TestEdgeContractsAndCanonicalDeps(t *testing.T) {
	// Ensure init_ and documentation_ packages compile and round-trip (empty_iface blank-imported).
	initMsg, err := g_v0_init.MarkerToProto(p_v0_init.Marker{Id: 9})
	if err != nil {
		t.Fatal(err)
	}
	initBack, err := g_v0_init.MarkerFromProto(initMsg)
	if err != nil || initBack.Id != 9 {
		t.Fatalf("init roundtrip mismatch: %+v, %v", initBack, err)
	}
	docMsg, err := g_v0_doc.NoteToProto(p_v0_doc.Note{Body: "clean"})
	if err != nil {
		t.Fatal(err)
	}
	docBack, err := g_v0_doc.NoteFromProto(docMsg)
	if err != nil || docBack.Body != "clean" {
		t.Fatalf("doc roundtrip mismatch: %+v, %v", docBack, err)
	}

	// Canonical-owner forwarding: v1 dep Tag is an exact type alias of v0 dep Tag!
	var sharedTag p_dep_v1.Tag = p_dep_v0.Tag{Id: 7, Label: "shared"}
	depV0Msg, err := g_dep_v0.TagToProto(sharedTag)
	if err != nil {
		t.Fatal(err)
	}
	depV1Msg, err := g_dep_v1.TagToProto(sharedTag)
	if err != nil {
		t.Fatal(err)
	}
	if !proto.Equal(depV0Msg, depV1Msg) {
		t.Fatalf("v0 and v1 canonical-owner dep messages diverged")
	}

	// v0.2 item with protoc-gen-go reserved field names (`string`, `reset`, `marshal`, `descriptor`)
	// + optional nil vs empty bytes distinction!
	v0Item := p_v0_main.Item{
		String:     "hello",
		Reset:      42,
		Marshal:    true,
		Descriptor: [2]uint16{10, 20},
		Tag:        sharedTag,
		MaybeTags:  ptr([]p_dep_v0.Tag{{Id: 1, Label: "a"}}),
		KwEntry:    p_kw.Entry{Code: 55},
		Raw:        []byte{9, 8, 7},
		AddedInV02: ptr(uint16(321)),
	}
	pbItem, err := g_v0_main.ItemToProto(v0Item)
	if err != nil {
		t.Fatal(err)
	}
	rawBin, err := proto.Marshal(pbItem)
	if err != nil {
		t.Fatal(err)
	}
	rtPb := &pb_v0.MainItem{}
	if err := proto.Unmarshal(rawBin, rtPb); err != nil {
		t.Fatal(err)
	}
	rtItem, err := g_v0_main.ItemFromProto(rtPb)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(rtItem, v0Item) {
		t.Fatalf("v0 item binary roundtrip mismatch:\n got: %#v\nwant: %#v", rtItem, v0Item)
	}
	jsonBytes, err := protojson.Marshal(pbItem)
	if err != nil {
		t.Fatal(err)
	}
	rtPbJSON := &pb_v0.MainItem{}
	if err := protojson.Unmarshal(jsonBytes, rtPbJSON); err != nil {
		t.Fatal(err)
	}
	rtItemJSON, err := g_v0_main.ItemFromProto(rtPbJSON)
	if err != nil || !reflect.DeepEqual(rtItemJSON, v0Item) {
		t.Fatalf("v0 item JSON roundtrip mismatch: %#v, err=%v", rtItemJSON, err)
	}

	// v1 and v2 items (v2 uses divergent dep with u16 Id + Extra bool).
	v1Item := p_v1_main.Item{
		String:    "v1",
		Tag:       sharedTag,
		MaybeTags: ptr([]p_dep_v1.Tag{{Id: 2, Label: "b"}}),
		KwEntry:   p_kw.Entry{Code: 66},
	}
	pbV1, err := g_v1_main.ItemToProto(v1Item)
	if err != nil {
		t.Fatal(err)
	}
	rawV1, _ := proto.Marshal(pbV1)
	rtPbV1 := &pb_v1.MainItem{}
	_ = proto.Unmarshal(rawV1, rtPbV1)
	rtV1, err := g_v1_main.ItemFromProto(rtPbV1)
	if err != nil || !reflect.DeepEqual(rtV1, v1Item) {
		t.Fatalf("v1 roundtrip mismatch: %#v, %v", rtV1, err)
	}

	v2Item := p_v2_main.Item{
		String:    "v2",
		Tag:       p_dep_v2.Tag{Id: 500, Label: "v2", Extra: true},
		MaybeTags: nil,
		KwEntry:   p_kw.Entry{Code: 88},
	}
	pbV2, err := g_v2_main.ItemToProto(v2Item)
	if err != nil {
		t.Fatal(err)
	}
	rawV2, _ := proto.Marshal(pbV2)
	rtPbV2 := &pb_v2.MainItem{}
	_ = proto.Unmarshal(rawV2, rtPbV2)
	rtV2, err := g_v2_main.ItemFromProto(rtPbV2)
	if err != nil || !reflect.DeepEqual(rtV2, v2Item) {
		t.Fatalf("v2 roundtrip mismatch: %#v, %v", rtV2, err)
	}
	_ = g_dep_v2.TagToProto

	// Triple option<option<option<u8>>> + present-empty-bytes (`&[]byte{}`) vs absent (`nil`)!
	tripleStates := []p_v0_main.OptTriple{
		p_wt.None[p_wt.Option[*uint8]](),
		p_wt.Some(p_wt.None[*uint8]()),
		p_wt.Some(p_wt.Some[*uint8](nil)),
		p_wt.Some(p_wt.Some(ptr(uint8(42)))),
	}
	for _, bt := range tripleStates {
		emptySlice := []byte{}
		var nilSlice []byte // nil inside *[]byte must still encode as present empty bytes `[]byte{}`!
		holder := p_v0_main.Holder{
			A:  &emptySlice,
			Al: ptr([]uint16{10, 20, 30}),
			Af: ptr([2]uint16{7, 8}),
			B:  p_wt.Some[*uint16](nil),
			Bt: bt,
			C:  p_v0_main.TrickyString{Value: 9},
			D:  ptr(map[uint8]string{7: "seven"}),
			G:  p_wt.Some(&nilSlice),
			H:  []p_v0_main.OptBytes{nil, &emptySlice},
			Hn: []p_v0_main.OptNested{p_wt.None[*uint16](), p_wt.Some[*uint16](nil), p_wt.Some(ptr(uint16(77)))},
			M:  map[uint8][4]byte{1: {10, 20, 30, 40}},
		}
		pbH, err := g_v0_main.HolderToProto(holder)
		if err != nil {
			t.Fatalf("HolderToProto failed: %v", err)
		}
		binH, _ := proto.Marshal(pbH)
		rtPbH := &pb_v0.MainHolder{}
		_ = proto.Unmarshal(binH, rtPbH)
		rtH, err := g_v0_main.HolderFromProto(rtPbH)
		if err != nil {
			t.Fatalf("HolderFromProto binary failed: %v", err)
		}
		if rtH.A == nil || len(*rtH.A) != 0 {
			t.Fatalf("expected present empty []byte for A, got %#v", rtH.A)
		}
		if !rtH.G.Some || rtH.G.Value == nil || len(*rtH.G.Value) != 0 {
			t.Fatalf("expected Some(&[]byte{}) for G, got %#v", rtH.G)
		}
		if !reflect.DeepEqual(rtH.Bt, bt) {
			t.Fatalf("triple option roundtrip mismatch: got %#v, want %#v", rtH.Bt, bt)
		}
		jsonH, err := protojson.Marshal(pbH)
		if err != nil {
			t.Fatal(err)
		}
		rtPbHJSON := &pb_v0.MainHolder{}
		if err := protojson.Unmarshal(jsonH, rtPbHJSON); err != nil {
			t.Fatal(err)
		}
		rtHJSON, err := g_v0_main.HolderFromProto(rtPbHJSON)
		if err != nil || !reflect.DeepEqual(rtHJSON.Bt, bt) {
			t.Fatalf("triple option JSON roundtrip mismatch: %#v, err=%v", rtHJSON.Bt, err)
		}

		// Function Call/Ping request & response helpers.
		reqMsg, err := g_v0_main.CallToRequest(v0Item, holder, "label")
		if err != nil {
			t.Fatal(err)
		}
		gotItem, gotHolder, gotType, err := g_v0_main.CallFromRequest(reqMsg)
		if err != nil || !reflect.DeepEqual(gotItem, v0Item) || !reflect.DeepEqual(gotHolder.Bt, bt) || gotType != "label" {
			t.Fatalf("CallFromRequest mismatch: err=%v", err)
		}
		respMsg, err := g_v0_main.CallToResponse(&v0Item)
		if err != nil {
			t.Fatal(err)
		}
		gotResp, err := g_v0_main.CallFromResponse(respMsg)
		if err != nil || gotResp == nil || !reflect.DeepEqual(*gotResp, v0Item) {
			t.Fatalf("CallFromResponse mismatch: %#v, err=%v", gotResp, err)
		}
	}

	pingReq, err := g_v0_main.PingToRequest()
	if err != nil || g_v0_main.PingFromRequest(pingReq) != nil {
		t.Fatalf("Ping request roundtrip failed")
	}
	pingResp, err := g_v0_main.PingToResponse()
	if err != nil || g_v0_main.PingFromResponse(pingResp) != nil {
		t.Fatalf("Ping response roundtrip failed")
	}

	// Deterministic sorted-key map validation: key 300 (IntegerOutOfRange) runs BEFORE value length (InvalidLength)!
	badPbH := &pb_v0.MainHolder{
		C: &pb_v0.MainTricky{WitforgeValue: &pb_v0.MainTricky_Reset_{Reset_: &pb_v0.Empty{}}},
		M: map[uint32][]byte{
			300: {1, 2}, // out-of-range key AND wrong length value
			500: {1, 2},
		},
	}
	_, err = g_v0_main.HolderFromProto(badPbH)
	var ge *g_err.GlueError
	if !errors.As(err, &ge) || ge.Kind != g_err.KindIntegerOutOfRange || ge.Field != "main/holder.m" || ge.Value != 300 {
		t.Fatalf("expected deterministic sorted-key IntegerOutOfRange(300) before InvalidLength, got %#v (%v)", ge, err)
	}
}
EOF

(
    cd "${EDGE}"
    "${GO_BIN}" build ./...
    "${GO_BIN}" vet ./...
    "${GO_BIN}" test -v ./...
)
