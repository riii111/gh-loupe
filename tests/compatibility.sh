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
