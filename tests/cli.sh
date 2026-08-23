#!/usr/bin/env bash

set -Eeuo pipefail

trap 'status=$?; printf "%s:%s: assertion failed (exit %s): %s\n" "${BASH_SOURCE[0]}" "$LINENO" "$status" "$BASH_COMMAND" >&2' ERR

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
source "$repo_root/tests/assertions.sh"
export GH_FIXTURE_DATA="$repo_root/tests/fixtures/data/gh"
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

assert_argument_error_without_github() {
  local name="$1"
  shift

  local calls_file="$tmpdir/$name.calls"
  local status
  if env PATH="$tmpdir/bin:$PATH" GH_TEST_CALLS_FILE="$calls_file" "$tmpdir/rust/gh-loupe" "$@" \
    >"$tmpdir/$name.stdout" 2>"$tmpdir/$name.stderr"; then
    status=0
  else
    status=$?
  fi

  test "$status" -eq 2
  test ! -s "$tmpdir/$name.stdout"
  test -s "$tmpdir/$name.stderr"
  test ! -e "$calls_file"
}

assert_target_guidance() {
  local name="$1"
  local resource="$2"
  local target="$3"
  local calls_file="$tmpdir/$name.calls"
  local status
  if env PATH="$tmpdir/bin:$PATH" GH_TEST_CALLS_FILE="$calls_file" \
    "$tmpdir/rust/gh-loupe" "$resource" "$target" \
    >"$tmpdir/$name.stdout" 2>"$tmpdir/$name.stderr"; then
    status=0
  else
    status=$?
  fi

  test "$status" -eq 2
  test ! -s "$tmpdir/$name.stdout"
  test ! -e "$calls_file"

  local expected
  case "$resource" in
    pr)
      expected="$(printf 'usage: gh-loupe pr [-h] {overview,comments,reviews,review-threads,review-thread,checks,for-commit} ...\ngh-loupe pr: error: a subcommand is required\n\nTry:\n  gh-loupe pr overview %s\n  gh-loupe pr comments %s\n  gh-loupe pr reviews %s\n  gh-loupe pr review-threads %s\n  gh-loupe pr checks %s' \
        "$target" "$target" "$target" "$target" "$target")"
      ;;
    issue)
      expected="$(printf 'usage: gh-loupe issue [-h] {overview,comments,relations} ...\ngh-loupe issue: error: a subcommand is required\n\nTry:\n  gh-loupe issue overview %s\n  gh-loupe issue comments %s\n  gh-loupe issue relations %s' \
        "$target" "$target" "$target")"
      ;;
    *)
      return 1
      ;;
  esac
  test "$(cat "$tmpdir/$name.stderr")" = "$expected"
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
  assert_json '
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
  assert_json '
    .schemaVersion == 1 and
    (.observedAt | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
    (.data | keys == ["reviewThreads"]) and
    (.data.reviewThreads | type == "array")
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
  # shellcheck disable=SC2016
  assert_json --arg kind "$expected_kind" '
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
  # shellcheck disable=SC2016
  assert_json --arg kind "$expected_kind" --argjson retryable "$expected_retryable" \
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
  # shellcheck disable=SC2016
  assert_json --arg kind "$expected_kind" --argjson retryable "$expected_retryable" \
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
  assert_json '
    .schemaVersion == 1 and
    (.observedAt | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
    (.data | keys == ["comments"])
  ' "$tmpdir/$name.comments.stdout" >/dev/null
}

run_cli root-help -- --help
test ! -s "$tmpdir/root-help.stderr"
grep -F 'usage: gh-loupe [-h] [--version] {pr,issue,search} ...' "$tmpdir/root-help.stdout" >/dev/null

run_cli root-version -- --version
test ! -s "$tmpdir/root-version.stderr"
test "$(cat "$tmpdir/root-version.stdout")" = "gh-loupe $GH_LOUPE_PACKAGE_VERSION"

run_cli pr-help -- pr --help
test ! -s "$tmpdir/pr-help.stderr"
grep -F 'usage: gh-loupe pr [-h] {overview,comments,reviews,review-threads,review-thread,checks,for-commit} ...' "$tmpdir/pr-help.stdout" >/dev/null
for subcommand in overview comments reviews review-threads review-thread checks for-commit; do
  grep -E "^    $subcommand  +" "$tmpdir/pr-help.stdout" >/dev/null
done

run_cli issue-help -- issue --help
test ! -s "$tmpdir/issue-help.stderr"
grep -Fx 'usage: gh-loupe issue [-h] {overview,comments,relations} ...' \
  "$tmpdir/issue-help.stdout" >/dev/null
for subcommand in overview comments relations; do
  grep -E "^    $subcommand  +" "$tmpdir/issue-help.stdout" >/dev/null
done

run_cli issue-relations-help -- issue relations --help
test ! -s "$tmpdir/issue-relations-help.stderr"
grep -Fx 'usage: gh-loupe issue relations [-h] [--repo REPO] [--limit N] target' \
  "$tmpdir/issue-relations-help.stdout" >/dev/null
grep -Fx '  --limit N    limit each relation list to 1 through 100 items (default: 20)' \
  "$tmpdir/issue-relations-help.stdout" >/dev/null
if grep -E '^    (threads|thread|full|legacy)  +' "$tmpdir/pr-help.stdout" >/dev/null; then
  exit 1
fi

run_cli search-help -- search --help
test ! -s "$tmpdir/search-help.stderr"
grep -F 'usage: gh-loupe search [-h] {issues,prs} ...' "$tmpdir/search-help.stdout" >/dev/null
for subcommand in issues prs; do
  grep -E "^    $subcommand  +" "$tmpdir/search-help.stdout" >/dev/null
done
run_cli search-issues-help -- search issues --help
grep -F 'usage: gh-loupe search issues [-h] [--repo REPO] [--limit N] query' \
  "$tmpdir/search-issues-help.stdout" >/dev/null
grep -F -- '--limit N' "$tmpdir/search-issues-help.stdout" >/dev/null
run_cli for-commit-help -- pr for-commit --help
grep -F 'usage: gh-loupe pr for-commit [-h] [--repo REPO] [--limit N] sha' \
  "$tmpdir/for-commit-help.stdout" >/dev/null

assert_argument_error_without_github search-missing-query search issues
assert_argument_error_without_github search-empty-query search issues '' --repo riii111/dotfiles
assert_argument_error_without_github search-invalid-repo search issues keyword --repo https://github.com/riii111/dotfiles
assert_argument_error_without_github search-limit-zero search issues keyword --limit 0 --repo riii111/dotfiles
assert_argument_error_without_github search-limit-high search issues keyword --limit 101 --repo riii111/dotfiles
assert_argument_error_without_github search-limit-equals-zero search issues keyword --limit=0 --repo riii111/dotfiles
assert_argument_error_without_github search-abbreviated-limit search issues keyword --lim 10 --repo riii111/dotfiles

search_calls_file="$tmpdir/search.calls"
run_cli search-issues-default "GH_TEST_CALLS_FILE=$search_calls_file" -- \
  search issues keyword --repo riii111/dotfiles
assert_json '
  .schemaVersion == 1 and
  (.data | keys == ["incompleteResults","issues","repository","totalCount","truncated"]) and
  .data.repository == "riii111/dotfiles" and
  [.data.issues[].number] == [1,2] and
  [.data.issues[].state] == ["OPEN","CLOSED"] and
  (.data.issues | all(keys == ["number","state","stateReason","title","updatedAt","url"])) and
  ([.. | objects | keys[]] | any(. == "body" or . == "labels" or . == "comments" or . == "pull_request") | not) and
  .data.totalCount == 2 and .data.truncated == false and .data.incompleteResults == false
' "$tmpdir/search-issues-default.stdout" >/dev/null
grep -Fx 'api --method GET search/issues?q=keyword%20repo%3Ariii111%2Fdotfiles%20is%3Aissue&per_page=21' \
  "$search_calls_file" >/dev/null

run_cli search-prs-default -- search prs keyword --repo riii111/dotfiles
assert_json '
  .data.repository == "riii111/dotfiles" and
  [.data.pullRequests[].number] == [10,11] and
  [.data.pullRequests[].isDraft] == [false,true] and
  (.data.pullRequests | all(keys == ["isDraft","number","state","title","updatedAt","url"])) and
  .data.totalCount == 2 and .data.truncated == false and .data.incompleteResults == false
' "$tmpdir/search-prs-default.stdout" >/dev/null

run_cli search-inferred-repo "GH_TEST_CALLS_FILE=$tmpdir/search-inferred.calls" -- \
  search issues keyword
test "$(sed -n '1p' "$tmpdir/search-inferred.calls")" = 'repo view --json nameWithOwner'
grep -F 'repo%3Ariii111%2Fdotfiles%20is%3Aissue' "$tmpdir/search-inferred.calls" >/dev/null

run_cli search-limit-one -- search issues keyword --repo riii111/dotfiles --limit 1
assert_json '(.data.issues | length) == 1 and .data.truncated == true' \
  "$tmpdir/search-limit-one.stdout" >/dev/null
run_cli search-limit-boundaries -- search issues keyword --repo riii111/dotfiles --limit 20
run_cli search-limit-hundred -- search issues keyword --repo riii111/dotfiles --limit 100

run_cli search-truncated GH_TEST_SEARCH_RESPONSE=truncated -- \
  search issues keyword --repo riii111/dotfiles --limit 2
assert_json '.data.totalCount == 3 and .data.truncated == true and (.data.issues | length == 2)' \
  "$tmpdir/search-truncated.stdout" >/dev/null
run_cli search-incomplete GH_TEST_SEARCH_RESPONSE=incomplete -- \
  search issues keyword --repo riii111/dotfiles
assert_json '.data.truncated == false and .data.incompleteResults == true and .data.issues == []' \
  "$tmpdir/search-incomplete.stdout" >/dev/null
assert_argument_error_without_github search-scope-and-type \
  search issues 'repo:other/repo is:pr type:issue type:pr keyword' --repo riii111/dotfiles

for response in wrapper missing-total wrong-total wrong-incomplete invalid-item missing-field wrong-field issue-marker missing-nullable wrong-nullable over-total; do
  assert_runtime_failure "search-$response" invalidResponse runtime \
    GH_TEST_SEARCH_RESPONSE="$response" -- search issues keyword --repo riii111/dotfiles
done
assert_runtime_failure search-pr-marker invalidResponse runtime \
  GH_TEST_SEARCH_RESPONSE=pr-marker -- search prs keyword --repo riii111/dotfiles
assert_runtime_failure search-missing-draft invalidResponse runtime \
  GH_TEST_SEARCH_RESPONSE=missing-draft -- search prs keyword --repo riii111/dotfiles
assert_runtime_failure search-wrong-draft invalidResponse runtime \
  GH_TEST_SEARCH_RESPONSE=wrong-draft -- search prs keyword --repo riii111/dotfiles
assert_runtime_failure search-invalid-json invalidResponse runtime \
  GH_TEST_SEARCH_RESPONSE=invalid-json -- search issues keyword --repo riii111/dotfiles
assert_runtime_failure search-failure githubCli runtime \
  GH_TEST_SEARCH_RESPONSE=failure -- search issues keyword --repo riii111/dotfiles

assert_argument_error_without_github for-commit-missing-sha pr for-commit
assert_argument_error_without_github for-commit-short-sha pr for-commit 012345 --repo riii111/dotfiles
assert_argument_error_without_github for-commit-ref pr for-commit main --repo riii111/dotfiles
assert_argument_error_without_github for-commit-non-hex pr for-commit 012345g --repo riii111/dotfiles
assert_argument_error_without_github for-commit-limit-zero pr for-commit 0123456 --limit 0 --repo riii111/dotfiles
assert_argument_error_without_github for-commit-limit-high pr for-commit 0123456 --limit 101 --repo riii111/dotfiles

commit_calls_file="$tmpdir/commit.calls"
run_cli for-commit-empty "GH_TEST_CALLS_FILE=$commit_calls_file" GH_TEST_COMMIT_RESPONSE=empty -- \
  pr for-commit 0123456 --repo riii111/dotfiles
assert_json '.data.repository == "riii111/dotfiles" and .data.pullRequests == [] and .data.truncated == false' \
  "$tmpdir/for-commit-empty.stdout" >/dev/null
grep -Fx 'api --method GET repos/riii111/dotfiles/commits/0123456/pulls?per_page=21' \
  "$commit_calls_file" >/dev/null

run_cli for-commit-no-search-marker GH_TEST_COMMIT_RESPONSE=wrong-marker -- \
  pr for-commit 0123456 --repo riii111/dotfiles
assert_json '.data.pullRequests[0].number == 1 and .data.truncated == false' \
  "$tmpdir/for-commit-no-search-marker.stdout" >/dev/null

run_cli for-commit-multiple GH_TEST_COMMIT_RESPONSE=multiple -- \
  pr for-commit 0123456789abcdef0123456789abcdef01234567 --repo riii111/dotfiles
assert_json '
  [.data.pullRequests[].number] == [10,11] and
  [.data.pullRequests[].isDraft] == [false,true] and
  (.data.pullRequests | all(keys == ["isDraft","number","state","title","updatedAt","url"])) and
  .data.truncated == false
' "$tmpdir/for-commit-multiple.stdout" >/dev/null
run_cli for-commit-truncated GH_TEST_COMMIT_RESPONSE=truncated -- \
  pr for-commit 0123456 --repo riii111/dotfiles --limit 2
assert_json '.data.truncated == true and (.data.pullRequests | length == 2)' \
  "$tmpdir/for-commit-truncated.stdout" >/dev/null
run_cli for-commit-limit-one -- pr for-commit 0123456 --repo riii111/dotfiles --limit 1
assert_json '(.data.pullRequests | length) == 1 and .data.truncated == true' \
  "$tmpdir/for-commit-limit-one.stdout" >/dev/null
run_cli for-commit-limit-boundaries -- pr for-commit 0123456 --repo riii111/dotfiles --limit 20
run_cli for-commit-limit-hundred -- pr for-commit 0123456 --repo riii111/dotfiles --limit 100

for response in wrapper invalid-item missing-field missing-draft; do
  assert_runtime_failure "for-commit-$response" invalidResponse runtime \
    GH_TEST_COMMIT_RESPONSE="$response" -- pr for-commit 0123456 --repo riii111/dotfiles
done
assert_runtime_failure for-commit-failure githubCli runtime \
  GH_TEST_COMMIT_RESPONSE=failure -- pr for-commit 0123456 --repo riii111/dotfiles

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
grep -F -- '--include-details' "$tmpdir/review-thread-help.stdout" >/dev/null
if grep -Eiq '(^|[[:space:]])pr (threads|thread)([[:space:]]|$)|data\.(threads|thread)' \
  "$tmpdir/review-thread-help.stdout"; then
  exit 1
fi
grep -F 'review_thread_id    one to 20 GraphQL review thread node IDs' \
  "$tmpdir/review-thread-help.stdout" >/dev/null

assert_target_guidance bare-pr-number pr 42
assert_target_guidance bare-pr-url pr https://github.com/riii111/dotfiles/pull/42
assert_target_guidance bare-issue-number issue 42
assert_target_guidance bare-issue-url issue https://github.com/riii111/dotfiles/issues/42

for invalid_target in typo 0 000 -1 \
  https://git.example.com/riii111/dotfiles/pull/42 \
  https://github.com/riii111/dotfiles/pull/42?tab=conversation \
  https://github.com/riii111/dotfiles/pull/42#discussion \
  https://github.com/riii111/dotfiles/pull/42/extra \
  https://github.com/riii111/dotfiles/issues/42 \
  https://github.com//dotfiles/pull/42; do
  assert_argument_error_without_github "bare-pr-invalid-${#invalid_target}" pr "$invalid_target"
  grep -F "argument subcommand: invalid choice: '$invalid_target'" \
    "$tmpdir/bare-pr-invalid-${#invalid_target}.stderr" >/dev/null
done
assert_argument_error_without_github bare-pr-range-outside pr 2147483648
grep -F "argument subcommand: invalid choice: '2147483648'" \
  "$tmpdir/bare-pr-range-outside.stderr" >/dev/null

for invalid_target in typo 0 000 -1 \
  https://git.example.com/riii111/dotfiles/issues/42 \
  https://github.com/riii111/dotfiles/issues/42?tab=conversation \
  https://github.com/riii111/dotfiles/issues/42#discussion \
  https://github.com/riii111/dotfiles/issues/42/extra \
  https://github.com/riii111/dotfiles/pull/42 \
  https://github.com//dotfiles/issues/42; do
  assert_argument_error_without_github "bare-issue-invalid-${#invalid_target}" issue "$invalid_target"
  grep -F "argument subcommand: invalid choice: '$invalid_target'" \
    "$tmpdir/bare-issue-invalid-${#invalid_target}.stderr" >/dev/null
done

assert_argument_error root-missing-resource
assert_argument_error pr-missing-subcommand pr
assert_argument_error issue-missing-subcommand issue
assert_argument_error issue-pr-only-option issue overview 42 --include-resolved
grep -Fx 'usage: gh-loupe issue overview [-h] [--repo REPO] target' \
  "$tmpdir/issue-pr-only-option.argument.stderr" >/dev/null
grep -F 'gh-loupe issue overview: error: unrecognized arguments: --include-resolved' \
  "$tmpdir/issue-pr-only-option.argument.stderr" >/dev/null

assert_argument_error issue-compact-option issue overview 42 --compact
grep -Fx 'usage: gh-loupe issue overview [-h] [--repo REPO] target' \
  "$tmpdir/issue-compact-option.argument.stderr" >/dev/null
grep -F 'gh-loupe issue overview: error: unrecognized arguments: --compact' \
  "$tmpdir/issue-compact-option.argument.stderr" >/dev/null

assert_argument_error issue-invalid-target issue overview 0
grep -Fx 'usage: gh-loupe issue overview [-h] [--repo REPO] target' \
  "$tmpdir/issue-invalid-target.argument.stderr" >/dev/null
grep -F 'gh-loupe issue overview: error: issue must be a positive number or GitHub issue URL' \
  "$tmpdir/issue-invalid-target.argument.stderr" >/dev/null

assert_argument_error issue-invalid-repo issue overview 42 --repo invalid
grep -F 'gh-loupe issue overview: error: --repo must use OWNER/REPO format' \
  "$tmpdir/issue-invalid-repo.argument.stderr" >/dev/null

assert_argument_error issue-invalid-url-repo issue overview https://github.com//dotfiles/issues/42
grep -F 'gh-loupe issue overview: error: issue URL must contain a valid OWNER/REPO' \
  "$tmpdir/issue-invalid-url-repo.argument.stderr" >/dev/null

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
run_cli issue-default "GH_TEST_CALLS_FILE=$issue_calls_file" -- issue overview 42
test ! -s "$tmpdir/issue-default.stderr"
assert_json '
  .schemaVersion == 1 and
  (.observedAt | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
  (.data | keys == ["issue","repository"]) and
  .data.repository == "riii111/dotfiles" and
  .data.issue == {
    "number":42,
    "title":"Issue",
    "url":"https://github.com/riii111/dotfiles/issues/42",
    "state":"OPEN",
    "stateReason":null,
    "body":"body",
    "author":"author",
    "labels":["alpha","zeta"],
    "assignees":["alice","zoe"],
    "milestone":{"title":"v1","state":"OPEN","dueOn":null},
    "createdAt":"2026-08-12T00:00:00Z",
    "updatedAt":"2026-08-12T01:00:00Z",
    "closedAt":null,
    "subIssues":{"total":2,"completed":1},
    "dependencies":{"blockedBy":2,"blocking":1}
  } and
  (.data.issue | keys == ["assignees","author","body","closedAt","createdAt","dependencies","labels","milestone","number","state","stateReason","subIssues","title","updatedAt","url"]) and
  ([.. | objects | keys[]] | any(. == "repository_url" or . == "labels_url" or . == "comments_url" or . == "reactions" or . == "pull_request" or . == "performed_via_github_app") | not) and
  ([.. | strings | select(test("^https://api\\.github\\.com/"))] | length == 0)
' "$tmpdir/issue-default.stdout" >/dev/null
test "$(sed -n '1p' "$issue_calls_file")" = 'repo view --json nameWithOwner'
grep -Fx 'api repos/riii111/dotfiles/issues/42' "$issue_calls_file" >/dev/null
test "$(wc -l <"$issue_calls_file")" -eq 2

issue_timing_file="$tmpdir/issue.timing"
run_cli issue-overview-timing \
  "GH_TEST_ISSUE_TIMING_FILE=$issue_timing_file" \
  GH_TEST_ISSUE_DELAY=0.2 -- \
  issue overview 42 --repo riii111/dotfiles
test "$(wc -l <"$issue_timing_file")" -eq 2
test "$(sed -n '1p' "$issue_timing_file" | cut -d' ' -f2)" = start
test "$(sed -n '2p' "$issue_timing_file" | cut -d' ' -f2)" = end
test "$(grep -c '^issue start$' "$issue_timing_file")" -eq 1

assert_runtime_failure issue-error-precedence notFound issue \
  GH_TEST_MISSING_ISSUE=1 -- \
  issue overview 42 --repo riii111/dotfiles

run_cli issue-url -- issue overview https://github.com/riii111/dotfiles/issues/42
test ! -s "$tmpdir/issue-url.stderr"
test "$(wc -l <"$tmpdir/issue-url.stdout" | tr -d ' ')" -eq 1
assert_json '.schemaVersion == 1 and .data.issue.number == 42 and (.data | has("comments") | not)' \
  "$tmpdir/issue-url.stdout" >/dev/null

run_cli issue-nullable GH_TEST_ISSUE_RESPONSE=nullable -- issue overview 42 --repo riii111/dotfiles
jq -e '
  .data.issue == {
    "number":42,
    "title":"Issue",
    "url":"https://github.com/riii111/dotfiles/issues/42",
    "state":"CLOSED",
    "stateReason":"NOT_PLANNED",
    "body":null,
    "author":null,
    "labels":[],
    "assignees":[],
    "milestone":null,
    "createdAt":"2026-08-12T00:00:00Z",
    "updatedAt":"2026-08-12T01:00:00Z",
    "closedAt":"2026-08-12T02:00:00Z",
    "subIssues":null,
    "dependencies":null
  }
' "$tmpdir/issue-nullable.stdout" >/dev/null

run_cli issue-missing-summaries GH_TEST_ISSUE_RESPONSE=missing-summary -- issue overview 42 --repo riii111/dotfiles
jq -e '.data.issue.subIssues == null and .data.issue.dependencies == null' \
  "$tmpdir/issue-missing-summaries.stdout" >/dev/null

if env PATH="$tmpdir/bin:$PATH" "$tmpdir/rust/gh-loupe" \
  issue overview https://github.com/riii111/dotfiles/issues/42 --repo other/repo \
  >"$tmpdir/issue-conflicting-repo.stdout" 2>"$tmpdir/issue-conflicting-repo.stderr"; then
  issue_status=0
else
  issue_status=$?
fi
test "$issue_status" -eq 2
test ! -s "$tmpdir/issue-conflicting-repo.stdout"
grep -Fx 'usage: gh-loupe issue overview [-h] [--repo REPO] target' \
  "$tmpdir/issue-conflicting-repo.stderr" >/dev/null
grep -F 'gh-loupe issue overview: error: --repo conflicts with the issue URL' \
  "$tmpdir/issue-conflicting-repo.stderr" >/dev/null

run_cli issue-utf8 GH_TEST_UTF8=1 -- issue overview 42
assert_json '.data.issue.title == "日本語のIssue" and .data.issue.body == "ずんだ"' \
  "$tmpdir/issue-utf8.stdout" >/dev/null

assert_runtime_failure issue-missing notFound issue \
  GH_TEST_MISSING_ISSUE=1 -- issue overview 42
assert_runtime_failure issue-gh-failure githubCli issue \
  GH_TEST_FAILURE=1 -- issue overview 42 --repo riii111/dotfiles
assert_runtime_failure issue-invalid-json invalidResponse issue \
  GH_TEST_INVALID_JSON=1 -- issue overview 42 --repo riii111/dotfiles
assert_runtime_failure issue-invalid-page invalidResponse issue \
  GH_TEST_ISSUE_RESPONSE=malformed -- issue overview 42 --repo riii111/dotfiles
assert_runtime_failure issue-invalid-item invalidResponse issue \
  GH_TEST_ISSUE_RESPONSE=malformed -- issue overview 42 --repo riii111/dotfiles
assert_runtime_failure issue-malformed invalidResponse issue \
  GH_TEST_ISSUE_RESPONSE=malformed -- issue overview 42 --repo riii111/dotfiles
assert_runtime_failure issue-pr-target invalidResponse issue \
  GH_TEST_ISSUE_RESPONSE=pr-marker -- issue overview 42 --repo riii111/dotfiles
grep -F 'use the pr commands' "$tmpdir/issue-pr-target.issue.stderr" >/dev/null
assert_runtime_failure issue-null-pr-target invalidResponse issue \
  GH_TEST_ISSUE_RESPONSE=null-pr-marker -- issue overview 42 --repo riii111/dotfiles
grep -F 'use the pr commands' "$tmpdir/issue-null-pr-target.issue.stderr" >/dev/null
assert_runtime_failure issue-comments-missing-field invalidResponse issue \
  GH_TEST_ISSUE_RESPONSE=malformed -- issue overview 42 --repo riii111/dotfiles
assert_runtime_failure issue-comments-wrong-type invalidResponse issue \
  GH_TEST_ISSUE_RESPONSE=malformed -- issue overview 42 --repo riii111/dotfiles
assert_runtime_failure issue-missing-repository-metadata invalidResponse issue \
  GH_TEST_REPO_METADATA_MISSING=1 -- issue overview 42
assert_overview_runtime_error overview-stdin-failure githubCli false \
  GH_TEST_STDIN_FAILURE=1 -- pr overview 42 --repo riii111/dotfiles
assert_runtime_failure issue-nonzero-valid-json githubCli issue \
  GH_TEST_NONZERO_VALID_JSON=1 -- issue overview 42 --repo riii111/dotfiles
assert_runtime_failure issue-rate-limit-resource githubCli issue \
  GH_TEST_RATE_LIMIT_RESOURCE=1 -- issue overview 42 --repo riii111/dotfiles
assert_runtime_failure issue-comments-page-failure invalidResponse issue \
  GH_TEST_ISSUE_RESPONSE=malformed -- issue overview 42 --repo riii111/dotfiles

if env PATH="$tmpdir/missing-gh" "$tmpdir/rust/gh-loupe" issue overview 42 --repo riii111/dotfiles \
  >"$tmpdir/issue-spawn.stdout" 2>"$tmpdir/issue-spawn.stderr"; then
  issue_status=0
else
  issue_status=$?
fi
test "$issue_status" -ne 0
test ! -s "$tmpdir/issue-spawn.stdout"
test "$(wc -l <"$tmpdir/issue-spawn.stderr")" -eq 1
assert_json '
  .schemaVersion == 1 and
  .error.kind == "githubCli" and
  (.error.message | type == "string") and
  .error.retryable == false and
  .error.retryAfterSeconds == null
' "$tmpdir/issue-spawn.stderr" >/dev/null

issue_comments_calls_file="$tmpdir/issue-comments.calls"
run_cli issue-comments-default "GH_TEST_CALLS_FILE=$issue_comments_calls_file" -- \
  issue comments https://github.com/riii111/dotfiles/issues/42
test ! -s "$tmpdir/issue-comments-default.stderr"
assert_json '
  .schemaVersion == 1 and
  (.data | keys == ["comments","repository"]) and
  .data.repository == "riii111/dotfiles" and
  [.data.comments[].id] == ["IC_a","IC_b","IC_z"] and
  (.data.comments | all(keys == ["author","body","createdAt","id","updatedAt","url"])) and
  ([.. | objects | keys[]] | any(. == "reactions" or . == "pull_request" or . == "comments_url") | not)
' "$tmpdir/issue-comments-default.stdout" >/dev/null
grep -Fx 'api repos/riii111/dotfiles/issues/42' "$issue_comments_calls_file" >/dev/null
grep -Fx 'api --method GET --paginate --slurp repos/riii111/dotfiles/issues/42/comments?per_page=100' \
  "$issue_comments_calls_file" >/dev/null
test "$(wc -l <"$issue_comments_calls_file")" -eq 2

issue_overview_calls_file="$tmpdir/issue-overview.calls"
run_cli issue-overview-no-comments "GH_TEST_CALLS_FILE=$issue_overview_calls_file" -- \
  issue overview 42 --repo riii111/dotfiles
test "$(grep -c 'issues/42/comments' "$issue_overview_calls_file" || true)" -eq 0
test "$(grep -c 'issues/42$' "$issue_overview_calls_file")" -eq 1

assert_runtime_failure issue-comments-pr-marker invalidResponse issue \
  GH_TEST_ISSUE_RESPONSE=pr-marker -- issue comments 42 --repo riii111/dotfiles
grep -F 'use the pr commands' "$tmpdir/issue-comments-pr-marker.issue.stderr" >/dev/null
assert_runtime_failure issue-comments-null-pr-marker invalidResponse issue \
  GH_TEST_ISSUE_RESPONSE=null-pr-marker -- issue comments 42 --repo riii111/dotfiles
assert_runtime_failure issue-comments-invalid-page invalidResponse issue \
  GH_TEST_ISSUE_COMMENTS=invalid-page -- issue comments 42 --repo riii111/dotfiles
assert_runtime_failure issue-comments-invalid-item invalidResponse issue \
  GH_TEST_ISSUE_COMMENTS=invalid-item -- issue comments 42 --repo riii111/dotfiles
assert_runtime_failure issue-comments-missing-field invalidResponse issue \
  GH_TEST_ISSUE_COMMENTS=missing-field -- issue comments 42 --repo riii111/dotfiles
assert_runtime_failure issue-comments-wrong-type invalidResponse issue \
  GH_TEST_ISSUE_COMMENTS=wrong-type -- issue comments 42 --repo riii111/dotfiles
assert_runtime_failure issue-comments-page-failure githubCli issue \
  GH_TEST_ISSUE_COMMENTS_FAILURE=1 -- issue comments 42 --repo riii111/dotfiles

assert_argument_error issue-relations-missing-target issue relations
assert_argument_error issue-relations-limit-zero issue relations 42 --limit 0
assert_argument_error issue-relations-limit-high issue relations 42 --limit 101
assert_argument_error issue-relations-limit-equals issue relations 42 --limit=0
assert_argument_error issue-relations-abbreviated-limit issue relations 42 --lim 10

run_cli issue-relations-default "GH_TEST_CALLS_FILE=$tmpdir/relations.calls" -- \
  issue relations 42 --repo riii111/dotfiles
test ! -s "$tmpdir/issue-relations-default.stderr"
assert_json '
  .schemaVersion == 1 and
  (.data | keys == ["blockedBy","blocking","parent","repository","subIssues"]) and
  .data.repository == "riii111/dotfiles" and
  .data.parent.repository == "riii111/dotfiles" and
  .data.parent.number == 7 and
  [.data.subIssues.items[].number] == [43,3,44] and
  .data.subIssues.totalCount == 3 and .data.subIssues.truncated == false and
  .data.blockedBy.items[0].number == 45 and
  .data.blocking.items[0].number == 46 and
  ([.. | objects | keys[]] | any(. == "body" or . == "comments" or . == "reactions" or . == "repository_url") | not)
' "$tmpdir/issue-relations-default.stdout" >/dev/null
test "$(grep -c 'api graphql --input -' "$tmpdir/relations.calls")" -eq 1

run_cli issue-relations-limit "GH_TEST_RELATIONS_RESPONSE=success" -- \
  issue relations 42 --repo riii111/dotfiles --limit=2
assert_json '.data.subIssues.totalCount == 3 and .data.subIssues.truncated and (.data.subIssues.items | length == 2)' \
  "$tmpdir/issue-relations-limit.stdout" >/dev/null
run_cli issue-relations-empty GH_TEST_RELATIONS_RESPONSE=empty -- \
  issue relations 42 --repo riii111/dotfiles --limit 1
assert_json '.data.parent == null and .data.subIssues == {"items":[],"totalCount":0,"truncated":false} and .data.blockedBy.items == [] and .data.blocking.items == []' \
  "$tmpdir/issue-relations-empty.stdout" >/dev/null
assert_runtime_failure issue-relations-wrong-repo invalidResponse runtime \
  GH_TEST_RELATIONS_RESPONSE=wrong-repo -- issue relations 42 --repo riii111/dotfiles
assert_runtime_failure issue-relations-wrong-issue invalidResponse runtime \
  GH_TEST_RELATIONS_RESPONSE=wrong-issue -- issue relations 42 --repo riii111/dotfiles
assert_runtime_failure issue-relations-missing invalidResponse runtime \
  GH_TEST_RELATIONS_RESPONSE=missing -- issue relations 42 --repo riii111/dotfiles
assert_runtime_failure issue-relations-wrong-type invalidResponse runtime \
  GH_TEST_RELATIONS_RESPONSE=wrong-type -- issue relations 42 --repo riii111/dotfiles
assert_runtime_failure issue-relations-wrong-state-reason invalidResponse runtime \
  GH_TEST_RELATIONS_RESPONSE=wrong-state-reason -- issue relations 42 --repo riii111/dotfiles
assert_runtime_failure issue-relations-wrong-assignees invalidResponse runtime \
  GH_TEST_RELATIONS_RESPONSE=wrong-assignees -- issue relations 42 --repo riii111/dotfiles
assert_runtime_failure issue-relations-graphql-error authorization runtime \
  GH_TEST_RELATIONS_GRAPHQL_ERROR=1 -- issue relations 42 --repo riii111/dotfiles

run_overview overview-default -- pr overview 42 --repo riii111/dotfiles
assert_json '
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
test "$(wc -l <"$tmpdir/overview-default.overview.stdout")" -eq 1

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

run_overview overview-null-fields GH_OVERVIEW_NULL_FIELDS=1 -- pr overview 42 --repo riii111/dotfiles
assert_json '
  .data.pullRequest.state == null and
  .data.pullRequest.isDraft == null and
  .data.pullRequest.headRefOid == null and
  .data.pullRequest.baseRefOid == null and
  .data.pullRequest.reviewDecision == null and
  .data.pullRequest.mergeStateStatus == null
' "$tmpdir/overview-null-fields.overview.stdout" >/dev/null

run_overview overview-no-required GH_OVERVIEW_CHECKS=empty -- pr overview 42 --repo riii111/dotfiles
assert_json '.data.checks == {
  "required": 0,
  "passed": 0,
  "pending": 0,
  "failed": 0,
  "all": {"total": 7, "passed": 3, "pending": 2, "failed": 2}
}' \
  "$tmpdir/overview-no-required.overview.stdout" >/dev/null

run_overview overview-no-required-cli GH_OVERVIEW_CHECKS=no-required -- pr overview 42 --repo riii111/dotfiles
assert_json '.data.checks == {
  "required": 0,
  "passed": 0,
  "pending": 0,
  "failed": 0,
  "all": {"total": 7, "passed": 3, "pending": 2, "failed": 2}
}' \
  "$tmpdir/overview-no-required-cli.overview.stdout" >/dev/null

run_overview overview-empty-all GH_OVERVIEW_ALL_CHECKS=no-checks -- pr overview 42 --repo riii111/dotfiles
assert_json '.data.checks.all == {"total": 0, "passed": 0, "pending": 0, "failed": 0}' \
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
  'GitHub GraphQL error: [{"message":"simulated GraphQL failure"}]' \
  GH_TEST_GRAPHQL_ERROR=1 GH_OVERVIEW_REQUIRED_FAILURE=1 GH_OVERVIEW_ALL_FAILURE=1 -- \
  pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-graphql-unauthorized authentication false \
  GH_TEST_GRAPHQL_ERROR=1 GH_TEST_GRAPHQL_ERROR_TYPE=UNAUTHORIZED -- \
  pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-graphql-forbidden authorization false \
  GH_TEST_GRAPHQL_ERROR=1 GH_TEST_GRAPHQL_ERROR_TYPE=FORBIDDEN -- \
  pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-graphql-not-found notFound false \
  GH_TEST_GRAPHQL_ERROR=1 GH_TEST_GRAPHQL_ERROR_TYPE=NOT_FOUND -- \
  pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-graphql-rate-limited rateLimited true \
  GH_TEST_GRAPHQL_ERROR=1 GH_TEST_GRAPHQL_ERROR_TYPE=RATE_LIMITED -- \
  pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-graphql-unknown githubCli false \
  GH_TEST_GRAPHQL_ERROR=1 GH_TEST_GRAPHQL_ERROR_TYPE=UNKNOWN -- \
  pr overview 42 --repo riii111/dotfiles
assert_overview_runtime_error overview-graphql-process-failure githubCli false \
  GH_OVERVIEW_GRAPHQL_FAILURE=1 -- pr overview 42 --repo riii111/dotfiles
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
assert_argument_error comments-abbreviated-option pr comments 42 --comp
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
grep -Fx 'usage: gh-loupe pr comments [-h] [--repo REPO] target' \
  "$tmpdir/comments-invalid.stderr" >/dev/null
grep -F 'gh-loupe pr comments: error: pr must be a positive number within GitHub GraphQL Int range or GitHub pr URL' \
  "$tmpdir/comments-invalid.stderr" >/dev/null

calls_file="$tmpdir/comments.calls"
run_comments comments-default "GH_TEST_CALLS_FILE=$calls_file" GH_PR_COMMENTS=success -- \
  pr comments 42 --repo riii111/dotfiles
assert_json '
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
  'api --method GET --paginate --slurp repos/riii111/dotfiles/issues/42/comments?per_page=100'
test "$(wc -l <"$tmpdir/comments-default.comments.stdout")" -eq 1

run_comments comments-url-empty GH_PR_COMMENTS=empty -- \
  pr comments https://github.com/riii111/dotfiles/pull/42
assert_json '.data.comments == []' "$tmpdir/comments-url-empty.comments.stdout" >/dev/null
test "$(wc -l <"$tmpdir/comments-url-empty.comments.stdout")" -eq 1

calls_file="$tmpdir/comments-inferred.calls"
run_comments comments-inferred-repo "GH_TEST_CALLS_FILE=$calls_file" GH_PR_COMMENTS=empty -- \
  pr comments 42
test "$(sed -n '1p' "$calls_file")" = 'repo view --json nameWithOwner'
test "$(sed -n '2p' "$calls_file")" = \
  'api --method GET --paginate --slurp repos/riii111/dotfiles/issues/42/comments?per_page=100'
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
assert_argument_error threads-abbreviated-option pr review-threads 42 --comp
assert_argument_error threads-abbreviated-include-resolved pr review-threads 42 --incl
assert_argument_error threads-invalid-zero pr review-threads 0 --repo riii111/dotfiles
assert_argument_error threads-invalid-repo pr review-threads 42 --repo ../..
assert_argument_error threads-conflicting-repo \
  pr review-threads https://github.com/riii111/dotfiles/pull/42 --repo other/repo

run_review_threads threads-default GH_FAIL_RESOLVED_COMMENTS=1 -- \
  pr review-threads 42 --repo riii111/dotfiles
assert_json '
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
test "$(wc -l <"$tmpdir/threads-default.review_threads.stdout")" -eq 1

run_review_threads threads-including-resolved -- \
  pr review-threads 42 --repo riii111/dotfiles --include-resolved
assert_json '
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
assert_argument_error thread-abbreviated-option pr review-thread 42 thread-detail --comp
assert_argument_error thread-abbreviated-diff-hunk pr review-thread 42 thread-detail --incl
assert_argument_error thread-abbreviated-details pr review-thread 42 thread-detail --include-det
assert_argument_error thread-details-value pr review-thread 42 thread-detail --include-details=true
assert_argument_error thread-invalid-target pr review-thread nope thread-detail --repo riii111/dotfiles
assert_argument_error thread-conflicting-repo \
  pr review-thread https://github.com/riii111/dotfiles/pull/42 thread-detail --repo other/repo

run_review_thread thread-default -- pr review-thread 42 thread-detail --repo riii111/dotfiles
assert_json '
  .data.reviewThreads == [{
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
    ],
    "detailsOmitted": false
  }] and
  ([.. | objects | keys[]] | any(. == "diffHunk" or . == "databaseId" or . == "resolvedBy" or . == "replyTo" or . == "pullRequest") | not)
' "$tmpdir/thread-default.review_thread.stdout" >/dev/null
test "$(wc -l <"$tmpdir/thread-default.review_thread.stdout")" -eq 1

run_review_thread thread-diff-hunk -- \
  pr review-thread 42 thread-detail --repo riii111/dotfiles --include-diff-hunk
assert_json '
  [.data.reviewThreads[0].comments[].diffHunk] == ["@@ a", "@@ b", "@@ z"] and
  (.data.reviewThreads[0].comments | all(keys == ["author", "body", "createdAt", "diffHunk", "id", "replyToId", "updatedAt", "url"]))
' "$tmpdir/thread-diff-hunk.review_thread.stdout" >/dev/null
test "$(wc -l <"$tmpdir/thread-diff-hunk.review_thread.stdout")" -eq 1

run_review_thread thread-details-default GH_TEST_THREAD_DETAILS=1 -- \
  pr review-thread 42 thread-detail --repo riii111/dotfiles
assert_json '
  .data.reviewThreads[0].detailsOmitted == true and
  [.data.reviewThreads[0].comments[].body] == [
    "human\n証拠\nreply",
    "second by id",
    "before\n結論\nafter"
  ]
' "$tmpdir/thread-details-default.review_thread.stdout" >/dev/null

run_review_thread thread-details-included GH_TEST_THREAD_DETAILS=1 -- \
  pr review-thread 42 thread-detail --repo riii111/dotfiles --include-details
assert_json '
  .data.reviewThreads[0].detailsOmitted == false and
  (.data.reviewThreads[0] | keys == ["comments", "detailsOmitted", "diffSide", "id", "isOutdated", "isResolved", "line", "originalLine", "path", "startLine"]) and
  [.data.reviewThreads[0].comments[].body] == [
    "human\n<details data-source=\"bot\">\n<summary>証拠</summary>\n省略\n</details>\nreply",
    "second by id",
    "before\n<DETAILS class=\"evidence\">\n<SUMMARY>結論</SUMMARY>\n折り畳み内容\n</DETAILS>\nafter"
  ]
' "$tmpdir/thread-details-included.review_thread.stdout" >/dev/null
test "$(wc -l <"$tmpdir/thread-details-included.review_thread.stdout")" -eq 1

run_review_thread thread-multiple GH_TEST_THREAD_DETAILS=1 -- \
  pr review-thread 42 thread-a thread-b --repo riii111/dotfiles
assert_json '
  [.data.reviewThreads[].id] == ["thread-a", "thread-b"] and
  [.data.reviewThreads[].detailsOmitted] == [true, false] and
  ([.data.reviewThreads[].comments[] | has("diffHunk")] | all == false)
' "$tmpdir/thread-multiple.review_thread.stdout" >/dev/null

run_review_thread thread-multiple-options GH_TEST_THREAD_DETAILS=1 -- \
  pr review-thread 42 thread-a thread-b --repo riii111/dotfiles --include-details --include-diff-hunk
assert_json '
  [.data.reviewThreads[].id] == ["thread-a", "thread-b"] and
  ([.data.reviewThreads[].detailsOmitted] | all == false) and
  ([.data.reviewThreads[].comments[] | has("diffHunk")] | all == true)
' "$tmpdir/thread-multiple-options.review_thread.stdout" >/dev/null

run_review_thread thread-many-one GH_TEST_THREAD_DETAIL_MANY=1 -- \
  pr review-thread 42 thread-a --repo riii111/dotfiles
assert_json '.data.reviewThreads[0].comments | length == 101' \
  "$tmpdir/thread-many-one.review_thread.stdout" >/dev/null

run_review_thread thread-many-multiple GH_TEST_THREAD_DETAIL_MANY=1 -- \
  pr review-thread 42 thread-a thread-b --repo riii111/dotfiles
assert_json '[.data.reviewThreads[].comments | length] == [101, 101]' \
  "$tmpdir/thread-many-multiple.review_thread.stdout" >/dev/null

assert_argument_error_without_github thread-empty-id \
  pr review-thread 42 "" --repo riii111/dotfiles
assert_argument_error_without_github thread-duplicate-id \
  pr review-thread 42 thread-a thread-a --repo riii111/dotfiles
over_limit_ids=()
for index in $(seq 1 21); do
  over_limit_ids+=("thread-$index")
done
assert_argument_error_without_github thread-over-limit \
  pr review-thread 42 "${over_limit_ids[@]}" --repo riii111/dotfiles
assert_argument_error_without_github thread-query-passthrough \
  pr review-thread 42 thread-a --repo riii111/dotfiles --query arbitrary

payloads_file="$tmpdir/thread-query.payloads"
run_review_thread thread-query-shape GH_TEST_GRAPHQL_PAYLOADS_FILE="$payloads_file" -- \
  pr review-thread 42 thread-a thread-b --repo riii111/dotfiles
assert_json -s "
  length == 3 and
  .[0].variables.ids == [\"thread-a\", \"thread-b\"] and
  (.[0].query | contains(\"nodes(ids: \$ids)\")) and
  ([.[1:][] | .variables | keys] | all(. == [\"cursor\", \"id\"]))
" "$payloads_file" >/dev/null

assert_runtime_failure thread-second-page-failure githubCli runtime \
  GH_TEST_THREAD_DETAIL_SECOND_PAGE_FAILURE=1 -- \
  pr review-thread 42 thread-a thread-b --repo riii111/dotfiles
grep -F 'thread-b' "$tmpdir/thread-second-page-failure.runtime.stderr" >/dev/null

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
assert_runtime_failure thread-wrong-id invalidResponse runtime \
  GH_TEST_THREAD_DETAIL=wrong-id -- pr review-thread 42 thread-detail --repo riii111/dotfiles
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
