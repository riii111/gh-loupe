#!/usr/bin/env bash

set -Eeuo pipefail

trap 'status=$?; printf "%s:%s: assertion failed (exit %s): %s\n" "${BASH_SOURCE[0]}" "$LINENO" "$status" "$BASH_COMMAND" >&2' ERR

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/gh-loupe-cli.XXXXXX")"
trap 'rm -rf "$tmpdir"' EXIT
GH_LOUPE_PACKAGE_VERSION="$(cargo metadata --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" | jq -r '.packages[] | select(.name == "gh-loupe") | .version')"
export GH_LOUPE_PACKAGE_VERSION
mkdir -p "$tmpdir/bin" "$tmpdir/rust"
cp "$repo_root/tests/fixtures/gh" "$tmpdir/bin/gh"
chmod +x "$tmpdir/bin/gh"
ln -s "$GH_LOUPE_BIN" "$tmpdir/rust/gh-loupe"

run_cli() {
  local name="$1"
  shift
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-loupe" "$@" \
    >"$tmpdir/$name.stdout" 2>"$tmpdir/$name.stderr"
}

assert_argument_error() {
  local name="$1"
  shift

  local status
  if env PATH="$tmpdir/bin:$PATH" "$tmpdir/rust/gh-loupe" "$@" \
    >"$tmpdir/$name.argument.stdout" 2>"$tmpdir/$name.argument.stderr"; then
    status=0
  else
    status=$?
  fi

  if [ "$status" -ne 2 ]; then
    printf '%s: expected argument error status 2, got %s\n' "$name" "$status" >&2
    return 1
  fi
  test ! -s "$tmpdir/$name.argument.stdout"
  test -s "$tmpdir/$name.argument.stderr"
}

run_review_threads() {
  local name="$1"
  shift
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-loupe" "$@" \
    >"$tmpdir/$name.review_threads.stdout" 2>"$tmpdir/$name.review_threads.stderr"
  test ! -s "$tmpdir/$name.review_threads.stderr"
  jq -e '
    .schemaVersion == 1 and
    (.observedAt | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
    (.data | keys == ["reviewThreads"])
  ' "$tmpdir/$name.review_threads.stdout" >/dev/null
}

run_review_thread() {
  local name="$1"
  shift
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-loupe" "$@" \
    >"$tmpdir/$name.review_thread.stdout" 2>"$tmpdir/$name.review_thread.stderr"
  test ! -s "$tmpdir/$name.review_thread.stderr"
  jq -e '
    .schemaVersion == 1 and
    (.observedAt | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
    (.data | keys == ["reviewThread"])
  ' "$tmpdir/$name.review_thread.stdout" >/dev/null
}

assert_runtime_failure() {
  local name="$1"
  local expected_kind="$2"
  local output_suffix="$3"
  shift 3
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  local status
  if env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-loupe" "$@" \
    >"$tmpdir/$name.$output_suffix.stdout" 2>"$tmpdir/$name.$output_suffix.stderr"; then
    status=0
  else
    status=$?
  fi

  test "$status" -ne 0
  test ! -s "$tmpdir/$name.$output_suffix.stdout"
  test "$(wc -l <"$tmpdir/$name.$output_suffix.stderr")" -eq 1
  jq -e --arg kind "$expected_kind" '
    .schemaVersion == 1 and
    .error.kind == $kind and
    (.error.message | type == "string") and
    (.error.retryable | type == "boolean") and
    (.error.retryAfterSeconds == null)
  ' "$tmpdir/$name.$output_suffix.stderr" >/dev/null
}

run_overview() {
  local name="$1"
  shift
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-loupe" "$@" \
    >"$tmpdir/$name.overview.stdout" 2>"$tmpdir/$name.overview.stderr"
  test ! -s "$tmpdir/$name.overview.stderr"
}

assert_overview_runtime_error() {
  local name="$1"
  local expected_kind="$2"
  local expected_retryable="$3"
  shift 3
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  local status
  if env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-loupe" "$@" \
    >"$tmpdir/$name.overview.stdout" 2>"$tmpdir/$name.overview.stderr"; then
    status=0
  else
    status=$?
  fi

  test "$status" -ne 0
  test ! -s "$tmpdir/$name.overview.stdout"
  test "$(wc -l <"$tmpdir/$name.overview.stderr")" -eq 1
  jq -e --arg kind "$expected_kind" --argjson retryable "$expected_retryable" \
    '.schemaVersion == 1 and .error.kind == $kind and .error.retryable == $retryable and (.error.retryAfterSeconds == null)' \
    "$tmpdir/$name.overview.stderr" >/dev/null
}

assert_overview_runtime_error_message() {
  local name="$1"
  local expected_kind="$2"
  local expected_retryable="$3"
  local expected_message="$4"
  shift 4
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  local status
  if env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-loupe" "$@" \
    >"$tmpdir/$name.overview.stdout" 2>"$tmpdir/$name.overview.stderr"; then
    status=0
  else
    status=$?
  fi

  test "$status" -ne 0
  test ! -s "$tmpdir/$name.overview.stdout"
  test "$(wc -l <"$tmpdir/$name.overview.stderr")" -eq 1
  jq -e --arg kind "$expected_kind" --argjson retryable "$expected_retryable" \
    --arg message "$expected_message" \
    '.schemaVersion == 1 and .error.kind == $kind and .error.retryable == $retryable and .error.message == $message and (.error.retryAfterSeconds == null)' \
    "$tmpdir/$name.overview.stderr" >/dev/null
}

run_comments() {
  local name="$1"
  shift
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-loupe" "$@" \
    >"$tmpdir/$name.comments.stdout" 2>"$tmpdir/$name.comments.stderr"
  test ! -s "$tmpdir/$name.comments.stderr"
  jq -e '
    .schemaVersion == 1 and
    (.observedAt | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
    (.data | keys == ["comments"])
  ' "$tmpdir/$name.comments.stdout" >/dev/null
}

run_cli root-help -- --help
test ! -s "$tmpdir/root-help.stderr"
grep -F 'usage: gh-loupe [-h] [--version] {pr,issue} ...' "$tmpdir/root-help.stdout" >/dev/null

run_cli root-version -- --version
test ! -s "$tmpdir/root-version.stderr"
test "$(cat "$tmpdir/root-version.stdout")" = "gh-loupe $GH_LOUPE_PACKAGE_VERSION"

run_cli pr-help -- pr --help
test ! -s "$tmpdir/pr-help.stderr"
grep -F 'usage: gh-loupe pr [-h] {overview,comments,reviews,review-threads,review-thread,checks} ...' "$tmpdir/pr-help.stdout" >/dev/null
for subcommand in overview comments reviews review-threads review-thread checks; do
  grep -E "^    $subcommand  +" "$tmpdir/pr-help.stdout" >/dev/null
done
if grep -E '^    (threads|thread|full|legacy)  +' "$tmpdir/pr-help.stdout" >/dev/null; then
  exit 1
fi

run_cli review-threads-help -- pr review-threads --help
test ! -s "$tmpdir/review-threads-help.stderr"
grep -F 'usage: gh-loupe pr review-threads ' "$tmpdir/review-threads-help.stdout" >/dev/null
if grep -Eiq '(^|[[:space:]])pr (threads|thread)([[:space:]]|$)|data\.(threads|thread)' \
  "$tmpdir/review-threads-help.stdout"; then
  exit 1
fi

run_cli review-thread-help -- pr review-thread --help
test ! -s "$tmpdir/review-thread-help.stderr"
grep -F 'usage: gh-loupe pr review-thread ' "$tmpdir/review-thread-help.stdout" >/dev/null
if grep -Eiq '(^|[[:space:]])pr (threads|thread)([[:space:]]|$)|data\.(threads|thread)' \
  "$tmpdir/review-thread-help.stdout"; then
  exit 1
fi

if GH_TEST_CALLS_FILE="$tmpdir/bare-pr.calls" PATH="$tmpdir/bin:$PATH" \
  "$tmpdir/rust/gh-loupe" pr 42 >"$tmpdir/bare-pr.stdout" 2>"$tmpdir/bare-pr.stderr"; then
  bare_status=0
else
  bare_status=$?
fi
test "$bare_status" -eq 2
test ! -s "$tmpdir/bare-pr.stdout"
test ! -e "$tmpdir/bare-pr.calls"
grep -Fx 'usage: gh-loupe pr [-h] {overview,comments,reviews,review-threads,review-thread,checks} ...' "$tmpdir/bare-pr.stderr" >/dev/null
grep -F "gh-loupe pr: error: argument subcommand: invalid choice: '42'" "$tmpdir/bare-pr.stderr" >/dev/null

assert_argument_error root-missing-resource
assert_argument_error pr-missing-subcommand pr
assert_argument_error issue-missing-target issue
assert_argument_error issue-pr-only-option issue 42 --include-resolved
grep -Fx 'usage: gh-loupe issue [-h] [--repo REPO] [--compact] target' \
  "$tmpdir/issue-pr-only-option.argument.stderr" >/dev/null
grep -F 'gh-loupe issue: error: unrecognized arguments: --include-resolved' \
  "$tmpdir/issue-pr-only-option.argument.stderr" >/dev/null

for removed_subcommand in threads thread; do
  calls_file="$tmpdir/removed-$removed_subcommand.calls"
  if GH_TEST_CALLS_FILE="$calls_file" PATH="$tmpdir/bin:$PATH" \
    "$tmpdir/rust/gh-loupe" pr "$removed_subcommand" \
    >"$tmpdir/removed-$removed_subcommand.stdout" \
    2>"$tmpdir/removed-$removed_subcommand.stderr"; then
    removed_status=0
  else
    removed_status=$?
  fi
  test "$removed_status" -eq 2
  test ! -s "$tmpdir/removed-$removed_subcommand.stdout"
  test ! -e "$calls_file"
  grep -F "argument subcommand: invalid choice: '$removed_subcommand'" \
    "$tmpdir/removed-$removed_subcommand.stderr" >/dev/null
done

issue_calls_file="$tmpdir/issue.calls"
run_cli issue-default "GH_TEST_CALLS_FILE=$issue_calls_file" -- issue 42
test ! -s "$tmpdir/issue-default.stderr"
jq -e '. == {
  "issue":{"number":42,"title":"Issue","state":"open","body":"body"},
  "comments":[{"id":2,"body":"conversation"},{"id":4,"body":"conversation page 2"}]
}' "$tmpdir/issue-default.stdout" >/dev/null
test "$(sed -n '1p' "$issue_calls_file")" = 'repo view --json nameWithOwner'
test "$(sed -n '2p' "$issue_calls_file")" = \
  'api repos/riii111/dotfiles/issues/42'
test "$(sed -n '3p' "$issue_calls_file")" = \
  'api --method GET --paginate --slurp repos/riii111/dotfiles/issues/42/comments?per_page=100'
test "$(wc -l <"$issue_calls_file")" -eq 3

run_cli issue-url-compact -- issue https://github.com/riii111/dotfiles/issues/42 --compact
test ! -s "$tmpdir/issue-url-compact.stderr"
test "$(wc -l <"$tmpdir/issue-url-compact.stdout" | tr -d ' ')" -eq 1
jq -e '.issue.number == 42 and (.comments | length == 2)' "$tmpdir/issue-url-compact.stdout" >/dev/null

if env PATH="$tmpdir/bin:$PATH" "$tmpdir/rust/gh-loupe" \
  issue https://github.com/riii111/dotfiles/issues/42 --repo other/repo \
  >"$tmpdir/issue-conflicting-repo.stdout" 2>"$tmpdir/issue-conflicting-repo.stderr"; then
  issue_status=0
else
  issue_status=$?
fi
test "$issue_status" -eq 1
test ! -s "$tmpdir/issue-conflicting-repo.stdout"
test "$(cat "$tmpdir/issue-conflicting-repo.stderr")" = \
  '--repo conflicts with the issue URL'

run_cli issue-utf8 GH_TEST_UTF8=1 -- issue 42
jq -e '.issue.title == "日本語のIssue" and .issue.body == "ずんだ"' "$tmpdir/issue-utf8.stdout" >/dev/null

assert_runtime_failure issue-missing notFound issue \
  GH_TEST_MISSING_ISSUE=1 -- issue 42
assert_runtime_failure issue-gh-failure githubCli issue \
  GH_TEST_FAILURE=1 -- issue 42 --repo riii111/dotfiles
assert_runtime_failure issue-invalid-json invalidResponse issue \
  GH_TEST_INVALID_JSON=1 -- issue 42 --repo riii111/dotfiles
assert_runtime_failure issue-invalid-page invalidResponse issue \
  GH_TEST_ISSUE_COMMENTS=invalid-page -- issue 42 --repo riii111/dotfiles
assert_runtime_failure issue-invalid-item invalidResponse issue \
  GH_TEST_ISSUE_COMMENTS=invalid-item -- issue 42 --repo riii111/dotfiles
assert_runtime_failure issue-missing-repository-metadata invalidResponse issue \
  GH_TEST_REPO_METADATA_MISSING=1 -- issue 42

if env PATH="$tmpdir/missing-gh" "$tmpdir/rust/gh-loupe" issue 42 --repo riii111/dotfiles \
  >"$tmpdir/issue-spawn.stdout" 2>"$tmpdir/issue-spawn.stderr"; then
  issue_status=0
else
  issue_status=$?
fi
test "$issue_status" -ne 0
test ! -s "$tmpdir/issue-spawn.stdout"
test "$(wc -l <"$tmpdir/issue-spawn.stderr")" -eq 1
jq -e '
  .schemaVersion == 1 and
  .error.kind == "githubCli" and
  (.error.message | type == "string") and
  .error.retryable == false and
  .error.retryAfterSeconds == null
' "$tmpdir/issue-spawn.stderr" >/dev/null

run_overview overview-default -- pr overview 42 --repo riii111/dotfiles
jq -e '
  .schemaVersion == 1 and
  (.observedAt | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
  .data.pullRequest == {
    "number": 42,
    "title": "Add pull request title",
    "url": "https://github.com/riii111/dotfiles/pull/42",
    "state": "OPEN",
    "isDraft": false,
    "headRefOid": "head",
    "baseRefOid": "base",
    "reviewDecision": "APPROVED",
    "mergeStateStatus": "CLEAN"
  } and
  .data.checks == {
    "required": 5,
    "passed": 2,
    "pending": 1,
    "failed": 2,
    "all": {"total": 7, "passed": 3, "pending": 2, "failed": 2}
  } and
  .data.reviewThreads == {"unresolved": 2} and
  ([.. | objects | keys[]] | any(. == "body" or . == "comments" or . == "reviews" or . == "bucket" or . == "name") | not)
' "$tmpdir/overview-default.overview.stdout" >/dev/null
test "$(wc -l <"$tmpdir/overview-default.overview.stdout")" -gt 1

for mode in repeat cycle missing empty wrong-type; do
  case "$mode" in
    repeat) expected_calls=2 ;;
    empty|missing|wrong-type) expected_calls=1 ;;
    cycle) expected_calls=3 ;;
  esac
  calls_file="$tmpdir/overview-pagination-$mode-calls"
  assert_overview_runtime_error "overview-pagination-$mode" invalidResponse false \
    GH_TEST_CALLS_FILE="$calls_file" GH_OVERVIEW_PAGINATION="$mode" -- \
    pr overview 42 --repo riii111/dotfiles
  test "$(grep -c 'api graphql' "$calls_file")" -eq "$expected_calls"
done

timing_file="$tmpdir/overview-timing"
run_overview overview-concurrent "GH_OVERVIEW_TIMING_FILE=$timing_file" GH_OVERVIEW_SLEEP=1 -- \
  pr overview 42 --repo riii111/dotfiles
test "$(awk '$2 == "start" { count += 1 } END { print count + 0 }' "$timing_file")" -eq 3
awk '
  $2 == "start" { starts += 1 }
  $2 == "end" && starts < 3 { invalid = 1 }
  END { exit invalid }
' "$timing_file"

run_overview overview-compact -- pr overview https://github.com/riii111/dotfiles/pull/42 --compact
test "$(wc -l <"$tmpdir/overview-compact.overview.stdout")" -eq 1
jq -e '.data.reviewThreads.unresolved == 2' "$tmpdir/overview-compact.overview.stdout" >/dev/null

run_overview overview-null-fields GH_OVERVIEW_NULL_FIELDS=1 -- pr overview 42 --repo riii111/dotfiles
jq -e '
  .data.pullRequest.state == null and
  .data.pullRequest.isDraft == null and
  .data.pullRequest.headRefOid == null and
  .data.pullRequest.baseRefOid == null and
  .data.pullRequest.reviewDecision == null and
  .data.pullRequest.mergeStateStatus == null
' "$tmpdir/overview-null-fields.overview.stdout" >/dev/null

run_overview overview-no-required GH_OVERVIEW_CHECKS=empty -- pr overview 42 --repo riii111/dotfiles
jq -e '.data.checks == {
  "required": 0,
  "passed": 0,
  "pending": 0,
  "failed": 0,
  "all": {"total": 7, "passed": 3, "pending": 2, "failed": 2}
}' \
  "$tmpdir/overview-no-required.overview.stdout" >/dev/null

run_overview overview-no-required-cli GH_OVERVIEW_CHECKS=no-required -- pr overview 42 --repo riii111/dotfiles
jq -e '.data.checks == {
  "required": 0,
  "passed": 0,
  "pending": 0,
  "failed": 0,
  "all": {"total": 7, "passed": 3, "pending": 2, "failed": 2}
}' \
  "$tmpdir/overview-no-required-cli.overview.stdout" >/dev/null

run_overview overview-empty-all GH_OVERVIEW_ALL_CHECKS=no-checks -- pr overview 42 --repo riii111/dotfiles
jq -e '.data.checks.all == {"total": 0, "passed": 0, "pending": 0, "failed": 0}' \
  "$tmpdir/overview-empty-all.overview.stdout" >/dev/null

assert_argument_error overview-abbreviated-option pr overview 42 --comp
assert_argument_error overview-unknown-option pr overview 42 --include-resolved
assert_argument_error overview-invalid-target pr overview nope --repo riii111/dotfiles
assert_overview_runtime_error overview-unknown-bucket invalidResponse false \
  GH_OVERVIEW_CHECKS=unknown -- pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-all-unknown-bucket invalidResponse false \
  GH_OVERVIEW_ALL_CHECKS=unknown -- pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-required-failure githubCli false \
  GH_OVERVIEW_REQUIRED_FAILURE=1 -- pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-all-failure githubCli false \
  GH_OVERVIEW_ALL_FAILURE=1 -- pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error_message overview-required-before-all-failure githubCli false \
  'simulated required checks failure' \
  GH_OVERVIEW_REQUIRED_FAILURE=1 GH_OVERVIEW_ALL_FAILURE=1 -- \
  pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error_message overview-graphql-before-check-failures githubCli false \
  '[{"message": "simulated GraphQL failure"}]' \
  GH_TEST_GRAPHQL_ERROR=1 GH_OVERVIEW_REQUIRED_FAILURE=1 GH_OVERVIEW_ALL_FAILURE=1 -- \
  pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-missing notFound false \
  GH_TEST_MISSING_PR=1 -- pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-network network true \
  GH_TEST_NETWORK_FAILURE=1 -- pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-repository-not-found notFound false \
  GH_TEST_REPOSITORY_NOT_FOUND=1 -- pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-gh-failure githubCli false \
  GH_TEST_FAILURE=1 -- pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-invalid-json invalidResponse false \
  GH_TEST_INVALID_JSON=1 -- pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-missing-title invalidResponse false \
  GH_OVERVIEW_TITLE=missing -- pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-invalid-title invalidResponse false \
  GH_OVERVIEW_TITLE=invalid -- pr overview 42 --repo riii111/dotfiles

assert_argument_error comments-missing-target pr comments
assert_argument_error comments-abbreviated-repo pr comments 42 --rep riii111/dotfiles
assert_argument_error comments-abbreviated-compact pr comments 42 --comp
assert_argument_error comments-invalid-zero pr comments 0 --repo riii111/dotfiles
assert_argument_error comments-invalid-repo pr comments 42 --repo ../..
assert_argument_error comments-conflicting-repo \
  pr comments https://github.com/riii111/dotfiles/pull/42 --repo other/repo

huge_pr_calls="$tmpdir/huge-pr.calls"
if GH_TEST_CALLS_FILE="$huge_pr_calls" PATH="$tmpdir/bin:$PATH" \
  "$tmpdir/rust/gh-loupe" pr checks 2147483648 --repo riii111/dotfiles --failed-diagnostics \
  >"$tmpdir/huge-pr.stdout" 2>"$tmpdir/huge-pr.stderr"; then
  huge_pr_status=0
else
  huge_pr_status=$?
fi
test "$huge_pr_status" -eq 2
test ! -s "$tmpdir/huge-pr.stdout"
test ! -e "$huge_pr_calls"
grep -F 'GitHub GraphQL Int range' "$tmpdir/huge-pr.stderr" >/dev/null
if grep -Eiq 'panicked|stack backtrace' "$tmpdir/huge-pr.stderr"; then
  exit 1
fi

calls_file="$tmpdir/comments-invalid.calls"
if GH_TEST_CALLS_FILE="$calls_file" PATH="$tmpdir/bin:$PATH" \
  "$tmpdir/rust/gh-loupe" pr comments nope --repo riii111/dotfiles \
  >"$tmpdir/comments-invalid.stdout" 2>"$tmpdir/comments-invalid.stderr"; then
  comments_status=0
else
  comments_status=$?
fi
test "$comments_status" -eq 2
test ! -s "$tmpdir/comments-invalid.stdout"
test ! -e "$calls_file"

calls_file="$tmpdir/comments.calls"
run_comments comments-default "GH_TEST_CALLS_FILE=$calls_file" -- \
  pr comments 42 --repo riii111/dotfiles
jq -e '
  .data.comments == [
    {
      "id": "IC_a",
      "url": "https://example.test/a",
      "author": null,
      "body": "first by id",
      "createdAt": "2026-01-01T00:00:00Z",
      "updatedAt": "2026-01-01T02:00:00Z"
    },
    {
      "id": "IC_b",
      "url": "https://example.test/b",
      "author": "author",
      "body": "second by id",
      "createdAt": "2026-01-01T00:00:00Z",
      "updatedAt": "2026-01-01T01:00:00Z"
    },
    {
      "id": "IC_z",
      "url": "https://example.test/z",
      "author": "later",
      "body": "later",
      "createdAt": "2026-01-02T00:00:00Z",
      "updatedAt": "2026-01-02T01:00:00Z"
    }
  ] and
  (.data.comments | all(keys == ["author", "body", "createdAt", "id", "updatedAt", "url"])) and
  ([.. | objects | keys[]] | any(. == "pull_request_review_id" or . == "diff_hunk" or . == "review" or . == "pullRequest") | not)
' "$tmpdir/comments-default.comments.stdout" >/dev/null
test "$(cat "$calls_file")" = \
  'api --method GET --paginate --slurp repos/riii111/dotfiles/issues/42/comments'
test "$(wc -l <"$tmpdir/comments-default.comments.stdout")" -gt 1

run_comments comments-url-compact GH_PR_COMMENTS=empty -- \
  pr comments https://github.com/riii111/dotfiles/pull/42 --compact
jq -e '.data.comments == []' "$tmpdir/comments-url-compact.comments.stdout" >/dev/null
test "$(wc -l <"$tmpdir/comments-url-compact.comments.stdout")" -eq 1

calls_file="$tmpdir/comments-inferred.calls"
run_comments comments-inferred-repo "GH_TEST_CALLS_FILE=$calls_file" GH_PR_COMMENTS=empty -- \
  pr comments 42 --compact
test "$(sed -n '1p' "$calls_file")" = 'repo view --json nameWithOwner'
test "$(sed -n '2p' "$calls_file")" = \
  'api --method GET --paginate --slurp repos/riii111/dotfiles/issues/42/comments'
test "$(wc -l <"$calls_file")" -eq 2

assert_runtime_failure comments-invalid-page invalidResponse comments \
  GH_PR_COMMENTS=invalid-page -- pr comments 42 --repo riii111/dotfiles
assert_runtime_failure comments-invalid-item invalidResponse comments \
  GH_PR_COMMENTS=invalid-item -- pr comments 42 --repo riii111/dotfiles
assert_runtime_failure comments-missing-field invalidResponse comments \
  GH_PR_COMMENTS=missing-field -- pr comments 42 --repo riii111/dotfiles
assert_runtime_failure comments-wrong-type invalidResponse comments \
  GH_PR_COMMENTS=wrong-type -- pr comments 42 --repo riii111/dotfiles
assert_runtime_failure comments-page-failure network comments \
  GH_PR_COMMENTS_FAILURE=1 -- pr comments 42 --repo riii111/dotfiles
assert_runtime_failure comments-repo-auth authentication comments \
  GH_TEST_REPO_AUTH_FAILURE=1 -- pr comments 42

assert_argument_error threads-missing-target pr review-threads
assert_argument_error threads-abbreviated-repo pr review-threads 42 --rep riii111/dotfiles
assert_argument_error threads-abbreviated-compact pr review-threads 42 --comp
assert_argument_error threads-abbreviated-include-resolved pr review-threads 42 --incl
assert_argument_error threads-invalid-zero pr review-threads 0 --repo riii111/dotfiles
assert_argument_error threads-invalid-repo pr review-threads 42 --repo ../..
assert_argument_error threads-conflicting-repo \
  pr review-threads https://github.com/riii111/dotfiles/pull/42 --repo other/repo

run_review_threads threads-default GH_FAIL_RESOLVED_COMMENTS=1 -- \
  pr review-threads 42 --repo riii111/dotfiles
jq -e '
  .data.reviewThreads == [
    {
      "id": "thread-same-a",
      "isResolved": false,
      "isOutdated": true,
      "path": null,
      "line": null,
      "originalLine": null,
      "startLine": null,
      "diffSide": null,
      "commentCount": 1,
      "lastUpdatedAt": "2026-01-01T00:30:00Z"
    },
    {
      "id": "thread-same-b",
      "isResolved": false,
      "isOutdated": false,
      "path": "same.rs",
      "line": 12,
      "originalLine": 10,
      "startLine": null,
      "diffSide": "LEFT",
      "commentCount": 2,
      "lastUpdatedAt": "2026-01-03T00:00:00Z"
    },
    {
      "id": "thread-later",
      "isResolved": false,
      "isOutdated": false,
      "path": "later.rs",
      "line": 30,
      "originalLine": 29,
      "startLine": 28,
      "diffSide": "RIGHT",
      "commentCount": 1,
      "lastUpdatedAt": "2026-01-02T01:00:00Z"
    }
  ] and
  ([.. | objects | keys[]] | any(. == "body" or . == "author" or . == "url" or . == "diffHunk" or . == "resolvedBy") | not)
' "$tmpdir/threads-default.review_threads.stdout" >/dev/null
test "$(wc -l <"$tmpdir/threads-default.review_threads.stdout")" -gt 1

run_review_threads threads-including-resolved -- \
  pr review-threads 42 --repo riii111/dotfiles --include-resolved --compact
jq -e '
  .data.reviewThreads[0].id == "thread-resolved-summary" and
  .data.reviewThreads[0].commentCount == 2 and
  .data.reviewThreads[0].lastUpdatedAt == "2025-12-31T02:00:00Z" and
  (.data.reviewThreads | length == 4)
' "$tmpdir/threads-including-resolved.review_threads.stdout" >/dev/null
test "$(wc -l <"$tmpdir/threads-including-resolved.review_threads.stdout")" -eq 1

for mode in repeat cycle missing empty wrong-type; do
  case "$mode" in
    repeat) expected_calls=2 ;;
    empty|missing|wrong-type) expected_calls=1 ;;
    cycle) expected_calls=3 ;;
  esac
  calls_file="$tmpdir/threads-pagination-$mode-calls"
  assert_runtime_failure "threads-pagination-$mode" invalidResponse runtime \
    GH_TEST_CALLS_FILE="$calls_file" GH_THREAD_PAGINATION="$mode" -- \
    pr review-threads 42 --repo riii111/dotfiles
  test "$(grep -c 'api graphql' "$calls_file")" -eq "$expected_calls"
done

assert_runtime_failure threads-thread-page-failure network runtime \
  GH_TEST_THREAD_PAGE_FAILURE=1 -- pr review-threads 42 --repo riii111/dotfiles
assert_runtime_failure threads-comment-page-failure githubCli runtime \
  GH_TEST_COMMENT_PAGE_FAILURE=1 -- pr review-threads 42 --repo riii111/dotfiles
assert_runtime_failure threads-missing-pr notFound runtime \
  GH_TEST_MISSING_PR=1 -- pr review-threads 42 --repo riii111/dotfiles
assert_runtime_failure threads-graphql-error githubCli runtime \
  GH_TEST_GRAPHQL_ERROR=1 -- pr review-threads 42 --repo riii111/dotfiles
assert_runtime_failure threads-invalid-json invalidResponse runtime \
  GH_TEST_INVALID_JSON=1 -- pr review-threads 42 --repo riii111/dotfiles
assert_runtime_failure threads-repo-auth authentication runtime \
  GH_TEST_REPO_AUTH_FAILURE=1 -- pr review-threads 42

assert_argument_error thread-missing-target pr review-thread
assert_argument_error thread-missing-id pr review-thread 42 --repo riii111/dotfiles
assert_argument_error thread-abbreviated-repo pr review-thread 42 thread-detail --rep riii111/dotfiles
assert_argument_error thread-abbreviated-compact pr review-thread 42 thread-detail --comp
assert_argument_error thread-abbreviated-diff-hunk pr review-thread 42 thread-detail --incl
assert_argument_error thread-invalid-target pr review-thread nope thread-detail --repo riii111/dotfiles
assert_argument_error thread-conflicting-repo \
  pr review-thread https://github.com/riii111/dotfiles/pull/42 thread-detail --repo other/repo

run_review_thread thread-default -- pr review-thread 42 thread-detail --repo riii111/dotfiles
jq -e '
  .data.reviewThread == {
    "id": "thread-detail",
    "isResolved": false,
    "isOutdated": true,
    "path": "src/lib.rs",
    "line": 42,
    "originalLine": 39,
    "startLine": null,
    "diffSide": "RIGHT",
    "comments": [
      {
        "id": "comment-a",
        "url": "https://example.test/a",
        "body": "first by id",
        "author": null,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-03T00:00:00Z",
        "replyToId": "comment-b"
      },
      {
        "id": "comment-b",
        "url": "https://example.test/b",
        "body": "second by id",
        "author": "author",
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "replyToId": null
      },
      {
        "id": "comment-z",
        "url": "https://example.test/z",
        "body": "later",
        "author": "reviewer",
        "createdAt": "2026-01-02T00:00:00Z",
        "updatedAt": "2026-01-02T00:00:00Z",
        "replyToId": null
      }
    ]
  } and
  ([.. | objects | keys[]] | any(. == "diffHunk" or . == "databaseId" or . == "resolvedBy" or . == "replyTo" or . == "pullRequest") | not)
' "$tmpdir/thread-default.review_thread.stdout" >/dev/null
test "$(wc -l <"$tmpdir/thread-default.review_thread.stdout")" -gt 1

run_review_thread thread-diff-hunk -- \
  pr review-thread 42 thread-detail --repo riii111/dotfiles --include-diff-hunk --compact
jq -e '
  [.data.reviewThread.comments[].diffHunk] == ["@@ a", "@@ b", "@@ z"] and
  (.data.reviewThread.comments | all(keys == ["author", "body", "createdAt", "diffHunk", "id", "replyToId", "updatedAt", "url"]))
' "$tmpdir/thread-diff-hunk.review_thread.stdout" >/dev/null
test "$(wc -l <"$tmpdir/thread-diff-hunk.review_thread.stdout")" -eq 1

assert_runtime_failure thread-wrong-pr notFound runtime \
  GH_TEST_THREAD_DETAIL=wrong-pr -- pr review-thread 42 thread-detail --repo riii111/dotfiles
assert_runtime_failure thread-wrong-repo notFound runtime \
  GH_TEST_THREAD_DETAIL=wrong-repo -- pr review-thread 42 thread-detail --repo riii111/dotfiles
assert_runtime_failure thread-missing notFound runtime \
  GH_TEST_THREAD_DETAIL=missing -- pr review-thread 42 thread-detail --repo riii111/dotfiles
assert_runtime_failure thread-unresolved-node notFound runtime \
  GH_TEST_THREAD_DETAIL_NODE_ERROR=1 -- pr review-thread 42 missing --repo riii111/dotfiles
assert_runtime_failure thread-wrong-type notFound runtime \
  GH_TEST_THREAD_DETAIL=wrong-type -- pr review-thread 42 thread-detail --repo riii111/dotfiles
assert_runtime_failure thread-comment-page-failure githubCli runtime \
  GH_TEST_THREAD_DETAIL_PAGE_FAILURE=1 -- pr review-thread 42 thread-detail --repo riii111/dotfiles

for mode in repeat cycle; do
  case "$mode" in
    repeat) expected_calls=2 ;;
    cycle) expected_calls=3 ;;
  esac
  calls_file="$tmpdir/thread-detail-pagination-$mode-calls"
  assert_runtime_failure "thread-detail-pagination-$mode" invalidResponse runtime \
    GH_TEST_CALLS_FILE="$calls_file" GH_THREAD_DETAIL_PAGINATION="$mode" -- \
    pr review-thread 42 thread-detail --repo riii111/dotfiles
  test "$(grep -c 'api graphql' "$calls_file")" -eq "$expected_calls"
done
