#!/usr/bin/env bash

set -Eeuo pipefail

trap 'status=$?; printf "%s:%s: assertion failed (exit %s): %s\n" "${BASH_SOURCE[0]}" "$LINENO" "$status" "$BASH_COMMAND" >&2' ERR

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
source "$repo_root/tests/assertions.sh"
export GH_FIXTURE_DATA="$repo_root/tests/fixtures/data/gh"
tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/gh-loupe-pr-checks.XXXXXX")"
trap 'rm -rf "$tmpdir"' EXIT
mkdir -p "$tmpdir/bin"
cp "$repo_root/tests/fixtures/gh" "$tmpdir/bin/gh"
chmod +x "$tmpdir/bin/gh"

run_checks() {
  env PATH="$tmpdir/bin:$PATH" GH_TEST_CHECKS_NEW="$1" \
    "$GH_LOUPE_BIN" pr checks 42 --repo riii111/dotfiles "${@:2}"
}

GH_TEST_ARGS_FILE="$tmpdir/default-args" run_checks success >"$tmpdir/pretty.json"
assert_json '
  .schemaVersion == 1 and
  (.observedAt | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
  [.data.checks[].name] == ["alpha", "alpha", "zeta"] and
  [.data.checks[0:2][].link] == ["https://example.test/a", "https://example.test/b"] and
  (.data.checks[] | keys == ["bucket", "completedAt", "link", "name", "startedAt", "state", "workflow"])
' "$tmpdir/pretty.json" >/dev/null
test "$(wc -l <"$tmpdir/pretty.json" | tr -d ' ')" -gt 1
if grep -Fx -- '--required' "$tmpdir/default-args" >/dev/null; then
  exit 1
fi

GH_TEST_ARGS_FILE="$tmpdir/args" run_checks success --required --compact >"$tmpdir/compact.json"
test "$(wc -l <"$tmpdir/compact.json" | tr -d ' ')" -eq 1
grep -Fx -- '--required' "$tmpdir/args" >/dev/null
assert_json '.data.checks | length == 3' "$tmpdir/compact.json" >/dev/null

run_checks failed --failed-only --compact >"$tmpdir/failed-only.json"
assert_json '
  .data.summary == {"total":5,"passed":2,"pending":1,"failed":2} and
  [.data.checks[].name] == ["cancelled", "failed"] and
  [.data.checks[].bucket] == ["cancel", "fail"]
' "$tmpdir/failed-only.json" >/dev/null

GH_TEST_ARGS_FILE="$tmpdir/failed-only-required-args" \
  run_checks failed --failed-only --required --compact >"$tmpdir/failed-only-required.json"
if grep -Fx -- '--failed-only' "$tmpdir/failed-only-required-args" >/dev/null; then
  exit 1
fi
grep -Fx -- '--required' "$tmpdir/failed-only-required-args" >/dev/null
assert_json '.data.summary.failed == 2 and (.data.checks | length) == 2' \
  "$tmpdir/failed-only-required.json" >/dev/null

run_checks success --failed-only --compact >"$tmpdir/failed-only-empty.json"
assert_json '.data.summary == {"total":3,"passed":2,"pending":1,"failed":0} and .data.checks == []' \
  "$tmpdir/failed-only-empty.json" >/dev/null

if run_checks invalid-with-failure --failed-only \
  >"$tmpdir/failed-only-invalid.stdout" 2>"$tmpdir/failed-only-invalid.stderr"; then
  status=0
else
  status=$?
fi
test "$status" -eq 1
test ! -s "$tmpdir/failed-only-invalid.stdout"
assert_json '.schemaVersion == 1 and .error.kind == "invalidResponse"' \
  "$tmpdir/failed-only-invalid.stderr" >/dev/null

"$GH_LOUPE_BIN" pr checks --help >"$tmpdir/help.stdout"
grep -F -- '--failed-only' "$tmpdir/help.stdout" >/dev/null

if (
  export GH_TEST_CHECKS_STATUS=unexpected
  run_checks success \
  >"$tmpdir/unexpected-status.stdout" 2>"$tmpdir/unexpected-status.stderr"); then
  status=0
else
  status=$?
fi
test "$status" -eq 1
test ! -s "$tmpdir/unexpected-status.stdout"
assert_json '.schemaVersion == 1 and .error.kind == "githubCli"' \
  "$tmpdir/unexpected-status.stderr" >/dev/null

if GH_TEST_SIGNAL=1 run_checks signal \
  >"$tmpdir/signal.stdout" 2>"$tmpdir/signal.stderr"; then
  status=0
else
  status=$?
fi
test "$status" -eq 1
test ! -s "$tmpdir/signal.stdout"
assert_json '.schemaVersion == 1 and .error.kind == "githubCli" and .error.message == "GitHub CLI terminated by signal"' \
  "$tmpdir/signal.stderr" >/dev/null

run_checks nullable --compact >"$tmpdir/nullable.json"
assert_json '
  .data.checks[0].link == null and
  .data.checks[0].workflow == null and
  .data.checks[0].startedAt == null and
  .data.checks[0].completedAt == null and
  .data.checks[1].workflow == "CI" and
  .data.checks[1].startedAt == "2026-08-11T09:00:00Z" and
  .data.checks[1].completedAt == "2026-08-11T09:05:00Z"
' "$tmpdir/nullable.json" >/dev/null

run_checks no-required --required --compact >"$tmpdir/no-required.json"
assert_json '.data.checks == []' "$tmpdir/no-required.json" >/dev/null

run_checks no-checks --compact >"$tmpdir/no-checks.json"
assert_json '.data.checks == []' "$tmpdir/no-checks.json" >/dev/null

for mode in missing wrong-type wrong-metadata-type unknown object; do
  if run_checks "$mode" >"$tmpdir/$mode.stdout" 2>"$tmpdir/$mode.stderr"; then
    status=0
  else
    status=$?
  fi
  test "$status" -eq 1
  test ! -s "$tmpdir/$mode.stdout"
  test "$(wc -l <"$tmpdir/$mode.stderr" | tr -d ' ')" -eq 1
  assert_json '.schemaVersion == 1 and .error.kind == "invalidResponse" and .error.retryable == false and .error.retryAfterSeconds == null' \
    "$tmpdir/$mode.stderr" >/dev/null
done

if run_checks authentication --failed-only >"$tmpdir/auth.stdout" 2>"$tmpdir/auth.stderr"; then
  status=0
else
  status=$?
fi
test "$status" -eq 1
test ! -s "$tmpdir/auth.stdout"
assert_json '.error.kind == "authentication" and .error.retryable == false' "$tmpdir/auth.stderr" >/dev/null

for mode in missing-pr missing-repository; do
  if run_checks "$mode" >"$tmpdir/$mode.stdout" 2>"$tmpdir/$mode.stderr"; then
    status=0
  else
    status=$?
  fi
  test "$status" -eq 1
  test ! -s "$tmpdir/$mode.stdout"
  assert_json '.error.kind == "notFound" and .error.retryable == false' \
    "$tmpdir/$mode.stderr" >/dev/null
done
