#!/usr/bin/env bash

assert_json() {
  local arg
  local input
  local -a files=()

  for arg in "$@"; do
    if [ -f "$arg" ]; then
      files+=("$arg")
    fi
  done

  if [ "${#files[@]}" -gt 0 ]; then
    if jq -e "$@" >/dev/null; then
      return 0
    fi
    for file in "${files[@]}"; do
      printf 'assertion input: %s\n' "$file" >&2
      cat "$file" >&2
    done
    return 1
  fi

  input="$(cat)"
  if jq -e "$@" <<<"$input" >/dev/null; then
    return 0
  fi
  printf 'assertion input: stdin\n%s\n' "$input" >&2
  return 1
}
