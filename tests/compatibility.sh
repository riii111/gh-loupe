#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
reference="$repo_root/tests/fixtures/reference_gh_read.py"
tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/gh-read-compatibility.XXXXXX")"
trap 'rm -rf "$tmpdir"' EXIT
mkdir -p "$tmpdir/bin"
cp "$repo_root/tests/fixtures/gh" "$tmpdir/bin/gh"
chmod +x "$tmpdir/bin/gh"

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
  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" python3 "$reference" "$@" \
    >"$tmpdir/$name.python.stdout" 2>"$tmpdir/$name.python.stderr"
  local python_status=$?
  env PATH="$tmpdir/bin:$PATH" "${environment[@]}" "$GH_READ_BIN" "$@" \
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

compare_case pr-default GH_FAIL_RESOLVED_COMMENTS=1 -- pr 42
compare_case pr-pages -- pr 42 --include-resolved
compare_case pr-url -- pr https://github.com/riii111/dotfiles/pull/42
compare_case repo-equals -- pr 42 --repo=riii111/dotfiles
compare_case options-before-target -- pr --compact --repo riii111/dotfiles 42
compare_case pr-compact -- pr 42 --compact
compare_case checks-pending GH_TEST_CHECKS_STATUS=pending -- pr 42
compare_case checks-failure GH_TEST_CHECKS_STATUS=failure -- pr 42
compare_case issue-default -- issue 42
compare_case issue-url-compact -- issue https://github.com/riii111/dotfiles/issues/42 --compact
compare_case utf8 GH_TEST_UTF8=1 -- issue 42
compare_case invalid-zero -- pr 0
compare_case invalid-unicode-number -- pr ²
compare_case invalid-repo -- pr 42 --repo ../..
compare_case conflicting-pr-repo -- pr https://github.com/riii111/dotfiles/pull/42 --repo other/repo
compare_case conflicting-issue-repo -- issue https://github.com/riii111/dotfiles/issues/42 --repo other/repo
compare_case missing-pr GH_TEST_MISSING_PR=1 -- pr 42
compare_case missing-issue GH_TEST_MISSING_ISSUE=1 -- issue 42
compare_case gh-failure GH_TEST_FAILURE=1 -- pr 42
compare_case pagination-failure GH_TEST_PAGINATION_FAILURE=1 -- pr 42
compare_case graphql-error GH_TEST_GRAPHQL_ERROR=1 -- pr 42
