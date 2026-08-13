#!/usr/bin/env bash

set -Eeuo pipefail

trap 'status=$?; printf "%s:%s: assertion failed (exit %s): %s\n" "${BASH_SOURCE[0]}" "$LINENO" "$status" "$BASH_COMMAND" >&2' ERR

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
source "$repo_root/tests/assertions.sh"
export GH_FIXTURE_DATA="$repo_root/tests/fixtures/data/gh"
tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/gh-loupe-pr-reviews.XXXXXX")"
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
  local status
  if env PATH="$tmpdir/bin:$PATH" "$@" >"$tmpdir/$name.stdout" 2>"$tmpdir/$name.stderr"; then
    status=0
  else
    status=$?
  fi
  test "$status" -eq 1
  test ! -s "$tmpdir/$name.stdout"
  test "$(wc -l <"$tmpdir/$name.stderr")" -eq 1
  # shellcheck disable=SC2016
  assert_json --arg kind "$kind" '
    .schemaVersion == 1 and
    .error.kind == $kind and
    (.error.message | type == "string") and
    (.error.retryable | type == "boolean") and
    .error.retryAfterSeconds == null
  ' "$tmpdir/$name.stderr" >/dev/null
}

run_reviews success GH_TEST_CALLS_FILE="$tmpdir/calls" "$GH_LOUPE_BIN" \
  pr reviews 42 --repo riii111/dotfiles
# shellcheck disable=SC2016
assert_json '
  .schemaVersion == 1 and
  (.observedAt | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
  .data.reviews == [
    {"id":"review-unknown","author":"future","state":"FUTURE_STATE","body":"future","submittedAt":"2025-12-31T00:00:00Z","commitOid":"oid-future","detailsOmitted":false},
    {"id":"review-a","author":"alice","state":"CHANGES_REQUESTED","body":"change","submittedAt":"2026-01-01T00:00:00Z","commitOid":"oid-a","detailsOmitted":false},
    {"id":"review-b","author":"bob","state":"APPROVED","body":"before\n`<details>inline</details>`\n\n```markdown\n<details><summary>fenced</summary>fenced</details>\n```\n\n<!-- <details><summary>comment</summary>comment</details> -->\n<details open><summary>open</summary>shown</details>\n<details>\n<summary>incomplete</summary>\nnot closed\n<details data-x=\"unterminated\ninvalid","submittedAt":"2026-01-01T00:00:00Z","commitOid":"oid-b","detailsOmitted":false},
    {"id":"review-z","author":"zoe","state":"COMMENTED","body":"comment\n証拠\nreply","submittedAt":"2026-01-02T00:00:00Z","commitOid":"oid-z","detailsOmitted":true},
    {"id":"review-null","author":null,"state":"DISMISSED","body":"","submittedAt":null,"commitOid":null,"detailsOmitted":false},
    {"id":"review-null-a","author":"pending","state":"PENDING","body":"pending","submittedAt":null,"commitOid":null,"detailsOmitted":false}
  ] and
  (.data.reviews | all(keys == ["author","body","commitOid","detailsOmitted","id","state","submittedAt"])) and
  ([.. | objects | keys[]] | any(. == "comments" or . == "diffHunk" or . == "path") | not)
' "$tmpdir/success.stdout" >/dev/null
test "$(cat "$tmpdir/calls")" = \
  'api --method GET --paginate --slurp repos/riii111/dotfiles/pulls/42/reviews?per_page=100'

run_reviews include-details "$GH_LOUPE_BIN" pr reviews \
  42 --repo riii111/dotfiles --include-details --compact
# shellcheck disable=SC2016
assert_json '
  ([.data.reviews[].detailsOmitted] | all(. == false)) and
  (.data.reviews | map(select(.id == "review-z"))[0].body) == "comment\n<details data-source=\"bot\">\n<summary>証拠</summary>\n省略\n</details>\nreply" and
  (.data.reviews | map(select(.id == "review-b"))[0].body) == "before\n`<details>inline</details>`\n\n```markdown\n<details><summary>fenced</summary>fenced</details>\n```\n\n<!-- <details><summary>comment</summary>comment</details> -->\n<details open><summary>open</summary>shown</details>\n<details>\n<summary>incomplete</summary>\nnot closed\n<details data-x=\"unterminated\ninvalid" and
  (.data.reviews | all(keys == ["author","body","commitOid","detailsOmitted","id","state","submittedAt"]))
' "$tmpdir/include-details.stdout" >/dev/null
test "$(wc -l <"$tmpdir/include-details.stdout")" -eq 1

run_reviews compact "$GH_LOUPE_BIN" pr reviews \
  https://github.com/riii111/dotfiles/pull/42 --compact
test "$(wc -l <"$tmpdir/compact.stdout")" -eq 1

run_reviews empty GH_TEST_REVIEWS=empty "$GH_LOUPE_BIN" \
  pr reviews 42 --repo riii111/dotfiles
assert_json '.data.reviews == []' "$tmpdir/empty.stdout" >/dev/null

if env PATH="$tmpdir/bin:$PATH" "$GH_LOUPE_BIN" pr reviews 0 --repo riii111/dotfiles \
  >"$tmpdir/argument.stdout" 2>"$tmpdir/argument.stderr"; then
  argument_status=0
else
  argument_status=$?
fi
test "$argument_status" -eq 2
test ! -s "$tmpdir/argument.stdout"
grep -F 'gh-loupe pr reviews: error:' "$tmpdir/argument.stderr" >/dev/null

if env PATH="$tmpdir/bin:$PATH" "$GH_LOUPE_BIN" pr reviews 42 --repo riii111/dotfiles \
  --include-details=true >"$tmpdir/include-details-value.stdout" 2>"$tmpdir/include-details-value.stderr"; then
  include_details_value_status=0
else
  include_details_value_status=$?
fi
test "$include_details_value_status" -eq 2
test ! -s "$tmpdir/include-details-value.stdout"
grep -F 'gh-loupe pr reviews: error: unrecognized arguments: --include-details=true' \
  "$tmpdir/include-details-value.stderr" >/dev/null

assert_runtime_error invalid-response invalidResponse \
  GH_TEST_REVIEWS=invalid-page "$GH_LOUPE_BIN" pr reviews 42 --repo riii111/dotfiles
assert_runtime_error invalid-field invalidResponse \
  GH_TEST_REVIEWS=invalid-field "$GH_LOUPE_BIN" pr reviews 42 --repo riii111/dotfiles
assert_runtime_error missing-field invalidResponse \
  GH_TEST_REVIEWS=missing-field "$GH_LOUPE_BIN" pr reviews 42 --repo riii111/dotfiles
assert_runtime_error page-failure network \
  GH_TEST_REVIEWS=page-failure "$GH_LOUPE_BIN" pr reviews 42 --repo riii111/dotfiles

run_reviews help "$GH_LOUPE_BIN" pr reviews --help
grep -F 'usage: gh-loupe pr reviews [-h] [--repo REPO] [--include-details] [--compact] target' \
  "$tmpdir/help.stdout" >/dev/null
grep -F -- '--include-details   include folded <details> content (omitted by default)' \
  "$tmpdir/help.stdout" >/dev/null
