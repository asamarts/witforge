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
export GOTOOLCHAIN=local

if ! command -v "${PROTOC_BIN}" >/dev/null 2>&1; then
    echo "error: protoc not found (set PROTOC or install protoc)" >&2
    exit 1
fi
if ! command -v "${GO_BIN}" >/dev/null 2>&1; then
    echo "error: go not found (set GO or install Go 1.22+)" >&2
    exit 1
fi
go_ver="$("${GO_BIN}" env GOVERSION)"
go_minor=0
if [[ "${go_ver}" =~ ^go([0-9]+)\.([0-9]+) ]]; then
    if (( BASH_REMATCH[1] > 1 )); then
        go_minor=999
    elif (( BASH_REMATCH[1] == 1 )); then
        go_minor="${BASH_REMATCH[2]}"
    fi
fi
if (( go_minor < 22 )); then
    echo "error: Go 1.22+ is required (found ${go_ver})" >&2
    exit 1
fi
if [[ -n "${GOFMT:-}" ]]; then
    GOFMT_BIN="${GOFMT}"
elif [[ "${GO_BIN}" == */* && -x "$(dirname "${GO_BIN}")/gofmt" ]]; then
    GOFMT_BIN="$(dirname "${GO_BIN}")/gofmt"
else
    GOFMT_BIN="gofmt"
fi
if ! command -v "${GOFMT_BIN}" >/dev/null 2>&1; then
    echo "error: gofmt not found (set GOFMT or install Go 1.22+)" >&2
    exit 1
fi

harness_gofmt_diff="$("${GOFMT_BIN}" -d "${HARNESS_DIR}")"
if [[ -n "${harness_gofmt_diff}" ]]; then
    echo "gofmt diff on checked-in Go conformance harness:" >&2
    echo "${harness_gofmt_diff}" >&2
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

export GOCACHE="${GOCACHE:-${WORK}/gocache}"
mkdir -p "${GOCACHE}"

# Build pinned `protoc-gen-go` from checksum-locked `ci/go-conformance/go.mod` / `go.sum`.
BIN_DIR="${WORK}/bin"
mkdir -p "${BIN_DIR}"
(
    cd "${HARNESS_DIR}"
    "${GO_BIN}" build -mod=readonly -o "${BIN_DIR}/protoc-gen-go" google.golang.org/protobuf/cmd/protoc-gen-go
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

gofmt_diff="$("${GOFMT_BIN}" -d "${CONF}/plain" "${CONF}/glue")"
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
    "${GO_BIN}" build -mod=readonly ./...
    "${GO_BIN}" vet -mod=readonly -tags witforge_conformance ./...
    WITFORGE_CONFORMANCE_BUNDLE="${CONF}/bundle" "${GO_BIN}" test -mod=readonly -count=1 -tags witforge_conformance -v ./...
)

# ---------------------------------------------------------------------------
# 2. Multi-minor + multi-major + canonical-owner forwarding + disambiguated deps
#    + special package names + protoc-gen-go reserved fields + triple option
#    + deterministic sorted-key map error order + optional bytes presence
# ---------------------------------------------------------------------------
EDGE="${WORK}/edge"
mkdir -p \
    "${EDGE}/wit/v0.1/deps/base" "${EDGE}/wit/v0.1/deps/shared" "${EDGE}/wit/v0.1/deps/kw" \
    "${EDGE}/wit/v0.2/deps/base" "${EDGE}/wit/v0.2/deps/shared" "${EDGE}/wit/v0.2/deps/kw" \
    "${EDGE}/wit/v1.0/deps/base" "${EDGE}/wit/v1.0/deps/shared" "${EDGE}/wit/v1.0/deps/kw" \
    "${EDGE}/wit/v2.0/deps/base" "${EDGE}/wit/v2.0/deps/shared" "${EDGE}/wit/v2.0/deps/kw" \
    "${EDGE}/proto"
cp "${HARNESS_DIR}/go.mod" "${HARNESS_DIR}/go.sum" "${EDGE}/"

# Transitive dependency `acme:base@0.1.0` used by `acme:shared@0.1.0`.
for ver_dir in v0.1 v0.2 v1.0 v2.0; do
    cat >"${EDGE}/wit/${ver_dir}/deps/base/base.wit" <<'EOF'
package acme:base@0.1.0;

interface core {
    enum level {
        low,
        high,
    }
    type opt-level = option<level>;
}
EOF
done

for ver_dir in v0.1 v0.2 v1.0; do
    cat >"${EDGE}/wit/${ver_dir}/deps/shared/dep.wit" <<'EOF'
package acme:shared@0.1.0;

interface core {
    use acme:base/core@0.1.0.{level, opt-level};

    record tag {
        id: u8,
        label: string,
        lvl: level,
    }
    type maybe-tags = option<list<tag>>;
    type byte-alias = u8;
    type foreign-bytes = list<byte-alias>;
    type opt-lvl-alias = opt-level;

    check-tag: func(t: tag, extra: opt-level) -> result<tag, string>;
}
EOF
done

cat >"${EDGE}/wit/v2.0/deps/shared/dep.wit" <<'EOF'
package acme:shared@0.1.0;

interface core {
    use acme:base/core@0.1.0.{level, opt-level};

    record tag {
        id: u16,
        label: string,
        lvl: level,
        extra: bool,
    }
    type maybe-tags = option<list<tag>>;
    type byte-alias = u8;
    type foreign-bytes = list<byte-alias>;
    type opt-lvl-alias = opt-level;

    check-tag: func(t: tag, extra: opt-level) -> result<tag, string>;
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

interface enum-only {
    enum mode {
        fast,
        safe,
    }
}

interface flags-only {
    flags bits {
        %string,
        reset,
        read,
    }
}

interface fn-only {
    noop: func();
}

interface alias-only {
    use enum-only.{mode};
    type mode-alias = mode;
}

interface shadow {
    record error {
        %string: string,
        len: u8,
        make: bool,
        copy: u16,
        append: list<u8>,
        byte: u8,
        rune: char,
        cmp: s32,
        slices: list<string>,
        utf8: string,
        proto: u32,
    }
    check: func(error: error, %string: string, len: u8, make: bool, copy: u16, byte: u8, rune: char) -> error;
}

interface internal {
    record box {
        id: u8,
    }
}

interface testdata {
    record sample {
        id: u8,
    }
}

interface vendor {
    record dep {
        id: u8,
    }
}

interface foo-test {
    record case-rec {
        id: u8,
    }
}

interface con {
    record dev {
        id: u8,
    }
}

interface main {
    use acme:base/core@0.1.0.{opt-level};
    use acme:shared/core@0.1.0.{tag, maybe-tags, foreign-bytes};
    use %func:select/main@0.1.0.{entry};
    use enum-only.{mode};
    use flags-only.{bits};

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

    type item-alias = item;
    type my-u8 = u8;
    type hash = list<u8, 4>;
    type opt-bytes = option<list<u8>>;
    type opt-u16-list = option<list<u16>>;
    type opt-of-opt-list = option<opt-u16-list>;
    type opt-fixed-u16s = option<list<u16, 2>>;
    type opt-nested = option<option<u16>>;
    type opt-triple = option<option<option<u8>>>;
    type opt-of-named = option<opt-bytes>;
    type opt-map = option<map<u8, string>>;
    type opt-opt-level = option<opt-level>;

    type res-both = result<u8, mode>;
    type res-ok = result<u8>;
    type res-err = result<_, mode>;
    type res-bare = result;

    variant tricky {
        %string(u8),
        reset,
    }

    record synth-oneof {
        a: u32,
        x-a: u32,
        get-x-a: u32,
        get-a: u32,
        aliased-u8: my-u8,
        opt-tag: option<tag>,
        opt-hash: option<hash>,
        opt-opt-tag: option<option<tag>>,
    }

    record holder {
        a: opt-bytes,
        al: opt-u16-list,
        aol: opt-of-opt-list,
        af: opt-fixed-u16s,
        b: opt-nested,
        bt: opt-triple,
        c: tricky,
        d: opt-map,
        g: opt-of-named,
        h: list<opt-bytes>,
        hn: list<opt-nested>,
        m: map<u8, list<u8, 4>>,
        char-modes: map<char, mode>,
        perms: bits,
        rb: res-both,
        ro: res-ok,
        re: res-err,
        rn: res-bare,
        inline-res: list<result<u8, string>>,
        aliased-item: item-alias,
    }

    call: func(it: item, h: holder, %type: string) -> option<item>;
    synth-rpc: func(a: u32, x-a: u32) -> synth-oneof;
    ping: func();
}

interface consumer {
    use main.{opt-u16-list, opt-opt-level};

    type opt3-foreign = option<opt-opt-level>;

    record wrap {
        xs: list<opt-u16-list>,
        nested: option<opt-u16-list>,
        f3: opt3-foreign,
    }
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

edge_gofmt_diff="$("${GOFMT_BIN}" -d "${EDGE}/plain" "${EDGE}/glue")"
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
	g_v0_alias "example.com/witforge/go-conformance/glue/v0/alias_only"
	g_v0_con "example.com/witforge/go-conformance/glue/v0/con_"
	g_v0_consumer "example.com/witforge/go-conformance/glue/v0/consumer"
	g_v0_doc "example.com/witforge/go-conformance/glue/v0/documentation_"
	_ "example.com/witforge/go-conformance/glue/v0/empty_iface"
	g_v0_enum "example.com/witforge/go-conformance/glue/v0/enum_only"
	g_v0_flags "example.com/witforge/go-conformance/glue/v0/flags_only"
	g_v0_fn "example.com/witforge/go-conformance/glue/v0/fn_only"
	g_v0_footest "example.com/witforge/go-conformance/glue/v0/foo_test_"
	g_v0_init "example.com/witforge/go-conformance/glue/v0/init_"
	g_v0_internal "example.com/witforge/go-conformance/glue/v0/internal_"
	g_v0_main "example.com/witforge/go-conformance/glue/v0/main_"
	g_v0_shadow "example.com/witforge/go-conformance/glue/v0/shadow"
	g_v0_testdata "example.com/witforge/go-conformance/glue/v0/testdata_"
	g_v0_vendor "example.com/witforge/go-conformance/glue/v0/vendor_"
	g_v1_main "example.com/witforge/go-conformance/glue/v1/main_"
	g_v2_main "example.com/witforge/go-conformance/glue/v2/main_"
	g_dep_v0 "example.com/witforge/go-conformance/glue/witforge_deps/v0/acme_shared/core"
	g_dep_v1 "example.com/witforge/go-conformance/glue/witforge_deps/v1/acme_shared/core"
	g_dep_v2 "example.com/witforge/go-conformance/glue/witforge_deps/v2/acme_shared/core"
	p_v0_con "example.com/witforge/go-conformance/plain/v0_2/con_"
	p_v0_consumer "example.com/witforge/go-conformance/plain/v0_2/consumer"
	p_v0_doc "example.com/witforge/go-conformance/plain/v0_2/documentation_"
	p_v0_enum "example.com/witforge/go-conformance/plain/v0_2/enum_only"
	p_v0_flags "example.com/witforge/go-conformance/plain/v0_2/flags_only"
	p_v0_footest "example.com/witforge/go-conformance/plain/v0_2/foo_test_"
	p_v0_init "example.com/witforge/go-conformance/plain/v0_2/init_"
	p_v0_internal "example.com/witforge/go-conformance/plain/v0_2/internal_"
	p_v0_main "example.com/witforge/go-conformance/plain/v0_2/main_"
	p_v0_shadow "example.com/witforge/go-conformance/plain/v0_2/shadow"
	p_v0_testdata "example.com/witforge/go-conformance/plain/v0_2/testdata_"
	p_v0_vendor "example.com/witforge/go-conformance/plain/v0_2/vendor_"
	p_v1_main "example.com/witforge/go-conformance/plain/v1_0/main_"
	p_v2_main "example.com/witforge/go-conformance/plain/v2_0/main_"
	p_base "example.com/witforge/go-conformance/plain/witforge_deps/acme_base/core"
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

func mustMarshal(t *testing.T, msg proto.Message) []byte {
	t.Helper()
	b, err := proto.Marshal(msg)
	if err != nil {
		t.Fatalf("proto.Marshal failed: %v", err)
	}
	return b
}

func mustUnmarshal(t *testing.T, raw []byte, msg proto.Message) {
	t.Helper()
	if err := proto.Unmarshal(raw, msg); err != nil {
		t.Fatalf("proto.Unmarshal failed: %v", err)
	}
}

func assertGlueErr(t *testing.T, label string, err error, kind g_err.ErrorKind) *g_err.GlueError {
	t.Helper()
	var ge *g_err.GlueError
	if !errors.As(err, &ge) || ge.Kind != kind {
		t.Fatalf("%s: expected *GlueError(%s), got %#v (%v)", label, kind, ge, err)
	}
	return ge
}

func TestEdgeContractsAndCanonicalDeps(t *testing.T) {
	// Ensure init_, documentation_, internal_, testdata_, vendor_, foo_test_, con_,
	// enum_only, flags_only, fn_only, alias_only, and shadow packages compile and round-trip.
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
	if bMsg, err := g_v0_internal.BoxToProto(p_v0_internal.Box{Id: 1}); err != nil {
		t.Fatal(err)
	} else if bBack, err := g_v0_internal.BoxFromProto(bMsg); err != nil || bBack.Id != 1 {
		t.Fatalf("internal_ mismatch: %+v, %v", bBack, err)
	}
	if sMsg, err := g_v0_testdata.SampleToProto(p_v0_testdata.Sample{Id: 2}); err != nil {
		t.Fatal(err)
	} else if sBack, err := g_v0_testdata.SampleFromProto(sMsg); err != nil || sBack.Id != 2 {
		t.Fatalf("testdata_ mismatch: %+v, %v", sBack, err)
	}
	if vMsg, err := g_v0_vendor.DepToProto(p_v0_vendor.Dep{Id: 3}); err != nil {
		t.Fatal(err)
	} else if vBack, err := g_v0_vendor.DepFromProto(vMsg); err != nil || vBack.Id != 3 {
		t.Fatalf("vendor_ mismatch: %+v, %v", vBack, err)
	}
	if ftMsg, err := g_v0_footest.CaseRecToProto(p_v0_footest.CaseRec{Id: 4}); err != nil {
		t.Fatal(err)
	} else if ftBack, err := g_v0_footest.CaseRecFromProto(ftMsg); err != nil || ftBack.Id != 4 {
		t.Fatalf("foo_test_ mismatch: %+v, %v", ftBack, err)
	}
	if cMsg, err := g_v0_con.DevToProto(p_v0_con.Dev{Id: 5}); err != nil {
		t.Fatal(err)
	} else if cBack, err := g_v0_con.DevFromProto(cMsg); err != nil || cBack.Id != 5 {
		t.Fatalf("con_ mismatch: %+v, %v", cBack, err)
	}

	// Minimal interfaces: enum-only (including high-bit uint32 >= 0x80000000 int64 preservation),
	// flags-only, fn-only, alias-only, and Go predeclared-identifier shadowing (`shadow`).
	mPb, err := g_v0_alias.ModeAliasToProto(p_v0_enum.ModeSafe)
	if err != nil {
		t.Fatal(err)
	}
	if mBack, err := g_v0_alias.ModeAliasFromProto(mPb); err != nil || mBack != p_v0_enum.ModeSafe {
		t.Fatalf("alias-only mode mismatch: %v, %v", mBack, err)
	}
	_, err = g_v0_enum.ModeToProto(p_v0_enum.Mode(0x80000000))
	geHi := assertGlueErr(t, "high-bit enum", err, g_err.KindUnknownEnumValue)
	if geHi.Enumeration != "enum-only/mode" || geHi.Value != 0x80000000 {
		t.Fatalf("expected int64 0x80000000 without int32 wrap, got %+v", geHi)
	}
	fPb, err := g_v0_flags.BitsToProto(p_v0_flags.Bits{String: true, Reset: false, Read: true})
	if err != nil {
		t.Fatal(err)
	}
	if fBack, err := g_v0_flags.BitsFromProto(fPb); err != nil || !fBack.String || fBack.Reset || !fBack.Read {
		t.Fatalf("flags-only roundtrip mismatch: %+v, %v", fBack, err)
	}
	if nReq, err := g_v0_fn.NoopToRequest(); err != nil || g_v0_fn.NoopFromRequest(nReq) != nil {
		t.Fatalf("fn-only Noop request failed")
	}
	if nResp, err := g_v0_fn.NoopToResponse(); err != nil || g_v0_fn.NoopFromResponse(nResp) != nil {
		t.Fatalf("fn-only Noop response failed")
	}
	if g_v0_fn.MethodNoop != pb_v0.FnOnlyMethod_FN_ONLY_METHOD_NOOP {
		t.Fatalf("MethodNoop mismatch")
	}
	shadowVal := p_v0_shadow.Error{
		String: "s", Len: 3, Make: true, Copy: 7, Append: []byte{1, 2},
		Byte: 9, Rune: 'x', Cmp: -4, Slices: []string{"a"}, Utf8: "ok", Proto: 11,
	}
	shReq, err := g_v0_shadow.CheckToRequest(shadowVal, "a", 1, true, 2, 3, 'z')
	if err != nil {
		t.Fatal(err)
	}
	gotSh, gotS, gotL, gotM, gotC, gotB, gotR, err := g_v0_shadow.CheckFromRequest(shReq)
	if err != nil || !reflect.DeepEqual(gotSh, shadowVal) || gotS != "a" || gotL != 1 || !gotM || gotC != 2 || gotB != 3 || gotR != 'z' {
		t.Fatalf("shadow request mismatch: %v", err)
	}

	// Canonical-owner forwarding (types AND functions AND transitive `acme:base` dep):
	// v1 dep Tag is an exact type alias of v0 dep Tag!
	var sharedTag p_dep_v1.Tag = p_dep_v0.Tag{Id: 7, Label: "shared", Lvl: p_base.LevelHigh}
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
	if depV1Back, err := g_dep_v1.TagFromProto(depV1Msg); err != nil || !reflect.DeepEqual(depV1Back, sharedTag) {
		t.Fatalf("g_dep_v1.TagFromProto mismatch: %+v, %v", depV1Back, err)
	}
	depFnReq, err := g_dep_v1.CheckTagToRequest(sharedTag, ptr(p_base.LevelLow))
	if err != nil {
		t.Fatal(err)
	}
	gotDepTag, gotDepLvl, err := g_dep_v1.CheckTagFromRequest(depFnReq)
	if err != nil || !reflect.DeepEqual(gotDepTag, sharedTag) || gotDepLvl == nil || *gotDepLvl != p_base.LevelLow {
		t.Fatalf("canonical-owner dep CheckTagFromRequest mismatch: %v", err)
	}
	depFnResp, err := g_dep_v1.CheckTagToResponse(p_wt.NewOk[p_dep_v1.Tag, string](sharedTag))
	if err != nil {
		t.Fatal(err)
	}
	if gotDepResp, err := g_dep_v1.CheckTagFromResponse(depFnResp); err != nil || !gotDepResp.IsOk || !reflect.DeepEqual(gotDepResp.Ok, sharedTag) {
		t.Fatalf("canonical-owner dep CheckTagFromResponse mismatch: %+v, %v", gotDepResp, err)
	}
	v2Tag := p_dep_v2.Tag{Id: 500, Label: "v2", Lvl: p_base.LevelLow, Extra: true}
	depV2Msg, err := g_dep_v2.TagToProto(v2Tag)
	if err != nil {
		t.Fatal(err)
	}
	if depV2Back, err := g_dep_v2.TagFromProto(depV2Msg); err != nil || !reflect.DeepEqual(depV2Back, v2Tag) {
		t.Fatalf("g_dep_v2.TagFromProto mismatch: %+v, %v", depV2Back, err)
	}

	// Critical #3: proto3 `optional` synthetic oneof (`a`, `x-a`, `get-x-a`, `get-a`) in both
	// record and function request + local alias identity (`main/synth-oneof.aliased-u8`) +
	// `option<tag>` without unused `wf_pb_acme_shared` import + `option<hash>`.
	synthIn := p_v0_main.SynthOneof{
		A:         11,
		XA:        22,
		GetXA:     33,
		GetA:      44,
		AliasedU8: 55,
		OptTag:    &sharedTag,
		OptHash:   ptr([4]byte{1, 2, 3, 4}),
		OptOptTag: p_wt.Some(&sharedTag),
	}
	synthPb, err := g_v0_main.SynthOneofToProto(synthIn)
	if err != nil {
		t.Fatal(err)
	}
	rtSynthPb := &pb_v0.MainSynthOneof{}
	mustUnmarshal(t, mustMarshal(t, synthPb), rtSynthPb)
	if rtSynth, err := g_v0_main.SynthOneofFromProto(rtSynthPb); err != nil || !reflect.DeepEqual(rtSynth, synthIn) {
		t.Fatalf("SynthOneof roundtrip mismatch: got %#v, want %#v, err=%v", rtSynth, synthIn, err)
	}
	// Verify local alias `aliased-u8: my-u8` reports `"main/synth-oneof.aliased-u8"` on overflow.
	rtSynthPb.AliasedU8 = ptr(uint32(300))
	_, err = g_v0_main.SynthOneofFromProto(rtSynthPb)
	geAlias := assertGlueErr(t, "local alias field identity", err, g_err.KindIntegerOutOfRange)
	if geAlias.Field != "main/synth-oneof.aliased-u8" || geAlias.Value != 300 {
		t.Fatalf("unexpected local alias error: %+v", geAlias)
	}
	// Verify `option<option<tag>>` with `Some{Some: nil}` returns `InvalidState` (not `MissingField`).
	rtSynthPb.AliasedU8 = ptr(uint32(55))
	rtSynthPb.OptOptTag = &pb_v0.WitforgeOptionOfAcmeSharedCoreTag{
		Value: &pb_v0.WitforgeOptionOfAcmeSharedCoreTag_Some{Some: nil},
	}
	_, err = g_v0_main.SynthOneofFromProto(rtSynthPb)
	geOptMsgNil := assertGlueErr(t, "option<option<msg>> Some(nil)", err, g_err.KindInvalidState)
	if geOptMsgNil.Message != "main/synth-oneof.opt-opt-tag" {
		t.Fatalf("unexpected Some(nil) error: %+v", geOptMsgNil)
	}
	// Verify `option` `None{None: nil}` returns `InvalidState`.
	rtSynthPb.OptOptTag = &pb_v0.WitforgeOptionOfAcmeSharedCoreTag{
		Value: &pb_v0.WitforgeOptionOfAcmeSharedCoreTag_None{None: nil},
	}
	_, err = g_v0_main.SynthOneofFromProto(rtSynthPb)
	assertGlueErr(t, "option None(nil)", err, g_err.KindInvalidState)

	// SynthRpc function request synthetic oneof (`a`, `x-a`).
	srPb, err := g_v0_main.SynthRpcToRequest(101, 202)
	if err != nil {
		t.Fatal(err)
	}
	rtSrPb := &pb_v0.MainSynthRpcRequest{}
	mustUnmarshal(t, mustMarshal(t, srPb), rtSrPb)
	if gotA, gotXA, err := g_v0_main.SynthRpcFromRequest(rtSrPb); err != nil || gotA != 101 || gotXA != 202 {
		t.Fatalf("SynthRpc request mismatch: (%d, %d), err=%v", gotA, gotXA, err)
	}

	// Critical #1, #2, #4: named `option<opt-u16-list>`, cross-interface `use main.{opt-u16-list}`
	// in `list<opt-u16-list>` and `option<opt-u16-list>`, and `option<opt-opt-level>` (no unused dep import).
	lvlLow := p_base.LevelLow
	wrapIn := p_v0_consumer.Wrap{
		Xs:     []p_v0_main.OptU16List{nil, ptr([]uint16{1, 2})},
		Nested: p_wt.Some(ptr([]uint16{3, 4})),
		F3:     p_wt.Some(p_wt.Some(&lvlLow)),
	}
	wrapPb, err := g_v0_consumer.WrapToProto(wrapIn)
	if err != nil {
		t.Fatal(err)
	}
	rtWrapPb := &pb_v0.ConsumerWrap{}
	mustUnmarshal(t, mustMarshal(t, wrapPb), rtWrapPb)
	if rtWrap, err := g_v0_consumer.WrapFromProto(rtWrapPb); err != nil || !reflect.DeepEqual(rtWrap, wrapIn) {
		t.Fatalf("ConsumerWrap roundtrip mismatch:\n got: %#v\nwant: %#v\nerr: %v", rtWrap, wrapIn, err)
	}

	// v0.2 item with protoc-gen-go reserved fields + nil required `[]byte` preserving presence!
	v0Item := p_v0_main.Item{
		String:     "hello",
		Reset:      42,
		Marshal:    true,
		Descriptor: [2]uint16{10, 20},
		Tag:        sharedTag,
		MaybeTags:  ptr([]p_dep_v0.Tag{sharedTag}),
		KwEntry:    p_kw.Entry{Code: 55},
		Raw:        nil, // nil required []byte must round-trip as present non-nil []byte{}!
		AddedInV02: ptr(uint16(321)),
	}
	pbItem, err := g_v0_main.ItemToProto(v0Item)
	if err != nil {
		t.Fatal(err)
	}
	rtPb := &pb_v0.MainItem{}
	mustUnmarshal(t, mustMarshal(t, pbItem), rtPb)
	rtItem, err := g_v0_main.ItemFromProto(rtPb)
	if err != nil {
		t.Fatal(err)
	}
	if rtItem.Raw == nil || len(rtItem.Raw) != 0 {
		t.Fatalf("expected non-nil empty Raw bytes after nil encode, got %#v", rtItem.Raw)
	}
	v0Item.Raw = []byte{}
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
		MaybeTags: ptr([]p_dep_v1.Tag{sharedTag}),
		KwEntry:   p_kw.Entry{Code: 66},
	}
	pbV1, err := g_v1_main.ItemToProto(v1Item)
	if err != nil {
		t.Fatal(err)
	}
	rtPbV1 := &pb_v1.MainItem{}
	mustUnmarshal(t, mustMarshal(t, pbV1), rtPbV1)
	if rtV1, err := g_v1_main.ItemFromProto(rtPbV1); err != nil || !reflect.DeepEqual(rtV1, v1Item) {
		t.Fatalf("v1 roundtrip mismatch: %#v, %v", rtV1, err)
	}

	v2Item := p_v2_main.Item{
		String:    "v2",
		Tag:       v2Tag,
		MaybeTags: nil,
		KwEntry:   p_kw.Entry{Code: 88},
	}
	pbV2, err := g_v2_main.ItemToProto(v2Item)
	if err != nil {
		t.Fatal(err)
	}
	rtPbV2 := &pb_v2.MainItem{}
	mustUnmarshal(t, mustMarshal(t, pbV2), rtPbV2)
	if rtV2, err := g_v2_main.ItemFromProto(rtPbV2); err != nil || !reflect.DeepEqual(rtV2, v2Item) {
		t.Fatalf("v2 roundtrip mismatch: %#v, %v", rtV2, err)
	}

	// Triple option, named option<opt-u16-list>, all 4 result forms, unit variant, absent optionals,
	// wire-marshaled RPC helpers, and Method* constants.
	if g_v0_main.MethodCall != pb_v0.MainMethod_MAIN_METHOD_CALL ||
		g_v0_main.MethodSynthRpc != pb_v0.MainMethod_MAIN_METHOD_SYNTH_RPC ||
		g_v0_main.MethodPing != pb_v0.MainMethod_MAIN_METHOD_PING {
		t.Fatalf("Method* constants mismatch")
	}
	tripleStates := []p_v0_main.OptTriple{
		p_wt.None[p_wt.Option[*uint8]](),
		p_wt.Some(p_wt.None[*uint8]()),
		p_wt.Some(p_wt.Some[*uint8](nil)),
		p_wt.Some(p_wt.Some(ptr(uint8(42)))),
	}
	for idx, bt := range tripleStates {
		emptySlice := []byte{}
		var nilSlice []byte
		var cVal p_v0_main.Tricky = p_v0_main.TrickyString{Value: 9}
		var optA p_v0_main.OptBytes = &emptySlice
		var optAl p_v0_main.OptU16List = ptr([]uint16{10, 20, 30})
		var optAol p_v0_main.OptOfOptList = p_wt.Some(ptr([]uint16{99}))
		var optAf p_v0_main.OptFixedU16s = ptr([2]uint16{7, 8})
		var optD p_v0_main.OptMap = ptr(map[uint8]string{7: "seven"})
		var optG p_v0_main.OptOfNamed = p_wt.Some(&nilSlice)
		if idx == 0 {
			// Exercise absent optionals + unit variant case `TrickyReset{}`.
			cVal = p_v0_main.TrickyReset{}
			optA = nil
			optAl = nil
			optAol = p_wt.Some[p_v0_main.OptU16List](nil)
			optAf = nil
			optD = nil
			optG = p_wt.None[p_v0_main.OptBytes]()
		}
		holder := p_v0_main.Holder{
			A:           optA,
			Al:          optAl,
			Aol:         optAol,
			Af:          optAf,
			B:           p_wt.Some[*uint16](nil),
			Bt:          bt,
			C:           cVal,
			D:           optD,
			G:           optG,
			H:           []p_v0_main.OptBytes{nil, &emptySlice},
			Hn:          []p_v0_main.OptNested{p_wt.None[*uint16](), p_wt.Some[*uint16](nil), p_wt.Some(ptr(uint16(77)))},
			M:           map[uint8][4]byte{1: {10, 20, 30, 40}},
			CharModes:   map[rune]p_v0_enum.Mode{'a': p_v0_enum.ModeFast},
			Perms:       p_v0_flags.Bits{String: true, Read: true},
			Rb:          p_wt.NewOk[uint8, p_v0_enum.Mode](7),
			Ro:          p_wt.NewErr[uint8, struct{}](struct{}{}),
			Re:          p_wt.NewOk[struct{}, p_v0_enum.Mode](struct{}{}),
			Rn:          p_wt.NewOk[struct{}, struct{}](struct{}{}),
			InlineRes:   []p_wt.Result[uint8, string]{p_wt.NewOk[uint8, string](1), p_wt.NewErr[uint8, string]("no")},
			AliasedItem: v0Item,
		}
		pbH, err := g_v0_main.HolderToProto(holder)
		if err != nil {
			t.Fatalf("HolderToProto failed: %v", err)
		}
		rtPbH := &pb_v0.MainHolder{}
		mustUnmarshal(t, mustMarshal(t, pbH), rtPbH)
		rtH, err := g_v0_main.HolderFromProto(rtPbH)
		if err != nil {
			t.Fatalf("HolderFromProto binary failed: %v", err)
		}
		if idx == 0 {
			if rtH.A != nil || rtH.Al != nil || rtH.Af != nil || rtH.D != nil || rtH.G.Some {
				t.Fatalf("expected absent optionals on idx=0, got %#v", rtH)
			}
			if rtH.C != (p_v0_main.TrickyReset{}) {
				t.Fatalf("expected TrickyReset, got %#v", rtH.C)
			}
		} else {
			if rtH.A == nil || len(*rtH.A) != 0 {
				t.Fatalf("expected present empty []byte for A, got %#v", rtH.A)
			}
			if !rtH.G.Some || rtH.G.Value == nil || len(*rtH.G.Value) != 0 {
				t.Fatalf("expected Some(&[]byte{}) for G, got %#v", rtH.G)
			}
		}
		if !reflect.DeepEqual(rtH.Bt, bt) || !reflect.DeepEqual(rtH.Aol, optAol) {
			t.Fatalf("nested option roundtrip mismatch: got Bt=%#v Aol=%#v", rtH.Bt, rtH.Aol)
		}
		jsonH, err := protojson.Marshal(pbH)
		if err != nil {
			t.Fatal(err)
		}
		rtPbHJSON := &pb_v0.MainHolder{}
		if err := protojson.Unmarshal(jsonH, rtPbHJSON); err != nil {
			t.Fatal(err)
		}
		if rtHJSON, err := g_v0_main.HolderFromProto(rtPbHJSON); err != nil || !reflect.DeepEqual(rtHJSON.Bt, bt) {
			t.Fatalf("triple option JSON roundtrip mismatch: %#v, err=%v", rtHJSON.Bt, err)
		}

		// Wire-marshaled Function Call/Ping request & response helpers (both present & nil response).
		reqMsg, err := g_v0_main.CallToRequest(v0Item, holder, "label")
		if err != nil {
			t.Fatal(err)
		}
		rtReqPb := &pb_v0.MainCallRequest{}
		mustUnmarshal(t, mustMarshal(t, reqMsg), rtReqPb)
		gotItem, gotHolder, gotType, err := g_v0_main.CallFromRequest(rtReqPb)
		if err != nil || !reflect.DeepEqual(gotItem, v0Item) || !reflect.DeepEqual(gotHolder.Bt, bt) || gotType != "label" {
			t.Fatalf("CallFromRequest wire mismatch: err=%v", err)
		}
		var respArg *p_v0_main.Item
		if idx != 0 {
			respArg = &v0Item
		}
		respMsg, err := g_v0_main.CallToResponse(respArg)
		if err != nil {
			t.Fatal(err)
		}
		rtRespPb := &pb_v0.MainCallResponse{}
		mustUnmarshal(t, mustMarshal(t, respMsg), rtRespPb)
		gotResp, err := g_v0_main.CallFromResponse(rtRespPb)
		if err != nil {
			t.Fatal(err)
		}
		if idx == 0 && gotResp != nil {
			t.Fatalf("expected nil CallFromResponse, got %#v", gotResp)
		}
		if idx != 0 && (gotResp == nil || !reflect.DeepEqual(*gotResp, v0Item)) {
			t.Fatalf("CallFromResponse wire mismatch: %#v", gotResp)
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

	// Critical/High #6 & #7: in-memory nil `*pb.Empty` arms & nil inline `result` in `list<...>`.
	_, err = g_v0_main.TrickyFromProto(&pb_v0.MainTricky{
		WitforgeValue: &pb_v0.MainTricky_Reset_{Reset_: nil},
	})
	assertGlueErr(t, "nil Empty unit variant", err, g_err.KindUnknownCase)
	_, err = g_v0_main.ResBareFromProto(&pb_v0.MainResBare{
		Value: &pb_v0.MainResBare_Ok{Ok: nil},
	})
	assertGlueErr(t, "nil Empty result Ok", err, g_err.KindUnknownCase)
	_, err = g_v0_main.ResBareFromProto(&pb_v0.MainResBare{
		Value: &pb_v0.MainResBare_Err{Err: nil},
	})
	assertGlueErr(t, "nil Empty result Err", err, g_err.KindUnknownCase)

	nilResHolderPb := &pb_v0.MainHolder{
		C:           &pb_v0.MainTricky{WitforgeValue: &pb_v0.MainTricky_Reset_{Reset_: &pb_v0.Empty{}}},
		Perms:       &pb_v0.FlagsOnlyBits{},
		Rb:          &pb_v0.MainResBoth{Value: &pb_v0.MainResBoth_Ok{Ok: 1}},
		Ro:          &pb_v0.MainResOk{Value: &pb_v0.MainResOk_Err{Err: &pb_v0.Empty{}}},
		Re:          &pb_v0.MainResErr{Value: &pb_v0.MainResErr_Ok{Ok: &pb_v0.Empty{}}},
		Rn:          &pb_v0.MainResBare{Value: &pb_v0.MainResBare_Ok{Ok: &pb_v0.Empty{}}},
		InlineRes:   []*pb_v0.WitforgeResultOfU8AndString{nil},
		AliasedItem: pbItem,
	}
	_, err = g_v0_main.HolderFromProto(nilResHolderPb)
	geNilRes := assertGlueErr(t, "nil inline result in list", err, g_err.KindUnknownCase)
	if geNilRes.Message != "main/holder.inline-res" {
		t.Fatalf("unexpected nil inline result target: %+v", geNilRes)
	}

	// Deterministic sorted-key map validation on DECODE: key 300 runs BEFORE value length!
	badPbH := &pb_v0.MainHolder{
		C: &pb_v0.MainTricky{WitforgeValue: &pb_v0.MainTricky_Reset_{Reset_: &pb_v0.Empty{}}},
		M: map[uint32][]byte{
			300: {1, 2},
			500: {1, 2},
		},
	}
	_, err = g_v0_main.HolderFromProto(badPbH)
	geDec := assertGlueErr(t, "decode sorted-key map", err, g_err.KindIntegerOutOfRange)
	if geDec.Field != "main/holder.m" || geDec.Value != 300 {
		t.Fatalf("expected deterministic sorted-key IntegerOutOfRange(300), got %+v", geDec)
	}

	// Deterministic sorted-key map validation on ENCODE: key 0xD800 (smallest invalid rune)
	// runs BEFORE key 0xD900 and BEFORE invalid enum value 99!
	badEncHolder := p_v0_main.Holder{
		C: p_v0_main.TrickyReset{},
		CharModes: map[rune]p_v0_enum.Mode{
			0xD900: p_v0_enum.Mode(99),
			0xD800: p_v0_enum.Mode(88),
		},
		AliasedItem: v0Item,
	}
	for range 16 {
		_, err = g_v0_main.HolderToProto(badEncHolder)
		geEnc := assertGlueErr(t, "encode sorted-key map", err, g_err.KindInvalidChar)
		if geEnc.Field != "main/holder.char-modes" || geEnc.CodePoint != 0xD800 {
			t.Fatalf("expected deterministic encode sorted-key InvalidChar(0xD800), got %+v", geEnc)
		}
	}
}
EOF

(
    cd "${EDGE}"
    "${GO_BIN}" build -mod=readonly ./...
    "${GO_BIN}" vet -mod=readonly ./...
    "${GO_BIN}" test -mod=readonly -count=1 -v ./...
)
