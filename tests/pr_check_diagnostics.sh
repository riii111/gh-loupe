#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/gh-read-diagnostics.XXXXXX")"
trap 'rm -rf "$tmpdir"' EXIT
mkdir -p "$tmpdir/bin"
cp "$repo_root/tests/fixtures/gh-diagnostics" "$tmpdir/bin/gh"
chmod +x "$tmpdir/bin/gh"

run_diagnostics() {
  local repository="${GH_DIAGNOSTICS_REPOSITORY:-owner/repo}"
  env PATH="$tmpdir/bin:$PATH" GH_DIAGNOSTICS_MODE="${1:-normal}" \
    "$GH_READ_BIN" pr checks 42 --repo "$repository" "${@:2}"
}

GH_DIAGNOSTICS_CALLS="$tmpdir/calls" run_diagnostics normal --failed-diagnostics --compact \
  >"$tmpdir/diagnostics.json" 2>"$tmpdir/diagnostics.stderr"
grep -Fx 'gh-read: collecting diagnostics for 2 failed checks' "$tmpdir/diagnostics.stderr" >/dev/null
jq -e '
  [.data.checks[].name] == ["actions-failure", "external-cancel", "pass"] and
  [.data.checks[0].annotations[].path] == ["a.rs", "a.rs", "z.rs"] and
  [.data.checks[0].annotations[].startLine] == [1, 2, 9] and
  .data.checks[1].annotations == [] and
  (.data.checks[2] | has("annotations") | not) and
  ([.data.checks[] | has("log")] | all(. == false))
' "$tmpdir/diagnostics.json" >/dev/null
grep -F -- 'check-runs/100/annotations?per_page=100' "$tmpdir/calls" >/dev/null

GH_DIAGNOSTICS_CALLS="$tmpdir/collision-calls" run_diagnostics status-collision \
  --failed-diagnostics --quiet --compact >"$tmpdir/status-collision.json"
jq -e '.data.checks[0].annotations == []' "$tmpdir/status-collision.json" >/dev/null
if grep -F -- 'check-runs/102/annotations' "$tmpdir/collision-calls" >/dev/null; then
  exit 1
fi

run_diagnostics normal --include-failed-logs --quiet --compact \
  >"$tmpdir/logs.json" 2>"$tmpdir/logs.stderr"
test ! -s "$tmpdir/logs.stderr"
jq -e '
  (.data.checks[0].annotations | length) == 3 and
  .data.checks[0].log.truncated == true and
  .data.checks[0].log.omittedLines == 205 and
  .data.checks[0].log.omittedBytes > 0 and
  (.data.checks[0].log.text | utf8bytelength) <= 65536 and
  (.data.checks[0].log.text | split("\n") | length) <= 200 and
  .data.checks[1].log == null and
  (.data.checks[2] | has("log") | not)
' "$tmpdir/logs.json" >/dev/null

GH_DIAGNOSTICS_REPOSITORY=Owner/Repo run_diagnostics normal \
  --include-failed-logs --quiet --compact >"$tmpdir/mixed-case.json"
jq -e '
  (.data.checks[0].annotations | length) == 3 and
  .data.checks[0].log != null
' "$tmpdir/mixed-case.json" >/dev/null

GH_DIAGNOSTICS_CALLS="$tmpdir/mismatch-calls" run_diagnostics job-mismatch \
  --include-failed-logs --quiet --compact >"$tmpdir/job-mismatch.json"
jq -e '.data.checks[0].log == null' "$tmpdir/job-mismatch.json" >/dev/null
grep -F -- 'actions/jobs/20' "$tmpdir/mismatch-calls" >/dev/null
if grep -F -- 'actions/jobs/20/logs' "$tmpdir/mismatch-calls" >/dev/null; then
  exit 1
fi

run_diagnostics no-failures --failed-diagnostics --compact \
  >"$tmpdir/no-failures.json" 2>"$tmpdir/no-failures.stderr"
test ! -s "$tmpdir/no-failures.stderr"
jq -e '(.data.checks | length) == 1 and (.data.checks[0] | has("annotations") | not)' \
  "$tmpdir/no-failures.json" >/dev/null

for mode in annotation-failure metadata-failure log-failure; do
  set +e
  run_diagnostics "$mode" --include-failed-logs --compact \
    >"$tmpdir/$mode.stdout" 2>"$tmpdir/$mode.stderr"
  status=$?
  set -e
  test "$status" -eq 1
  test ! -s "$tmpdir/$mode.stdout"
  tail -n 1 "$tmpdir/$mode.stderr" | jq -e '.schemaVersion == 1 and .error.kind == "githubCli"' >/dev/null
  test "$(grep -c '"schemaVersion":1,"error"' "$tmpdir/$mode.stderr")" -eq 1
done

set +e
GH_DIAGNOSTICS_PID_FILE="$tmpdir/pid" run_diagnostics timeout --include-failed-logs --timeout 1 --compact \
  >"$tmpdir/timeout.stdout" 2>"$tmpdir/timeout.stderr"
status=$?
set -e
test "$status" -eq 1
test ! -s "$tmpdir/timeout.stdout"
tail -n 1 "$tmpdir/timeout.stderr" | jq -e '
  .error.kind == "timeout" and
  .error.message == "failed check diagnostics timed out after 1 seconds" and
  .error.retryable == true and
  .error.retryAfterSeconds == null
' >/dev/null
test "$(grep -c '"schemaVersion":1,"error"' "$tmpdir/timeout.stderr")" -eq 1
if kill -0 "$(cat "$tmpdir/pid")" 2>/dev/null; then
  exit 1
fi

run_diagnostics progress --include-failed-logs --timeout 30 --compact \
  >"$tmpdir/progress.json" 2>"$tmpdir/progress.stderr"
grep -E '^gh-read: diagnostics 0/2 complete; (15|16)s elapsed$' "$tmpdir/progress.stderr" >/dev/null
jq -e '.data.checks | length == 3' "$tmpdir/progress.json" >/dev/null

for args in '--timeout 0' '--timeout nope' '--timeout' '--time 1' '--failed' '--include-failed' '--qui'; do
  set +e
  # shellcheck disable=SC2086
  run_diagnostics normal $args >"$tmpdir/argument.stdout" 2>"$tmpdir/argument.stderr"
  status=$?
  set -e
  test "$status" -eq 2
  test ! -s "$tmpdir/argument.stdout"
done
