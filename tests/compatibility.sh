#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
reference="$repo_root/tests/fixtures/reference_gh_read.py"
tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/gh-read-compatibility.XXXXXX")"
trap 'rm -rf "$tmpdir"' EXIT
mkdir -p "$tmpdir/bin" "$tmpdir/python" "$tmpdir/rust"
cp "$repo_root/tests/fixtures/gh" "$tmpdir/bin/gh"
chmod +x "$tmpdir/bin/gh"
cp "$reference" "$tmpdir/python/gh-read"
chmod +x "$tmpdir/python/gh-read"
ln -s "$GH_READ_BIN" "$tmpdir/rust/gh-read"

compare_case() {
  local name="$1"
  shift
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  set +e
  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/python/gh-read" "$@" \
    >"$tmpdir/$name.python.stdout" 2>"$tmpdir/$name.python.stderr"
  local python_status=$?
  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-read" "$@" \
    >"$tmpdir/$name.rust.stdout" 2>"$tmpdir/$name.rust.stderr"
  local rust_status=$?
  set -e

  if [ "$python_status" -ne "$rust_status" ]; then
    printf '%s: exit status differs: Python=%s Rust=%s\n' "$name" "$python_status" "$rust_status" >&2
    return 1
  fi
  cmp "$tmpdir/$name.python.stdout" "$tmpdir/$name.rust.stdout"
  cmp "$tmpdir/$name.python.stderr" "$tmpdir/$name.rust.stderr"
}

assert_rust_failure() {
  local name="$1"
  local expected_status="$2"
  local expected_stderr="$3"
  shift 3
  local -a environment=()
  while [ "$1" != "--" ]; do
    environment+=("$1")
    shift
  done
  shift

  set +e
  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$tmpdir/rust/gh-read" "$@" \
    >"$tmpdir/$name.rust.stdout" 2>"$tmpdir/$name.rust.stderr"
  local rust_status=$?
  set -e

  if [ "$rust_status" -ne "$expected_status" ]; then
    printf '%s: exit status differs: expected=%s Rust=%s\n' \
      "$name" "$expected_status" "$rust_status" >&2
    return 1
  fi
  test ! -s "$tmpdir/$name.rust.stdout"
  printf '%s\n' "$expected_stderr" >"$tmpdir/$name.expected.stderr"
  cmp "$tmpdir/$name.expected.stderr" "$tmpdir/$name.rust.stderr"
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

compare_case root-help -- --help
compare_case pr-help -- pr --help
compare_case issue-help -- issue --help
compare_case root-missing-resource --
compare_case pr-missing-target -- pr
compare_case issue-missing-target -- issue
compare_case pr-missing-repo-value -- pr --repo
compare_case pr-option-instead-of-repo-value -- pr --repo --compact 42
compare_case pr-unrecognized-option -- pr 42 --bogus
compare_case issue-pr-only-option -- issue 42 --include-resolved
compare_case root-end-options -- -- pr 0
compare_case pr-end-options -- pr -- 42
compare_case pr-default GH_FAIL_RESOLVED_COMMENTS=1 -- pr 42
compare_case pr-pages -- pr 42 --include-resolved
compare_case pr-url -- pr https://github.com/riii111/dotfiles/pull/42
compare_case repo-equals -- pr 42 --repo=riii111/dotfiles
compare_case abbreviated-repo -- pr 42 --rep riii111/dotfiles
compare_case abbreviated-repo-equals -- pr 42 --rep=riii111/dotfiles
compare_case abbreviated-compact -- pr 42 --comp
compare_case abbreviated-include-resolved -- pr 42 --incl
compare_case abbreviated-help -- pr --hel
assert_argument_error abbreviated-repo-status pr 42 --rep riii111/dotfiles
assert_argument_error abbreviated-compact-status pr 42 --comp
assert_argument_error abbreviated-include-resolved-status pr 42 --incl
assert_argument_error unknown-option-status pr 42 --bogus
compare_case options-before-target -- pr --compact --repo riii111/dotfiles 42
compare_case pr-compact -- pr 42 --compact
compare_case checks-pending GH_TEST_CHECKS_STATUS=pending -- pr 42
compare_case checks-failure GH_TEST_CHECKS_STATUS=failure -- pr 42
compare_case issue-default -- issue 42
compare_case issue-url-compact -- issue https://github.com/riii111/dotfiles/issues/42 --compact
compare_case utf8 GH_TEST_UTF8=1 -- issue 42
compare_case invalid-zero -- pr 0
compare_case arbitrary-precision-number -- pr 18446744073709551616
compare_case invalid-unicode-number -- pr ²
compare_case invalid-repo -- pr 42 --repo ../..
compare_case conflicting-pr-repo -- pr https://github.com/riii111/dotfiles/pull/42 --repo other/repo
compare_case conflicting-issue-repo -- issue https://github.com/riii111/dotfiles/issues/42 --repo other/repo
compare_case missing-pr GH_TEST_MISSING_PR=1 -- pr 42
compare_case missing-issue GH_TEST_MISSING_ISSUE=1 -- issue 42
compare_case gh-failure GH_TEST_FAILURE=1 -- pr 42
compare_case stdin-failure GH_TEST_STDIN_FAILURE=1 -- pr 42 --repo riii111/dotfiles
compare_case pagination-failure GH_TEST_PAGINATION_FAILURE=1 -- pr 42
compare_case graphql-error GH_TEST_GRAPHQL_ERROR=1 -- pr 42
assert_rust_failure invalid-json 1 \
  'GitHub returned invalid JSON: expected ident at line 1 column 2' \
  GH_TEST_INVALID_JSON=1 -- pr 42

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
