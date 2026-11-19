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
- uses: ./
  with:
    version: 9.9.9
- uses: asamarts/witforge@v9.9.9
  with:
    version: 9.9.9
EOF
    cat >"${base}/CHANGELOG.md" <<'EOF'
# Changelog

## [Unreleased]

## [9.9.9] - 2026-01-03

- Current.

## [9.9.8] - 2026-01-02

- Previous.

## [9.9.7] - 2026-01-01

- Older.
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

# CI runs a published release: the workspace version or the release before it, and one release
# throughout the workflow.
expect_failure "a workflow Action reference that disagrees with its version inputs" \
    .github/workflows/ci.yml 's|@v9\.9\.9|@v9.9.8|'
expect_failure "workflow version inputs that disagree with the Action reference" \
    .github/workflows/ci.yml 's/version: 9\.9\.9/version: 9.9.8/'
expect_failure "a local-smoke version input that disagrees with the published Action" \
    .github/workflows/ci.yml '3s/9\.9\.9/9.9.8/'
expect_failure "a workflow two releases behind" .github/workflows/ci.yml 's/9\.9\.9/9.9.7/g'

# A release-preparation tree: documentation names the version being prepared, and CI still runs
# the previous release, whose tag and assets exist.
previous_workflow() {
    sed 's/9\.9\.9/9.9.8/g' "${base}/.github/workflows/ci.yml" \
        >"${tmp}/case/.github/workflows/ci.yml"
}
reset_case
previous_workflow
expect_pass
report="$(bash "${SCRIPT}" "${tmp}/case" 9.9.9)"
[[ "${report}" == *"CI runs the published 9.9.8 Action"* ]] \
    || { echo "the release-preparation tree was not reported as such: ${report}" >&2; exit 1; }

# Without a previous release in the changelog, the workflow must name the workspace version.
reset_case
previous_workflow
rm "${tmp}/case/CHANGELOG.md"
if bash "${SCRIPT}" "${tmp}/case" 9.9.9 >/dev/null 2>&1; then
    echo "version reference check accepted an older workflow release without a changelog" >&2
    exit 1
fi
reset_case
previous_workflow
sed '/^## \[9\.9\.9\]/,/^- Current\./d' "${base}/CHANGELOG.md" >"${tmp}/case/CHANGELOG.md"
if report="$(bash "${SCRIPT}" "${tmp}/case" 9.9.9 2>&1)"; then
    echo "version reference check accepted an older workflow release for an undated version" >&2
    exit 1
fi
# The message must not send the reader to pin CI to the unpublished version.
[[ "${report}" == *"no dated section for 9.9.9"* ]] \
    || { echo "an undated version was not reported as such: ${report}" >&2; exit 1; }

# Once the version's tag is on an earlier commit, the release is published and the workflow
# must run it. Without the tag, and with the tag at HEAD (the release being made), the previous
# release is still accepted.
scratch_git() {
    git -C "${tmp}/case" -c user.name=test -c user.email=test@example.invalid \
        -c commit.gpgsign=false -c tag.gpgsign=false "$@"
}
reset_case
previous_workflow
scratch_git init --quiet
scratch_git add --all
scratch_git commit --quiet --message "prepare 9.9.9"
expect_pass
scratch_git tag v9.9.9
expect_pass
scratch_git commit --quiet --allow-empty --message "after the release"
if report="$(bash "${SCRIPT}" "${tmp}/case" 9.9.9 2>&1)"; then
    echo "version reference check accepted the previous release in CI after 9.9.9 was tagged" >&2
    exit 1
fi
[[ "${report}" == *"v9.9.9 is published"* ]] \
    || { echo "a stale CI pin was not reported as such: ${report}" >&2; exit 1; }
cp "${base}/.github/workflows/ci.yml" "${tmp}/case/.github/workflows/ci.yml"
expect_pass

# Both arguments are required.
reset_case
if bash "${SCRIPT}" "${tmp}/case" >/dev/null 2>&1; then
    echo "version reference check ran without a version" >&2
    exit 1
fi

echo "release version reference test passed"
