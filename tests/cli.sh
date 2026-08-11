#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/gh-read-cli.XXXXXX")"
trap 'rm -rf "$tmpdir"' EXIT
export GH_READ_PACKAGE_VERSION="$(cargo metadata --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" | jq -r '.packages[] | select(.name == "gh-read") | .version')"
mkdir -p "$tmpdir/bin" "$tmpdir/rust"
cp "$repo_root/tests/fixtures/gh" "$tmpdir/bin/gh"
chmod +x "$tmpdir/bin/gh"
ln -s "$GH_READ_BIN" "$tmpdir/rust/gh-read"

run_cli() {
  local name="$1"
  shift
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-read" "$@" \
    >"$tmpdir/$name.stdout" 2>"$tmpdir/$name.stderr"
}

assert_argument_error() {
  local name="$1"
  shift

  set +e
  env PATH="$tmpdir/bin:$PATH" "$tmpdir/rust/gh-read" "$@" \
    >"$tmpdir/$name.argument.stdout" 2>"$tmpdir/$name.argument.stderr"
  local status=$?
  set -e

  if [ "$status" -ne 2 ]; then
    printf '%s: expected argument error status 2, got %s\n' "$name" "$status" >&2
    return 1
  fi
  test ! -s "$tmpdir/$name.argument.stdout"
  test -s "$tmpdir/$name.argument.stderr"
}

run_threads() {
  local name="$1"
  shift
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-read" "$@" \
    >"$tmpdir/$name.threads.stdout" 2>"$tmpdir/$name.threads.stderr"
  test ! -s "$tmpdir/$name.threads.stderr"
  jq -e '
    .schemaVersion == 1 and
    (.observedAt | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
    (.data | keys == ["threads"])
  ' "$tmpdir/$name.threads.stdout" >/dev/null
}

run_thread() {
  local name="$1"
  shift
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-read" "$@" \
    >"$tmpdir/$name.thread.stdout" 2>"$tmpdir/$name.thread.stderr"
  test ! -s "$tmpdir/$name.thread.stderr"
  jq -e '
    .schemaVersion == 1 and
    (.observedAt | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
    (.data | keys == ["thread"])
  ' "$tmpdir/$name.thread.stdout" >/dev/null
}

assert_threads_runtime_failure() {
  local name="$1"
  local expected_kind="$2"
  shift 2
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  set +e
  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-read" "$@" \
    >"$tmpdir/$name.runtime.stdout" 2>"$tmpdir/$name.runtime.stderr"
  local status=$?
  set -e

  test "$status" -ne 0
  test ! -s "$tmpdir/$name.runtime.stdout"
  test "$(wc -l <"$tmpdir/$name.runtime.stderr")" -eq 1
  jq -e --arg kind "$expected_kind" '
    .schemaVersion == 1 and
    .error.kind == $kind and
    (.error.message | type == "string") and
    (.error.retryable | type == "boolean") and
    (.error.retryAfterSeconds == null)
  ' "$tmpdir/$name.runtime.stderr" >/dev/null
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

  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-read" "$@" \
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

  set +e
  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-read" "$@" \
    >"$tmpdir/$name.overview.stdout" 2>"$tmpdir/$name.overview.stderr"
  local status=$?
  set -e

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

  set +e
  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-read" "$@" \
    >"$tmpdir/$name.overview.stdout" 2>"$tmpdir/$name.overview.stderr"
  local status=$?
  set -e

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

  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-read" "$@" \
    >"$tmpdir/$name.comments.stdout" 2>"$tmpdir/$name.comments.stderr"
  test ! -s "$tmpdir/$name.comments.stderr"
  jq -e '
    .schemaVersion == 1 and
    (.observedAt | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
    (.data | keys == ["comments"])
  ' "$tmpdir/$name.comments.stdout" >/dev/null
}

assert_comments_runtime_failure() {
  local name="$1"
  local expected_kind="$2"
  shift 2
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  set +e
  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-read" "$@" \
    >"$tmpdir/$name.comments.stdout" 2>"$tmpdir/$name.comments.stderr"
  local status=$?
  set -e

  test "$status" -ne 0
  test ! -s "$tmpdir/$name.comments.stdout"
  test "$(wc -l <"$tmpdir/$name.comments.stderr")" -eq 1
  jq -e --arg kind "$expected_kind" '
    .schemaVersion == 1 and
    .error.kind == $kind and
    (.error.message | type == "string") and
    (.error.retryable | type == "boolean") and
    (.error.retryAfterSeconds == null)
  ' "$tmpdir/$name.comments.stderr" >/dev/null
}

run_cli root-help -- --help
test ! -s "$tmpdir/root-help.stderr"
grep -F 'usage: gh-read [-h] [--version] {pr,issue} ...' "$tmpdir/root-help.stdout" >/dev/null

run_cli root-version -- --version
test ! -s "$tmpdir/root-version.stderr"
test "$(cat "$tmpdir/root-version.stdout")" = "gh-read $GH_READ_PACKAGE_VERSION"

run_cli pr-help -- pr --help
test ! -s "$tmpdir/pr-help.stderr"
grep -F 'usage: gh-read pr [-h] {overview,comments,reviews,threads,thread,checks} ...' "$tmpdir/pr-help.stdout" >/dev/null
for subcommand in overview comments reviews threads thread checks; do
  grep -E "^    $subcommand  +" "$tmpdir/pr-help.stdout" >/dev/null
done
if grep -E '^    (full|legacy)  +' "$tmpdir/pr-help.stdout" >/dev/null; then
  exit 1
fi

set +e
GH_TEST_CALLS_FILE="$tmpdir/bare-pr.calls" PATH="$tmpdir/bin:$PATH" \
  "$tmpdir/rust/gh-read" pr 42 >"$tmpdir/bare-pr.stdout" 2>"$tmpdir/bare-pr.stderr"
bare_status=$?
set -e
test "$bare_status" -eq 2
test ! -s "$tmpdir/bare-pr.stdout"
test ! -e "$tmpdir/bare-pr.calls"
grep -Fx 'usage: gh-read pr [-h] {overview,comments,reviews,threads,thread,checks} ...' "$tmpdir/bare-pr.stderr" >/dev/null
grep -Fx 'gh-read pr: error: the following arguments are required: subcommand' "$tmpdir/bare-pr.stderr" >/dev/null

assert_argument_error root-missing-resource
assert_argument_error pr-missing-subcommand pr
assert_argument_error issue-missing-target issue
assert_argument_error issue-pr-only-option issue 42 --include-resolved

run_cli issue-default -- issue 42
test ! -s "$tmpdir/issue-default.stderr"
jq -e '. == {
  "issue":{"number":42,"title":"Issue","state":"open","body":"body"},
  "comments":[{"id":2,"body":"conversation"},{"id":4,"body":"conversation page 2"}]
}' "$tmpdir/issue-default.stdout" >/dev/null

run_cli issue-url-compact -- issue https://github.com/riii111/dotfiles/issues/42 --compact
test ! -s "$tmpdir/issue-url-compact.stderr"
test "$(wc -l <"$tmpdir/issue-url-compact.stdout" | tr -d ' ')" -eq 1
jq -e '.issue.number == 42 and (.comments | length == 2)' "$tmpdir/issue-url-compact.stdout" >/dev/null

run_cli issue-utf8 GH_TEST_UTF8=1 -- issue 42
jq -e '.issue.title == "日本語のIssue" and .issue.body == "ずんだ"' "$tmpdir/issue-utf8.stdout" >/dev/null

set +e
env PATH="$tmpdir/bin:$PATH" GH_TEST_MISSING_ISSUE=1 "$tmpdir/rust/gh-read" issue 42 \
  >"$tmpdir/issue-missing.stdout" 2>"$tmpdir/issue-missing.stderr"
issue_status=$?
set -e
test "$issue_status" -eq 44
test ! -s "$tmpdir/issue-missing.stdout"
test "$(cat "$tmpdir/issue-missing.stderr")" = 'missing issue'

set +e
env PATH="$tmpdir/bin:$PATH" GH_TEST_INVALID_JSON=1 "$tmpdir/rust/gh-read" issue 42 \
  >"$tmpdir/issue-invalid-json.stdout" 2>"$tmpdir/issue-invalid-json.stderr"
issue_status=$?
set -e
test "$issue_status" -eq 1
test ! -s "$tmpdir/issue-invalid-json.stdout"
test "$(cat "$tmpdir/issue-invalid-json.stderr")" = \
  'GitHub returned invalid JSON: expected ident at line 1 column 2'

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

calls_file="$tmpdir/comments-invalid.calls"
set +e
GH_TEST_CALLS_FILE="$calls_file" PATH="$tmpdir/bin:$PATH" \
  "$tmpdir/rust/gh-read" pr comments nope --repo riii111/dotfiles \
  >"$tmpdir/comments-invalid.stdout" 2>"$tmpdir/comments-invalid.stderr"
comments_status=$?
set -e
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

assert_comments_runtime_failure comments-invalid-page invalidResponse \
  GH_PR_COMMENTS=invalid-page -- pr comments 42 --repo riii111/dotfiles
assert_comments_runtime_failure comments-invalid-item invalidResponse \
  GH_PR_COMMENTS=invalid-item -- pr comments 42 --repo riii111/dotfiles
assert_comments_runtime_failure comments-missing-field invalidResponse \
  GH_PR_COMMENTS=missing-field -- pr comments 42 --repo riii111/dotfiles
assert_comments_runtime_failure comments-wrong-type invalidResponse \
  GH_PR_COMMENTS=wrong-type -- pr comments 42 --repo riii111/dotfiles
assert_comments_runtime_failure comments-page-failure network \
  GH_PR_COMMENTS_FAILURE=1 -- pr comments 42 --repo riii111/dotfiles
assert_comments_runtime_failure comments-repo-auth authentication \
  GH_TEST_REPO_AUTH_FAILURE=1 -- pr comments 42

assert_argument_error threads-missing-target pr threads
assert_argument_error threads-abbreviated-repo pr threads 42 --rep riii111/dotfiles
assert_argument_error threads-abbreviated-compact pr threads 42 --comp
assert_argument_error threads-abbreviated-include-resolved pr threads 42 --incl
assert_argument_error threads-invalid-zero pr threads 0 --repo riii111/dotfiles
assert_argument_error threads-invalid-repo pr threads 42 --repo ../..
assert_argument_error threads-conflicting-repo \
  pr threads https://github.com/riii111/dotfiles/pull/42 --repo other/repo

run_threads threads-default GH_FAIL_RESOLVED_COMMENTS=1 -- \
  pr threads 42 --repo riii111/dotfiles
jq -e '
  .data.threads == [
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
' "$tmpdir/threads-default.threads.stdout" >/dev/null
test "$(wc -l <"$tmpdir/threads-default.threads.stdout")" -gt 1

run_threads threads-including-resolved -- \
  pr threads 42 --repo riii111/dotfiles --include-resolved --compact
jq -e '
  .data.threads[0].id == "thread-resolved-summary" and
  .data.threads[0].commentCount == 2 and
  .data.threads[0].lastUpdatedAt == "2025-12-31T02:00:00Z" and
  (.data.threads | length == 4)
' "$tmpdir/threads-including-resolved.threads.stdout" >/dev/null
test "$(wc -l <"$tmpdir/threads-including-resolved.threads.stdout")" -eq 1

for mode in repeat cycle missing empty wrong-type; do
  case "$mode" in
    repeat) expected_calls=2 ;;
    empty|missing|wrong-type) expected_calls=1 ;;
    cycle) expected_calls=3 ;;
  esac
  calls_file="$tmpdir/threads-pagination-$mode-calls"
  assert_threads_runtime_failure "threads-pagination-$mode" invalidResponse \
    GH_TEST_CALLS_FILE="$calls_file" GH_THREAD_PAGINATION="$mode" -- \
    pr threads 42 --repo riii111/dotfiles
  test "$(grep -c 'api graphql' "$calls_file")" -eq "$expected_calls"
done

assert_threads_runtime_failure threads-thread-page-failure network \
  GH_TEST_THREAD_PAGE_FAILURE=1 -- pr threads 42 --repo riii111/dotfiles
assert_threads_runtime_failure threads-comment-page-failure githubCli \
  GH_TEST_COMMENT_PAGE_FAILURE=1 -- pr threads 42 --repo riii111/dotfiles
assert_threads_runtime_failure threads-missing-pr notFound \
  GH_TEST_MISSING_PR=1 -- pr threads 42 --repo riii111/dotfiles
assert_threads_runtime_failure threads-graphql-error githubCli \
  GH_TEST_GRAPHQL_ERROR=1 -- pr threads 42 --repo riii111/dotfiles
assert_threads_runtime_failure threads-invalid-json invalidResponse \
  GH_TEST_INVALID_JSON=1 -- pr threads 42 --repo riii111/dotfiles
assert_threads_runtime_failure threads-repo-auth authentication \
  GH_TEST_REPO_AUTH_FAILURE=1 -- pr threads 42

assert_argument_error thread-missing-target pr thread
assert_argument_error thread-missing-id pr thread 42 --repo riii111/dotfiles
assert_argument_error thread-abbreviated-repo pr thread 42 thread-detail --rep riii111/dotfiles
assert_argument_error thread-abbreviated-compact pr thread 42 thread-detail --comp
assert_argument_error thread-abbreviated-diff-hunk pr thread 42 thread-detail --incl
assert_argument_error thread-invalid-target pr thread nope thread-detail --repo riii111/dotfiles
assert_argument_error thread-conflicting-repo \
  pr thread https://github.com/riii111/dotfiles/pull/42 thread-detail --repo other/repo

run_thread thread-default -- pr thread 42 thread-detail --repo riii111/dotfiles
jq -e '
  .data.thread == {
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
' "$tmpdir/thread-default.thread.stdout" >/dev/null
test "$(wc -l <"$tmpdir/thread-default.thread.stdout")" -gt 1

run_thread thread-diff-hunk -- \
  pr thread 42 thread-detail --repo riii111/dotfiles --include-diff-hunk --compact
jq -e '
  [.data.thread.comments[].diffHunk] == ["@@ a", "@@ b", "@@ z"] and
  (.data.thread.comments | all(keys == ["author", "body", "createdAt", "diffHunk", "id", "replyToId", "updatedAt", "url"]))
' "$tmpdir/thread-diff-hunk.thread.stdout" >/dev/null
test "$(wc -l <"$tmpdir/thread-diff-hunk.thread.stdout")" -eq 1

assert_threads_runtime_failure thread-wrong-pr notFound \
  GH_TEST_THREAD_DETAIL=wrong-pr -- pr thread 42 thread-detail --repo riii111/dotfiles
assert_threads_runtime_failure thread-wrong-repo notFound \
  GH_TEST_THREAD_DETAIL=wrong-repo -- pr thread 42 thread-detail --repo riii111/dotfiles
assert_threads_runtime_failure thread-missing notFound \
  GH_TEST_THREAD_DETAIL=missing -- pr thread 42 thread-detail --repo riii111/dotfiles
assert_threads_runtime_failure thread-unresolved-node notFound \
  GH_TEST_THREAD_DETAIL_NODE_ERROR=1 -- pr thread 42 missing --repo riii111/dotfiles
assert_threads_runtime_failure thread-wrong-type notFound \
  GH_TEST_THREAD_DETAIL=wrong-type -- pr thread 42 thread-detail --repo riii111/dotfiles
assert_threads_runtime_failure thread-comment-page-failure githubCli \
  GH_TEST_THREAD_DETAIL_PAGE_FAILURE=1 -- pr thread 42 thread-detail --repo riii111/dotfiles

for mode in repeat cycle; do
  case "$mode" in
    repeat) expected_calls=2 ;;
    cycle) expected_calls=3 ;;
  esac
  calls_file="$tmpdir/thread-detail-pagination-$mode-calls"
  assert_threads_runtime_failure "thread-detail-pagination-$mode" invalidResponse \
    GH_TEST_CALLS_FILE="$calls_file" GH_THREAD_DETAIL_PAGINATION="$mode" -- \
    pr thread 42 thread-detail --repo riii111/dotfiles
  test "$(grep -c 'api graphql' "$calls_file")" -eq "$expected_calls"
done
