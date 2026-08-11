#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/gh-read-pr-reviews.XXXXXX")"
trap 'rm -rf "$tmpdir"' EXIT
mkdir -p "$tmpdir/bin"
cp "$repo_root/tests/fixtures/gh" "$tmpdir/bin/gh"
chmod +x "$tmpdir/bin/gh"

run_reviews() {
  local name="$1"
  shift
  env PATH="$tmpdir/bin:$PATH" "$@" >"$tmpdir/$name.stdout" 2>"$tmpdir/$name.stderr"
  test ! -s "$tmpdir/$name.stderr"
}

assert_runtime_error() {
  local name="$1"
  local kind="$2"
  shift 2
  set +e
  env PATH="$tmpdir/bin:$PATH" "$@" >"$tmpdir/$name.stdout" 2>"$tmpdir/$name.stderr"
  local status=$?
  set -e
  test "$status" -eq 1
  test ! -s "$tmpdir/$name.stdout"
  test "$(wc -l <"$tmpdir/$name.stderr")" -eq 1
  jq -e --arg kind "$kind" '
    .schemaVersion == 1 and
    .error.kind == $kind and
    (.error.message | type == "string") and
    (.error.retryable | type == "boolean") and
    .error.retryAfterSeconds == null
  ' "$tmpdir/$name.stderr" >/dev/null
}

run_reviews success GH_TEST_CALLS_FILE="$tmpdir/calls" "$GH_READ_BIN" \
  pr reviews 42 --repo riii111/dotfiles
jq -e '
  .schemaVersion == 1 and
  (.observedAt | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
  .data.reviews == [
    {"id":"review-unknown","author":"future","state":"FUTURE_STATE","body":"future","submittedAt":"2025-12-31T00:00:00Z","commitOid":"oid-future"},
    {"id":"review-a","author":"alice","state":"CHANGES_REQUESTED","body":"change","submittedAt":"2026-01-01T00:00:00Z","commitOid":"oid-a"},
    {"id":"review-b","author":"bob","state":"APPROVED","body":"approved","submittedAt":"2026-01-01T00:00:00Z","commitOid":"oid-b"},
    {"id":"review-z","author":"zoe","state":"COMMENTED","body":"comment","submittedAt":"2026-01-02T00:00:00Z","commitOid":"oid-z"},
    {"id":"review-null","author":null,"state":"DISMISSED","body":"","submittedAt":null,"commitOid":null}
  ] and
  (.data.reviews | all(keys == ["author","body","commitOid","id","state","submittedAt"])) and
  ([.. | objects | keys[]] | any(. == "comments" or . == "diffHunk" or . == "path") | not)
' "$tmpdir/success.stdout" >/dev/null
test "$(cat "$tmpdir/calls")" = \
  'api --method GET --paginate --slurp repos/riii111/dotfiles/pulls/42/reviews?per_page=100'

run_reviews compact "$GH_READ_BIN" pr reviews \
  https://github.com/riii111/dotfiles/pull/42 --compact
test "$(wc -l <"$tmpdir/compact.stdout")" -eq 1

run_reviews empty GH_TEST_REVIEWS=empty "$GH_READ_BIN" \
  pr reviews 42 --repo riii111/dotfiles
jq -e '.data.reviews == []' "$tmpdir/empty.stdout" >/dev/null

set +e
env PATH="$tmpdir/bin:$PATH" "$GH_READ_BIN" pr reviews 0 --repo riii111/dotfiles \
  >"$tmpdir/argument.stdout" 2>"$tmpdir/argument.stderr"
argument_status=$?
set -e
test "$argument_status" -eq 2
test ! -s "$tmpdir/argument.stdout"
grep -F 'gh-read pr reviews: error:' "$tmpdir/argument.stderr" >/dev/null

assert_runtime_error invalid-response invalidResponse \
  GH_TEST_REVIEWS=invalid-page "$GH_READ_BIN" pr reviews 42 --repo riii111/dotfiles
assert_runtime_error invalid-field invalidResponse \
  GH_TEST_REVIEWS=invalid-field "$GH_READ_BIN" pr reviews 42 --repo riii111/dotfiles
assert_runtime_error missing-field invalidResponse \
  GH_TEST_REVIEWS=missing-field "$GH_READ_BIN" pr reviews 42 --repo riii111/dotfiles
assert_runtime_error page-failure network \
  GH_TEST_REVIEWS=page-failure "$GH_READ_BIN" pr reviews 42 --repo riii111/dotfiles

run_reviews help "$GH_READ_BIN" pr reviews --help
grep -F 'usage: gh-read pr reviews [-h] [--repo REPO] [--compact] target' \
  "$tmpdir/help.stdout" >/dev/null
