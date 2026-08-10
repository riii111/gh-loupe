#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/gh-read-pr-checks.XXXXXX")"
trap 'rm -rf "$tmpdir"' EXIT
mkdir -p "$tmpdir/bin"
cp "$repo_root/tests/fixtures/gh" "$tmpdir/bin/gh"
chmod +x "$tmpdir/bin/gh"

run_checks() {
  env PATH="$tmpdir/bin:$PATH" GH_TEST_CHECKS_NEW="$1" \
    "$GH_READ_BIN" pr checks 42 --repo riii111/dotfiles "${@:2}"
}

GH_TEST_ARGS_FILE="$tmpdir/default-args" run_checks success >"$tmpdir/pretty.json"
jq -e '
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
jq -e '.data.checks | length == 3' "$tmpdir/compact.json" >/dev/null

run_checks no-required --required --compact >"$tmpdir/no-required.json"
jq -e '.data.checks == []' "$tmpdir/no-required.json" >/dev/null

for mode in missing wrong-type unknown object; do
  set +e
  run_checks "$mode" >"$tmpdir/$mode.stdout" 2>"$tmpdir/$mode.stderr"
  status=$?
  set -e
  test "$status" -eq 1
  test ! -s "$tmpdir/$mode.stdout"
  test "$(wc -l <"$tmpdir/$mode.stderr" | tr -d ' ')" -eq 1
  jq -e '.schemaVersion == 1 and .error.kind == "invalidResponse" and .error.retryable == false and .error.retryAfterSeconds == null' \
    "$tmpdir/$mode.stderr" >/dev/null
done

set +e
run_checks authentication >"$tmpdir/auth.stdout" 2>"$tmpdir/auth.stderr"
status=$?
set -e
test "$status" -eq 1
test ! -s "$tmpdir/auth.stdout"
jq -e '.error.kind == "authentication" and .error.retryable == false' "$tmpdir/auth.stderr" >/dev/null

for mode in missing-pr missing-repository; do
  set +e
  run_checks "$mode" >"$tmpdir/$mode.stdout" 2>"$tmpdir/$mode.stderr"
  status=$?
  set -e
  test "$status" -eq 1
  test ! -s "$tmpdir/$mode.stdout"
  jq -e '.error.kind == "notFound" and .error.retryable == false' \
    "$tmpdir/$mode.stderr" >/dev/null
done

for option in --failed-diagnostics --include-failed-logs --timeout --quiet; do
  set +e
  env PATH="$tmpdir/bin:$PATH" "$GH_READ_BIN" pr checks 42 --repo riii111/dotfiles "$option" \
    >"$tmpdir/argument.stdout" 2>"$tmpdir/argument.stderr"
  status=$?
  set -e
  test "$status" -eq 2
  test ! -s "$tmpdir/argument.stdout"
done
