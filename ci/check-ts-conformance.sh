#!/usr/bin/env bash
# End-to-end TypeScript checked protobuf glue conformance harness (`W13-003`).
#
# Generates the `W11-034` conformance bundle, emits plain TypeScript (`witforge ts`),
# compiles `.proto` files with `protoc` + `@bufbuild/protoc-gen-es`, emits checked
# TypeScript glue (`witforge ts-glue`), type-checks everything under strict `tsc`,
# and executes every binary, proto3-JSON, and encode-bounds conformance vector plus
# a multi-major disambiguated `deps/` and object-property edge-case smoke contract.
#
# Usage: [WITFORGE=path/to/witforge] [PROTOC=path/to/protoc] ci/check-ts-conformance.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HARNESS_DIR="${ROOT}/ci/ts-conformance"
PROTOC_BIN="${PROTOC:-protoc}"

if ! command -v "${PROTOC_BIN}" >/dev/null 2>&1; then
    echo "error: protoc not found (set PROTOC or install protoc)" >&2
    exit 1
fi
if ! command -v node >/dev/null 2>&1; then
    echo "error: node not found" >&2
    exit 1
fi

if [[ ! -d "${HARNESS_DIR}/node_modules/@bufbuild/protobuf" ]]; then
    npm --prefix "${HARNESS_DIR}" ci --ignore-scripts
fi

PROTOC_GEN_ES="${HARNESS_DIR}/node_modules/.bin/protoc-gen-es"
TSC_BIN="${HARNESS_DIR}/node_modules/.bin/tsc"

if [[ -n "${WITFORGE:-}" ]]; then
    RUN=("${WITFORGE}")
else
    cargo build --manifest-path "${ROOT}/Cargo.toml" --locked -p witforge-cli
    RUN=("${ROOT}/target/debug/witforge")
fi

scratch="${TMPDIR:-${HOME}/.tmp}"
mkdir -p "${scratch}"
WORK="$(mktemp -d "${scratch%/}/witforge-ts-conformance.XXXXXX")"
trap 'rm -rf "${WORK}"' EXIT

# ---------------------------------------------------------------------------
# 1. Canonical W11-034 conformance bundle (all binary + proto3-JSON + encode vectors)
# ---------------------------------------------------------------------------
CONF="${WORK}/conformance"
mkdir -p "${CONF}/src/proto"

"${RUN[@]}" conformance --out "${CONF}/bundle"
"${RUN[@]}" ts "${CONF}/bundle/wit" --out "${CONF}/src/plain"
"${RUN[@]}" ts "${CONF}/bundle/wit" --out "${CONF}/src/plain" --check
"${RUN[@]}" ts-glue "${CONF}/bundle/wit" --out "${CONF}/src/glue" \
    --plain-root ../plain --proto-root ../proto
"${RUN[@]}" ts-glue "${CONF}/bundle/wit" --out "${CONF}/src/glue" \
    --plain-root ../plain --proto-root ../proto --check

"${PROTOC_BIN}" \
    --plugin="protoc-gen-es=${PROTOC_GEN_ES}" \
    -I "${CONF}/bundle/proto" \
    --es_out="${CONF}/src/proto" \
    --es_opt=target=ts,import_extension=js \
    "${CONF}/bundle/proto/v0.proto" \
    "${CONF}/bundle/proto/witforge.conformance_dep.v0.proto"

cp "${HARNESS_DIR}/run.ts" "${CONF}/src/run.ts"
ln -s "${HARNESS_DIR}/node_modules" "${CONF}/node_modules"

cat >"${CONF}/package.json" <<'EOF'
{
  "type": "module"
}
EOF

cat >"${CONF}/tsconfig.json" <<'EOF'
{
  "compilerOptions": {
    "target": "ES2023",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "rootDir": "src",
    "outDir": "dist",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "exactOptionalPropertyTypes": true,
    "noImplicitOverride": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "verbatimModuleSyntax": true,
    "types": ["node"],
    "skipLibCheck": false
  },
  "include": ["src/**/*.ts"]
}
EOF

"${TSC_BIN}" --project "${CONF}/tsconfig.json"
node "${CONF}/dist/run.js" "${CONF}/bundle"

# ---------------------------------------------------------------------------
# 2. Multi-major + disambiguated deps + JS Object.prototype & Schema collision smoke
# ---------------------------------------------------------------------------
EDGE="${WORK}/edge"
mkdir -p "${EDGE}/wit/v0.1/deps/shared" "${EDGE}/wit/v1.0/deps/shared" "${EDGE}/src/proto"

cat >"${EDGE}/wit/v0.1/deps/shared/dep.wit" <<'EOF'
package acme:shared@0.1.0;

interface core {
    record tag {
        id: u8,
        label: string,
    }
    type maybe-tags = option<list<tag>>;
}
EOF

cat >"${EDGE}/wit/v0.1/contract.wit" <<'EOF'
package acme:edge@0.1.0;

interface types {
    use acme:shared/core@0.1.0.{tag, maybe-tags};

    record item {
        %constructor: u16,
        to-string: string,
        tag: tag,
        maybe-tags: maybe-tags,
    }

    record item-schema {
        id: u32,
    }

    flags tricky-flags {
        %constructor,
        to-string,
    }

    type opt-list = option<list<u8>>;
    type opt-nested = option<option<u16>>;
    type opt-map = option<map<u8, string>>;

    variant tricky {
        %constructor(u8),
        to-string,
    }

    record holder {
        a: opt-list,
        b: opt-nested,
        c: tricky,
        d: opt-map,
        e: tricky-flags,
        f: item-schema,
    }
}

interface api {
    use types.{item, holder};
    echo: func(%constructor: item, h: holder) -> option<item>;
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
}
EOF

cat >"${EDGE}/wit/v1.0/contract.wit" <<'EOF'
package acme:edge@1.0.0;

interface types {
    use acme:shared/core@0.1.0.{tag, maybe-tags};

    record item {
        %constructor: u32,
        to-string: string,
        tag: tag,
        maybe-tags: maybe-tags,
    }
}
EOF

"${RUN[@]}" lock "${EDGE}/wit/v0.1" --first >/dev/null
"${RUN[@]}" lock "${EDGE}/wit/v1.0" --first >/dev/null

"${RUN[@]}" proto "${EDGE}/wit" --out "${EDGE}/proto-src"
"${RUN[@]}" ts "${EDGE}/wit" --out "${EDGE}/src/plain"
"${RUN[@]}" ts-glue "${EDGE}/wit" --out "${EDGE}/src/glue" \
    --plain-root ../plain --proto-root ../proto
"${RUN[@]}" ts-glue "${EDGE}/wit" --out "${EDGE}/src/glue" \
    --plain-root ../plain --proto-root ../proto --check

shopt -s nullglob
proto_files=("${EDGE}/proto-src/"*.proto)
shopt -u nullglob
if [[ ${#proto_files[@]} -eq 0 ]]; then
    echo "error: no edge proto files generated" >&2
    exit 1
fi

"${PROTOC_BIN}" \
    --plugin="protoc-gen-es=${PROTOC_GEN_ES}" \
    -I "${EDGE}/proto-src" \
    --es_out="${EDGE}/src/proto" \
    --es_opt=target=ts,import_extension=js \
    "${proto_files[@]}"

cat >"${EDGE}/src/smoke.ts" <<'EOF'
import assert from "node:assert/strict";
import { fromBinary, toBinary } from "@bufbuild/protobuf";
import * as g from "./glue/index.js";
import { NONE, some } from "./plain/witforge_types.js";

const v0Item = {
  constructor_: 42,
  toString: "hello",
  tag: { id: 7, label: "v0" },
  maybeTags: some([{ id: 1, label: "a" }]),
};
const v0Bytes = toBinary(g.v0.types.item.schema, g.v0.types.item.toProto(v0Item));
const v0Back = g.v0.types.item.fromProto(fromBinary(g.v0.types.item.schema, v0Bytes));
assert.deepStrictEqual(v0Back, v0Item);

const v1Item = {
  constructor_: 99999,
  toString: "world",
  tag: { id: 500, label: "v1", extra: true },
  maybeTags: NONE,
};
const v1Bytes = toBinary(g.v1.types.item.schema, g.v1.types.item.toProto(v1Item));
const v1Back = g.v1.types.item.fromProto(fromBinary(g.v1.types.item.schema, v1Bytes));
assert.deepStrictEqual(v1Back, v1Item);

const holder = {
  a: some(Uint8Array.from([1, 2, 3])),
  b: some(NONE),
  c: { tag: "constructor" as const, value: 9 },
  d: some(new Map([[7, "seven"]])),
  e: { constructor_: true, toString: false },
  f: { id: 123 },
};
const reqWire = g.v0.api.functions.echo.toRequest(v0Item, holder);
const [reqItem, reqHolder] = g.v0.api.functions.echo.fromRequest(
  fromBinary(g.v0.api.functions.echo.requestSchema, toBinary(g.v0.api.functions.echo.requestSchema, reqWire)),
);
assert.deepStrictEqual(reqItem, v0Item);
assert.deepStrictEqual(reqHolder, holder);

const respWire = g.v0.api.functions.echo.toResponse(some(v0Item));
const respBack = g.v0.api.functions.echo.fromResponse(
  fromBinary(g.v0.api.functions.echo.responseSchema, toBinary(g.v0.api.functions.echo.responseSchema, respWire)),
);
assert.deepStrictEqual(respBack, some(v0Item));

const emptyReq = g.v0.api.functions.ping.toRequest();
g.v0.api.functions.ping.fromRequest(emptyReq);
const emptyResp = g.v0.api.functions.ping.toResponse();
g.v0.api.functions.ping.fromResponse(emptyResp);

console.log("TypeScript multi-major & edge-case smoke passed");
EOF

ln -s "${HARNESS_DIR}/node_modules" "${EDGE}/node_modules"
cp "${CONF}/package.json" "${EDGE}/package.json"
cp "${CONF}/tsconfig.json" "${EDGE}/tsconfig.json"

"${TSC_BIN}" --project "${EDGE}/tsconfig.json"
node "${EDGE}/dist/smoke.js"
