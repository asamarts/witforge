#!/usr/bin/env bash
# Test ci/check-version-references.sh against a scratch documentation tree.
set -euo pipefail

SCRIPT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/check-version-references.sh"
tmp_root="${HOME:-/tmp}/.tmp"
mkdir -p "${tmp_root}"
tmp="$(mktemp -d "${tmp_root}/witforge-release-version.XXXXXX")"
trap 'rm -rf "${tmp}"' EXIT
base="${tmp}/base"
mkdir -p "${base}/docs/guide" "${base}/docs/design" "${base}/.github/workflows"

write_fixture() {
    cat >"${base}/README.md" <<'EOF'
Current release: **9.9.9**.

```sh
cargo install witforge-cli --version 9.9.9 --locked
```

```yaml
- uses: asamarts/witforge@v9.9.9
```
EOF
    cat >"${base}/docs/index.md" <<'EOF'
Current release: **9.9.9**.
EOF
    cat >"${base}/docs/quickstart.md" <<'EOF'
cargo install witforge-cli --version 9.9.9 --locked
EOF
    cat >"${base}/docs/guide/ci.md" <<'EOF'
- uses: asamarts/witforge@v9.9.9
EOF
    cat >"${base}/docs/design/action.md" <<'EOF'
VERSION=9.9.9
- uses: asamarts/witforge@v9.9.9 # pinned: installs witforge 9.9.9
    version: 9.9.9
EOF
    cat >"${base}/.github/workflows/ci.yml" <<'EOF'
- uses: asamarts/witforge@v9.9.9
  with:
    version: 9.9.9
EOF
}
write_fixture

reset_case() {
    rm -rf "${tmp}/case"
    cp -R "${base}" "${tmp}/case"
}

expect_pass() {
    bash "${SCRIPT}" "${tmp}/case" 9.9.9 >/dev/null \
        || { echo "version reference check rejected a valid tree" >&2; exit 1; }
}

expect_failure() {
    local label="$1"
    local file="$2"
    local expression="$3"
    reset_case
    sed "${expression}" "${tmp}/case/${file}" >"${tmp}/case/${file}.new"
    mv "${tmp}/case/${file}.new" "${tmp}/case/${file}"
    if bash "${SCRIPT}" "${tmp}/case" 9.9.9 >/dev/null 2>&1; then
        echo "version reference check accepted ${label}" >&2
        exit 1
    fi
}

reset_case
expect_pass

expect_failure "the README current release" README.md \
    's/Current release: \*\*9\.9\.9\*\*/Current release: **9.9.8**/'
expect_failure "the README install version" README.md \
    's/--version 9\.9\.9/--version 9.9.8/'
expect_failure "the README Action reference" README.md \
    's|@v9\.9\.9|@v9.9.8|'
expect_failure "the docs index current release" docs/index.md \
    's/9\.9\.9/9.9.8/'
expect_failure "the quickstart install version" docs/quickstart.md \
    's/9\.9\.9/9.9.8/'
expect_failure "the CI guide Action reference" docs/guide/ci.md \
    's/9\.9\.9/9.9.8/'
expect_failure "the Action guide VERSION example" docs/design/action.md \
    's/VERSION=9\.9\.9/VERSION=9.9.8/'
expect_failure "the Action guide reference" docs/design/action.md \
    's|@v9\.9\.9|@v9.9.8|'
expect_failure "the Action guide reference comment" docs/design/action.md \
    's/installs witforge 9\.9\.9/installs witforge 9.9.8/'
expect_failure "the Action guide input example" docs/design/action.md \
    's/version: 9\.9\.9/version: 9.9.8/'
expect_failure "the workflow Action reference" .github/workflows/ci.yml \
    's|@v9\.9\.9|@v9.9.8|'
expect_failure "the workflow Action input" .github/workflows/ci.yml \
    's/version: 9\.9\.9/version: 9.9.8/'

echo "release version reference test passed"
