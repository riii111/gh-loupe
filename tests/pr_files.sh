#!/usr/bin/env bash

set -Eeuo pipefail

trap 'status=$?; printf "%s:%s: assertion failed (exit %s): %s\n" "${BASH_SOURCE[0]}" "$LINENO" "$status" "$BASH_COMMAND" >&2' ERR

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
source "$repo_root/tests/assertions.sh"
tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/gh-loupe-pr-files.XXXXXX")"
trap 'rm -rf "$tmpdir"' EXIT
mkdir -p "$tmpdir/bin"
cp "$repo_root/tests/fixtures/gh" "$tmpdir/bin/gh"
chmod +x "$tmpdir/bin/gh"

run_files() {
  local name="$1"
  shift
  env PATH="$tmpdir/bin:$PATH" "$@" >"$tmpdir/$name.stdout" 2>"$tmpdir/$name.stderr"
  test ! -s "$tmpdir/$name.stderr"
}

assert_argument_error() {
  local name="$1"
  shift
  local status
  if env PATH="$tmpdir/bin:$PATH" GH_TEST_CALLS_FILE="$tmpdir/$name.calls" "$GH_LOUPE_BIN" "$@" \
    >"$tmpdir/$name.stdout" 2>"$tmpdir/$name.stderr"; then
    status=0
  else
    status=$?
  fi
  test "$status" -eq 2
  test ! -s "$tmpdir/$name.stdout"
  test ! -e "$tmpdir/$name.calls"
}

run_files default GH_TEST_GRAPHQL_PAYLOADS_FILE="$tmpdir/default.payloads" "$GH_LOUPE_BIN" \
  pr files 42 --repo riii111/dotfiles
assert_json '
  .schemaVersion == 1 and
  .data.files == [
    {"path":"src/new.rs","status":"ADDED","additions":10,"deletions":0},
    {"path":"README.md","status":"MODIFIED","additions":2,"deletions":4},
    {"path":"old.txt","status":"DELETED","additions":0,"deletions":7}
  ] and
  .data.summary == {"total":3,"additions":12,"deletions":4} and
  .data.totalCount == 3 and
  .data.truncated == false and
  (.data.files | all(keys == ["additions","deletions","path","status"])) and
  ([.. | objects | keys[]] | any(. == "patch" or . == "diffHunk" or . == "body" or . == "contents") | not)
' "$tmpdir/default.stdout" >/dev/null
jq -s -e '
  length == 1 and
  .[0].variables.limit == 20 and
  (.[0].query | contains("PullRequestFiles")) and
  (.[0].query | [contains("patch"), contains("diffHunk"), contains("body"), contains("contents")] | any | not)
' "$tmpdir/default.payloads" >/dev/null

run_files limit-one "$GH_LOUPE_BIN" pr files 42 --repo riii111/dotfiles --limit 1 --compact
assert_json '
  (.data.files | length) == 1 and
  .data.files[0].path == "src/new.rs" and
  .data.summary == {"total":3,"additions":12,"deletions":4} and
  .data.totalCount == 3 and
  .data.truncated == true
' "$tmpdir/limit-one.stdout" >/dev/null

run_files limit-hundred GH_TEST_GRAPHQL_PAYLOADS_FILE="$tmpdir/limit-hundred.payloads" \
  "$GH_LOUPE_BIN" pr files 42 --repo riii111/dotfiles --limit 100
jq -e '.variables.limit == 100' "$tmpdir/limit-hundred.payloads" >/dev/null

run_files server-truncated GH_TEST_PR_FILES=server-truncated "$GH_LOUPE_BIN" \
  pr files 42 --repo riii111/dotfiles --limit 100
assert_json '
  .data.summary == {"total":4001,"additions":4001,"deletions":4002} and
  .data.totalCount == 4001 and
  .data.truncated == true and
  (.data.files | length) == 1
' "$tmpdir/server-truncated.stdout" >/dev/null

run_files empty GH_TEST_PR_FILES=empty "$GH_LOUPE_BIN" pr files 42 --repo riii111/dotfiles
assert_json '.data.files == [] and .data.summary == {"total":0,"additions":0,"deletions":0} and .data.truncated == false' \
  "$tmpdir/empty.stdout" >/dev/null

run_files last-page-cursor GH_TEST_PR_FILES=last-page-cursor "$GH_LOUPE_BIN" \
  pr files 42 --repo riii111/dotfiles
assert_json '.data.files == [{"path":"last.txt","status":"ADDED","additions":1,"deletions":0}] and .data.truncated == false' \
  "$tmpdir/last-page-cursor.stdout" >/dev/null

run_files url "$GH_LOUPE_BIN" pr files https://github.com/riii111/dotfiles/pull/42 --compact
test "$(wc -l <"$tmpdir/url.stdout" | tr -d ' ')" -eq 1

assert_argument_error limit-zero pr files 42 --repo riii111/dotfiles --limit 0
assert_argument_error limit-high pr files 42 --repo riii111/dotfiles --limit 101
assert_argument_error limit-equals-zero pr files 42 --repo riii111/dotfiles --limit=0
assert_argument_error missing-limit-value pr files 42 --repo riii111/dotfiles --limit
assert_argument_error unrecognized-limit-value pr files 42 --repo riii111/dotfiles --limit=true

run_files help "$GH_LOUPE_BIN" pr files --help
grep -Fx 'usage: gh-loupe pr files [-h] [--repo REPO] [--limit N] [--compact] target' \
  "$tmpdir/help.stdout" >/dev/null
grep -Fx '  --limit N    return at most N files, from 1 through 100 (default: 20)' \
  "$tmpdir/help.stdout" >/dev/null

assert_runtime_error() {
  local name="$1"
  local expected_kind="$2"
  shift 2
  local status
  if env PATH="$tmpdir/bin:$PATH" "$@" >"$tmpdir/$name.stdout" 2>"$tmpdir/$name.stderr"; then
    status=0
  else
    status=$?
  fi
  test "$status" -eq 1
  test ! -s "$tmpdir/$name.stdout"
  # shellcheck disable=SC2016
  assert_json --arg kind "$expected_kind" '.schemaVersion == 1 and .error.kind == $kind' \
    "$tmpdir/$name.stderr" >/dev/null
}

assert_runtime_error invalid-node invalidResponse GH_TEST_PR_FILES=invalid-node "$GH_LOUPE_BIN" \
  pr files 42 --repo riii111/dotfiles
assert_runtime_error missing-total-count invalidResponse GH_TEST_PR_FILES=missing-total-count "$GH_LOUPE_BIN" \
  pr files 42 --repo riii111/dotfiles
assert_runtime_error invalid-page-info invalidResponse GH_TEST_PR_FILES=invalid-page-info "$GH_LOUPE_BIN" \
  pr files 42 --repo riii111/dotfiles
assert_runtime_error graphql-error githubCli GH_TEST_GRAPHQL_ERROR=1 "$GH_LOUPE_BIN" \
  pr files 42 --repo riii111/dotfiles
