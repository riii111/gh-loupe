#!/usr/bin/env bash
set -Eeuo pipefail
status=0

trap 'status=$?; printf "%s:%s: assertion failed (exit %s): %s\n" "${BASH_SOURCE[0]}" "$LINENO" "$status" "$BASH_COMMAND" >&2' ERR

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/gh-loupe-diagnostics.XXXXXX")"
trap 'rm -rf "$tmpdir"' EXIT
mkdir -p "$tmpdir/bin"
cp "$repo_root/tests/fixtures/gh-diagnostics" "$tmpdir/bin/gh"
chmod +x "$tmpdir/bin/gh"

run_diagnostics() {
  local repository="${GH_DIAGNOSTICS_REPOSITORY:-owner/repo}"
  env PATH="$tmpdir/bin:$PATH" \
    GH_DIAGNOSTICS_MODE="${1:-normal}" \
    GH_DIAGNOSTICS_FAILURES="${GH_DIAGNOSTICS_FAILURES:-}" \
    GH_DIAGNOSTICS_DELAY_SECONDS="${GH_DIAGNOSTICS_DELAY_SECONDS:-}" \
    GH_DIAGNOSTICS_ACTIVE_FILE="${GH_DIAGNOSTICS_ACTIVE_FILE:-}" \
    GH_DIAGNOSTICS_MAX_FILE="${GH_DIAGNOSTICS_MAX_FILE:-}" \
    GH_DIAGNOSTICS_STARTED_FILE="${GH_DIAGNOSTICS_STARTED_FILE:-}" \
    GH_DIAGNOSTICS_ROUNDS_FILE="${GH_DIAGNOSTICS_ROUNDS_FILE:-}" \
    GH_DIAGNOSTICS_MAX_ROUND_FILE="${GH_DIAGNOSTICS_MAX_ROUND_FILE:-}" \
    GH_DIAGNOSTICS_BATCH_DIR="${GH_DIAGNOSTICS_BATCH_DIR:-}" \
    GH_DIAGNOSTICS_STAGGERED_LOW_FILE="${GH_DIAGNOSTICS_STAGGERED_LOW_FILE:-}" \
    GH_DIAGNOSTICS_STAGGERED_FAILURE_FILE="${GH_DIAGNOSTICS_STAGGERED_FAILURE_FILE:-}" \
    "$GH_LOUPE_BIN" pr checks 42 --repo "$repository" "${@:2}"
}

GH_DIAGNOSTICS_CALLS="$tmpdir/calls" run_diagnostics normal --failed-diagnostics --compact \
  >"$tmpdir/diagnostics.json" 2>"$tmpdir/diagnostics.stderr"
grep -Fx 'gh-loupe: collecting diagnostics for 2 failed checks' "$tmpdir/diagnostics.stderr" >/dev/null
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

for failures in 0 1 2 10; do
  calls_file="$tmpdir/parallel-$failures-calls"
  active_file="$tmpdir/parallel-$failures-active"
  max_file="$tmpdir/parallel-$failures-max"
  rounds_file="$tmpdir/parallel-$failures-rounds"
  max_round_file="$tmpdir/parallel-$failures-max-round"
  batch_dir="$tmpdir/parallel-$failures-batches"
  mkdir -p "$batch_dir"
  mode=parallel
  if [ "$failures" -eq 10 ]; then
    mode=parallel-timing
  fi
  GH_DIAGNOSTICS_FAILURES="$failures" \
    GH_DIAGNOSTICS_DELAY_SECONDS=0.04 \
    GH_DIAGNOSTICS_ACTIVE_FILE="$active_file" \
    GH_DIAGNOSTICS_MAX_FILE="$max_file" \
    GH_DIAGNOSTICS_ROUNDS_FILE="$rounds_file" \
    GH_DIAGNOSTICS_MAX_ROUND_FILE="$max_round_file" \
    GH_DIAGNOSTICS_BATCH_DIR="$batch_dir" \
    GH_DIAGNOSTICS_CALLS="$calls_file" \
    run_diagnostics "$mode" --failed-diagnostics --quiet --compact \
    >"$tmpdir/parallel-$failures.json"
  test "$(wc -l <"$calls_file" | tr -d ' ')" -eq "$((failures + 1))"
  if [ "$failures" -eq 0 ]; then
    test ! -e "$max_file"
    jq -e '.data.checks == []' "$tmpdir/parallel-$failures.json" >/dev/null
  else
    expected_workers="$failures"
    if [ "$expected_workers" -gt 4 ]; then
      expected_workers=4
    fi
    test "$(cat "$max_file")" -eq "$expected_workers"
    jq -e --argjson failures "$failures" \
      '.data.checks | length == $failures and all(.annotations == [])' \
      "$tmpdir/parallel-$failures.json" >/dev/null
  fi
  if [ "$failures" -eq 10 ]; then
    expected_rounds=$(( (failures + 3) / 4 ))
    test "$(cat "$max_round_file")" -eq "$expected_rounds"
    test "$(cat "$batch_dir/1")" -eq 4
    test "$(cat "$batch_dir/2")" -eq 4
    test "$(cat "$batch_dir/3")" -eq 2
  fi
done

if GH_DIAGNOSTICS_FAILURES=2 \
  GH_DIAGNOSTICS_STAGGERED_LOW_FILE="$tmpdir/staggered-low" \
  GH_DIAGNOSTICS_STAGGERED_FAILURE_FILE="$tmpdir/staggered-failure" \
  GH_DIAGNOSTICS_ACTIVE_FILE="$tmpdir/staggered-active" \
  GH_DIAGNOSTICS_MAX_FILE="$tmpdir/staggered-max" \
  GH_DIAGNOSTICS_STARTED_FILE="$tmpdir/staggered-started" \
  GH_DIAGNOSTICS_CALLS="$tmpdir/staggered-calls" \
  run_diagnostics parallel-staggered-error --failed-diagnostics --quiet --compact \
  >"$tmpdir/staggered.stdout" 2>"$tmpdir/staggered.stderr"; then
  status=0
else
  status=$?
fi
test "$status" -eq 1
test ! -s "$tmpdir/staggered.stdout"
jq -e '.error.kind == "githubCli" and .error.message == "simulated staggered failure 2"' \
  "$tmpdir/staggered.stderr" >/dev/null
test "$(cat "$tmpdir/staggered-active")" -eq 0
test "$(cat "$tmpdir/staggered-max")" -eq 2
test "$(wc -l <"$tmpdir/staggered-calls" | tr -d ' ')" -eq 3

GH_DIAGNOSTICS_DELAY_SECONDS=16 \
  run_diagnostics parallel-status-progress --failed-diagnostics --timeout 30 --compact \
  >"$tmpdir/status-progress.json" 2>"$tmpdir/status-progress.stderr"
grep -Fx 'gh-loupe: collecting diagnostics for 3 failed checks' \
  "$tmpdir/status-progress.stderr" >/dev/null
grep -E '^gh-loupe: diagnostics 1/3 complete; (15|16)s elapsed$' \
  "$tmpdir/status-progress.stderr" >/dev/null
jq -e '
  [.data.checks[].name] == ["aaa-status", "bbb-failure", "ccc-failure"] and
  .data.checks[0].annotations == [] and
  .data.checks[1].annotations == [] and
  .data.checks[2].annotations == []
' "$tmpdir/status-progress.json" >/dev/null

if GH_DIAGNOSTICS_FAILURES=4 \
  GH_DIAGNOSTICS_ACTIVE_FILE="$tmpdir/parallel-error-active" \
  GH_DIAGNOSTICS_MAX_FILE="$tmpdir/parallel-error-max" \
  GH_DIAGNOSTICS_STARTED_FILE="$tmpdir/parallel-error-started" \
  GH_DIAGNOSTICS_CALLS="$tmpdir/parallel-error-calls" \
  run_diagnostics parallel-error --failed-diagnostics --quiet --compact \
  >"$tmpdir/parallel-error.stdout" 2>"$tmpdir/parallel-error.stderr"; then
  status=0
else
  status=$?
fi
test "$status" -eq 1
test ! -s "$tmpdir/parallel-error.stdout"
jq -e '
  .error.kind == "githubCli" and
  .error.message == "simulated parallel failure 1"
' "$tmpdir/parallel-error.stderr" >/dev/null
test "$(cat "$tmpdir/parallel-error-active")" -eq 0
test "$(cat "$tmpdir/parallel-error-max")" -eq 4

if GH_DIAGNOSTICS_FAILURES=4 \
  GH_DIAGNOSTICS_ACTIVE_FILE="$tmpdir/parallel-rate-limit-active" \
  GH_DIAGNOSTICS_MAX_FILE="$tmpdir/parallel-rate-limit-max" \
  GH_DIAGNOSTICS_STARTED_FILE="$tmpdir/parallel-rate-limit-started" \
  GH_DIAGNOSTICS_CALLS="$tmpdir/parallel-rate-limit-calls" \
  run_diagnostics parallel-rate-limit --failed-diagnostics --quiet --compact \
  >"$tmpdir/parallel-rate-limit.stdout" 2>"$tmpdir/parallel-rate-limit.stderr"; then
  status=0
else
  status=$?
fi
test "$status" -eq 1
test ! -s "$tmpdir/parallel-rate-limit.stdout"
jq -e '
  .error.kind == "rateLimited" and
  .error.retryAfterSeconds == 45 and
  .error.retryable == true
' "$tmpdir/parallel-rate-limit.stderr" >/dev/null
test "$(cat "$tmpdir/parallel-rate-limit-active")" -eq 0
test "$(cat "$tmpdir/parallel-rate-limit-max")" -eq 4

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

run_diagnostics required-filter --required --failed-diagnostics --quiet --compact \
  >"$tmpdir/required-filter.json"
jq -e '
  (.data.checks | length) == 1 and
  .data.checks[0].name == "required-failure" and
  (.data.checks[0].annotations | length) == 3
' "$tmpdir/required-filter.json" >/dev/null

for mode in graphql-error missing-pr; do
  case "$mode" in
    graphql-error) expected_kind=githubCli ;;
    missing-pr) expected_kind=notFound ;;
  esac
  if run_diagnostics "$mode" --failed-diagnostics --quiet --compact \
    >"$tmpdir/$mode.stdout" 2>"$tmpdir/$mode.stderr"; then
    status=0
  else
    status=$?
  fi
  test "$status" -eq 1
  test ! -s "$tmpdir/$mode.stdout"
  test "$(wc -l <"$tmpdir/$mode.stderr" | tr -d ' ')" -eq 1
  jq -e --arg kind "$expected_kind" \
    '.schemaVersion == 1 and .error.kind == $kind and .error.retryable == false' \
    "$tmpdir/$mode.stderr" >/dev/null
done

if run_diagnostics non-utf8 --include-failed-logs --quiet --compact \
  >"$tmpdir/non-utf8.stdout" 2>"$tmpdir/non-utf8.stderr"; then
  status=0
else
  status=$?
fi
test "$status" -eq 1
test ! -s "$tmpdir/non-utf8.stdout"
test "$(wc -l <"$tmpdir/non-utf8.stderr" | tr -d ' ')" -eq 1
jq -e '
  .schemaVersion == 1 and
  .error.kind == "invalidResponse" and
  .error.message == "GitHub returned a non-UTF-8 job log"
' "$tmpdir/non-utf8.stderr" >/dev/null

for mode in job-mismatch job-head-mismatch job-link-repository-mismatch job-metadata-repository-mismatch; do
  GH_DIAGNOSTICS_CALLS="$tmpdir/$mode-calls" run_diagnostics "$mode" \
    --include-failed-logs --quiet --compact >"$tmpdir/$mode.json"
  jq -e '.data.checks[0].log == null' "$tmpdir/$mode.json" >/dev/null
  if grep -F -- 'actions/jobs/20/logs' "$tmpdir/$mode-calls" >/dev/null; then
    exit 1
  fi
done

if run_diagnostics job-id-mismatch --include-failed-logs --quiet --compact \
  >"$tmpdir/job-id-mismatch.stdout" 2>"$tmpdir/job-id-mismatch.stderr"; then
  status=0
else
  status=$?
fi
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
  if GH_DIAGNOSTICS_CALLS="$calls_file" run_diagnostics "$mode" \
    --failed-diagnostics --quiet --compact \
    >"$tmpdir/$mode.stdout" 2>"$tmpdir/$mode.stderr"; then
    status=0
  else
    status=$?
  fi
  test "$status" -eq 1
  test ! -s "$tmpdir/$mode.stdout"
  test "$(wc -l <"$tmpdir/$mode.stderr" | tr -d ' ')" -eq 1
  jq -e '.schemaVersion == 1 and .error.kind == "invalidResponse"' \
    "$tmpdir/$mode.stderr" >/dev/null
  test "$(grep -c 'api graphql' "$calls_file")" -eq "$expected_calls"
done

for mode in annotation-failure metadata-failure log-failure; do
  if run_diagnostics "$mode" --include-failed-logs --compact \
    >"$tmpdir/$mode.stdout" 2>"$tmpdir/$mode.stderr"; then
    status=0
  else
    status=$?
  fi
  test "$status" -eq 1
  test ! -s "$tmpdir/$mode.stdout"
  tail -n 1 "$tmpdir/$mode.stderr" | jq -e '.schemaVersion == 1 and .error.kind == "githubCli"' >/dev/null
  test "$(grep -c '"schemaVersion":1,"error"' "$tmpdir/$mode.stderr")" -eq 1
done

if run_diagnostics annotation-malformed --failed-diagnostics --quiet --compact \
  >"$tmpdir/annotation-malformed.stdout" 2>"$tmpdir/annotation-malformed.stderr"; then
  status=0
else
  status=$?
fi
test "$status" -eq 1
test ! -s "$tmpdir/annotation-malformed.stdout"
tail -n 1 "$tmpdir/annotation-malformed.stderr" | jq -e '.schemaVersion == 1 and .error.kind == "invalidResponse"' >/dev/null

for mode in graphql-missing-completed graphql-wrong-completed; do
  if run_diagnostics "$mode" --failed-diagnostics --quiet --compact \
    >"$tmpdir/$mode.stdout" 2>"$tmpdir/$mode.stderr"; then
    status=0
  else
    status=$?
  fi
  test "$status" -eq 1
  test ! -s "$tmpdir/$mode.stdout"
  tail -n 1 "$tmpdir/$mode.stderr" | jq -e '.schemaVersion == 1 and .error.kind == "invalidResponse"' >/dev/null
done

if GH_DIAGNOSTICS_PID_FILE="$tmpdir/pid" run_diagnostics timeout --include-failed-logs --timeout 1 --compact \
  >"$tmpdir/timeout.stdout" 2>"$tmpdir/timeout.stderr"; then
  status=0
else
  status=$?
fi
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

if run_diagnostics normal --failed-diagnostics --timeout 18446744073709551615 --compact \
  >"$tmpdir/unrepresentable-timeout.stdout" 2>"$tmpdir/unrepresentable-timeout.stderr"; then
  status=0
else
  status=$?
fi
test "$status" -eq 2
test ! -s "$tmpdir/unrepresentable-timeout.stdout"
grep -F 'argument --timeout: value cannot be represented as a diagnostic deadline' \
  "$tmpdir/unrepresentable-timeout.stderr" >/dev/null

run_diagnostics progress --include-failed-logs --timeout 30 --compact \
  >"$tmpdir/progress.json" 2>"$tmpdir/progress.stderr"
grep -E '^gh-loupe: diagnostics 0/2 complete; (15|16)s elapsed$' "$tmpdir/progress.stderr" >/dev/null
jq -e '.data.checks | length == 3' "$tmpdir/progress.json" >/dev/null

run_diagnostics normal --failed-diagnostics --compact 2>&- >"$tmpdir/closed-progress.json"
jq -e '.data.checks | length == 3' "$tmpdir/closed-progress.json" >/dev/null

for args in '--timeout 0' '--timeout nope' '--timeout' '--time 1' '--failed' '--include-failed' '--qui'; do
  # shellcheck disable=SC2086
  if run_diagnostics normal $args >"$tmpdir/argument.stdout" 2>"$tmpdir/argument.stderr"; then
    status=0
  else
    status=$?
  fi
  test "$status" -eq 2
  test ! -s "$tmpdir/argument.stdout"
done
