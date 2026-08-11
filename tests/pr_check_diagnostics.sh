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
  .data.checks[1].workflow == null and
  .data.checks[1].startedAt == null and
  .data.checks[1].completedAt == null and
  (.data.checks[2] | has("annotations") | not) and
  ([.data.checks[] | has("log")] | all(. == false))
' "$tmpdir/diagnostics.json" >/dev/null
grep -F -- 'check-runs/100/annotations?per_page=100' "$tmpdir/calls" >/dev/null

GH_DIAGNOSTICS_CALLS="$tmpdir/collision-calls" run_diagnostics status-collision \
  --failed-diagnostics --quiet --compact >"$tmpdir/status-collision.json"
jq -e '
  (.data.checks | length) == 2 and
  ([.data.checks[] | select(.workflow == null)][0].annotations == []) and
  ([.data.checks[] | select(.workflow == "CI")][0].annotations[].path == "collision.rs")
' "$tmpdir/status-collision.json" >/dev/null
test "$(grep -c 'check-runs/102/annotations' "$tmpdir/collision-calls")" -eq 1

run_diagnostics status-duplicate --failed-diagnostics --quiet --compact \
  >"$tmpdir/status-duplicate.json"
jq -e '
  (.data.checks | length) == 2 and
  (.data.checks | all(.name == "duplicate-status" and .annotations == []))
' "$tmpdir/status-duplicate.json" >/dev/null

GH_DIAGNOSTICS_CALLS="$tmpdir/check-run-collision-calls" run_diagnostics check-run-collision \
  --failed-diagnostics --quiet --compact >"$tmpdir/check-run-collision.json"
jq -e '
  [.data.checks[].name] == ["duplicate", "duplicate"] and
  [.data.checks[].annotations[0].path] == ["first.rs", "second.rs"]
' "$tmpdir/check-run-collision.json" >/dev/null
test "$(grep -c 'check-runs/102/annotations' "$tmpdir/check-run-collision-calls")" -eq 1
test "$(grep -c 'check-runs/103/annotations' "$tmpdir/check-run-collision-calls")" -eq 1

run_diagnostics pending-metadata --failed-diagnostics --quiet --compact \
  >"$tmpdir/pending-metadata.json"
jq -e '
  .data.checks == [{"name":"pending","state":"IN_PROGRESS","bucket":"pending","link":null,"workflow":null,"startedAt":"2026-08-11T11:00:00Z","completedAt":null}]
' "$tmpdir/pending-metadata.json" >/dev/null

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

run_diagnostics large-log --include-failed-logs --quiet --compact >"$tmpdir/large-log.json"
jq -e '
  .data.checks[0].log.omittedLines == 10000 and
  .data.checks[0].log.omittedBytes == 2044475 and
  (.data.checks[0].log.text | utf8bytelength) == 65536 and
  (.data.checks[0].log.text | endswith("final-tail\n"))
' "$tmpdir/large-log.json" >/dev/null

run_diagnostics utf8-boundary --include-failed-logs --quiet --compact >"$tmpdir/utf8-boundary.json"
jq -e '
  .data.checks[0].log.omittedLines == 0 and
  .data.checks[0].log.omittedBytes == 65538 and
  (.data.checks[0].log.text | utf8bytelength) == 65534 and
  (.data.checks[0].log.text | endswith("tail\n"))
' "$tmpdir/utf8-boundary.json" >/dev/null

GH_DIAGNOSTICS_REPOSITORY=Owner/Repo run_diagnostics normal \
  --include-failed-logs --quiet --compact >"$tmpdir/mixed-case.json"
jq -e '
  (.data.checks[0].annotations | length) == 3 and
  .data.checks[0].log != null
' "$tmpdir/mixed-case.json" >/dev/null

for mode in job-mismatch job-head-mismatch job-link-repository-mismatch job-metadata-repository-mismatch; do
  GH_DIAGNOSTICS_CALLS="$tmpdir/$mode-calls" run_diagnostics "$mode" \
    --include-failed-logs --quiet --compact >"$tmpdir/$mode.json"
  jq -e '.data.checks[0].log == null' "$tmpdir/$mode.json" >/dev/null
  if grep -F -- 'actions/jobs/20/logs' "$tmpdir/$mode-calls" >/dev/null; then
    exit 1
  fi
done

set +e
run_diagnostics job-id-mismatch --include-failed-logs --quiet --compact \
  >"$tmpdir/job-id-mismatch.stdout" 2>"$tmpdir/job-id-mismatch.stderr"
status=$?
set -e
test "$status" -eq 1
test ! -s "$tmpdir/job-id-mismatch.stdout"
tail -n 1 "$tmpdir/job-id-mismatch.stderr" | jq -e '.schemaVersion == 1 and .error.kind == "invalidResponse"' >/dev/null

run_diagnostics no-failures --failed-diagnostics --compact \
  >"$tmpdir/no-failures.json" 2>"$tmpdir/no-failures.stderr"
test ! -s "$tmpdir/no-failures.stderr"
jq -e '(.data.checks | length) == 1 and (.data.checks[0] | has("annotations") | not)' \
  "$tmpdir/no-failures.json" >/dev/null

for mode in pagination-repeat pagination-cycle pagination-missing pagination-empty pagination-wrong-type head-oid-changed; do
  case "$mode" in
    pagination-repeat) expected_calls=2 ;;
    pagination-cycle) expected_calls=3 ;;
    head-oid-changed) expected_calls=2 ;;
    *) expected_calls=1 ;;
  esac
  calls_file="$tmpdir/$mode-calls"
  set +e
  GH_DIAGNOSTICS_CALLS="$calls_file" run_diagnostics "$mode" \
    --failed-diagnostics --quiet --compact \
    >"$tmpdir/$mode.stdout" 2>"$tmpdir/$mode.stderr"
  status=$?
  set -e
  test "$status" -eq 1
  test ! -s "$tmpdir/$mode.stdout"
  test "$(wc -l <"$tmpdir/$mode.stderr" | tr -d ' ')" -eq 1
  jq -e '.schemaVersion == 1 and .error.kind == "invalidResponse"' \
    "$tmpdir/$mode.stderr" >/dev/null
  test "$(grep -c 'api graphql' "$calls_file")" -eq "$expected_calls"
done

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
run_diagnostics annotation-malformed --failed-diagnostics --quiet --compact \
  >"$tmpdir/annotation-malformed.stdout" 2>"$tmpdir/annotation-malformed.stderr"
status=$?
set -e
test "$status" -eq 1
test ! -s "$tmpdir/annotation-malformed.stdout"
tail -n 1 "$tmpdir/annotation-malformed.stderr" | jq -e '.schemaVersion == 1 and .error.kind == "invalidResponse"' >/dev/null

for mode in graphql-missing-completed graphql-wrong-completed; do
  set +e
  run_diagnostics "$mode" --failed-diagnostics --quiet --compact \
    >"$tmpdir/$mode.stdout" 2>"$tmpdir/$mode.stderr"
  status=$?
  set -e
  test "$status" -eq 1
  test ! -s "$tmpdir/$mode.stdout"
  tail -n 1 "$tmpdir/$mode.stderr" | jq -e '.schemaVersion == 1 and .error.kind == "invalidResponse"' >/dev/null
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

set +e
run_diagnostics normal --failed-diagnostics --timeout 18446744073709551615 --compact \
  >"$tmpdir/unrepresentable-timeout.stdout" 2>"$tmpdir/unrepresentable-timeout.stderr"
status=$?
set -e
test "$status" -eq 2
test ! -s "$tmpdir/unrepresentable-timeout.stdout"
grep -F 'argument --timeout: value cannot be represented as a diagnostic deadline' \
  "$tmpdir/unrepresentable-timeout.stderr" >/dev/null

run_diagnostics progress --include-failed-logs --timeout 30 --compact \
  >"$tmpdir/progress.json" 2>"$tmpdir/progress.stderr"
grep -E '^gh-read: diagnostics 0/2 complete; (15|16)s elapsed$' "$tmpdir/progress.stderr" >/dev/null
jq -e '.data.checks | length == 3' "$tmpdir/progress.json" >/dev/null

run_diagnostics normal --failed-diagnostics --compact 2>&- >"$tmpdir/closed-progress.json"
jq -e '.data.checks | length == 3' "$tmpdir/closed-progress.json" >/dev/null

for args in '--timeout 0' '--timeout nope' '--timeout' '--time 1' '--failed' '--include-failed' '--qui'; do
  set +e
  # shellcheck disable=SC2086
  run_diagnostics normal $args >"$tmpdir/argument.stdout" 2>"$tmpdir/argument.stderr"
  status=$?
  set -e
  test "$status" -eq 2
  test ! -s "$tmpdir/argument.stdout"
done
