#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage: ./install.sh [--binary-root DIR] [--skill-root DIR]

  --binary-root DIR  Install gh-read to DIR/bin/gh-read (default: $HOME/.cargo)
  --skill-root DIR   Install the Skill to DIR/gh-read (default: $HOME/.codex/skills)
EOF
}

binary_root="${HOME:?HOME must be set}/.cargo"
skill_root="$HOME/.codex/skills"

while (($# > 0)); do
  case "$1" in
    --binary-root)
      (($# >= 2)) || { printf '%s\n' 'error: --binary-root requires a directory' >&2; exit 2; }
      binary_root=$2
      shift 2
      ;;
    --skill-root)
      (($# >= 2)) || { printf '%s\n' 'error: --skill-root requires a directory' >&2; exit 2; }
      skill_root=$2
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      printf 'error: unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

repository=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
skill_source="$repository/skills/gh-read"
[[ -f "$skill_source/SKILL.md" ]] || { printf '%s\n' 'error: bundled gh-read Skill is missing' >&2; exit 1; }

package_version=$(awk '
  /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
  /^\[/ { in_workspace_package = 0 }
  in_workspace_package && /^version = "/ {
    version = $0
    sub(/^version = "/, "", version)
    sub(/".*$/, "", version)
    print version
    exit
  }
' "$repository/Cargo.toml")
minimum_version=$(sed -n 's/^Minimum gh-read version: //p' "$skill_source/SKILL.md")

[[ -n "$package_version" ]] || { printf '%s\n' 'error: Cargo package version could not be read' >&2; exit 1; }
[[ "$minimum_version" == "$package_version" ]] || {
  printf 'error: Skill minimum version %s does not match Cargo package version %s\n' \
    "${minimum_version:-<missing>}" "$package_version" >&2
  exit 1
}

mkdir -p "$binary_root/bin" "$skill_root"
binary_root=$(CDPATH= cd -- "$binary_root" && pwd -P)
skill_root=$(CDPATH= cd -- "$skill_root" && pwd -P)
binary_destination="$binary_root/bin/gh-read"
skill_destination="$skill_root/gh-read"

if [[ -d "$binary_destination" && ! -L "$binary_destination" ]]; then
  printf 'error: binary destination is a directory: %s\n' "$binary_destination" >&2
  exit 1
fi

printf 'Binary destination: %s\n' "$binary_destination"
printf 'Skill destination: %s\n' "$skill_destination"

cargo_work=
binary_work=
skill_work=
binary_replaced=0
binary_had_previous=0
skill_replaced=0
skill_had_previous=0
committed=0

cleanup() {
  status=$?
  trap - EXIT
  set +e
  binary_keep_work=0
  skill_keep_work=0
  cleanup_failed=0

  if ((committed == 0)); then
    if ((skill_replaced == 1)); then
      if [[ -e "$skill_destination" || -L "$skill_destination" ]] &&
        ! mv -- "$skill_destination" "$skill_work/failed"; then
        if ((skill_had_previous == 1)); then
          printf 'error: could not move the failed Skill; previous Skill retained at %s\n' \
            "$skill_work/previous" >&2
        else
          printf 'error: could not remove the failed Skill at %s\n' "$skill_destination" >&2
        fi
        skill_keep_work=1
        cleanup_failed=1
      elif ((skill_had_previous == 1)); then
        if mv -- "$skill_work/previous" "$skill_destination"; then
          skill_had_previous=0
        else
          printf 'error: could not restore the previous Skill; recover it from %s\n' \
            "$skill_work/previous" >&2
          skill_keep_work=1
          cleanup_failed=1
        fi
      fi
    fi
    if ((binary_replaced == 1)); then
      if [[ -e "$binary_destination" || -L "$binary_destination" ]] &&
        ! mv -- "$binary_destination" "$binary_work/failed"; then
        if ((binary_had_previous == 1)); then
          printf 'error: could not move the failed binary; previous binary retained at %s\n' \
            "$binary_work/previous" >&2
        else
          printf 'error: could not remove the failed binary at %s\n' "$binary_destination" >&2
        fi
        binary_keep_work=1
        cleanup_failed=1
      elif ((binary_had_previous == 1)); then
        if mv -- "$binary_work/previous" "$binary_destination"; then
          binary_had_previous=0
        else
          printf 'error: could not restore the previous binary; recover it from %s\n' \
            "$binary_work/previous" >&2
          binary_keep_work=1
          cleanup_failed=1
        fi
      fi
    fi
  fi

  if [[ -n "$cargo_work" ]] && ! rm -rf -- "$cargo_work"; then
    printf 'error: could not remove temporary directory %s\n' "$cargo_work" >&2
    cleanup_failed=1
  fi
  if [[ -n "$binary_work" ]] && ((binary_keep_work == 0)) &&
    ! rm -rf -- "$binary_work"; then
    printf 'error: could not remove temporary directory %s\n' "$binary_work" >&2
    cleanup_failed=1
  fi
  if [[ -n "$skill_work" ]] && ((skill_keep_work == 0)) &&
    ! rm -rf -- "$skill_work"; then
    printf 'error: could not remove temporary directory %s\n' "$skill_work" >&2
    cleanup_failed=1
  fi
  if ((cleanup_failed == 1 && status == 0)); then
    status=1
  fi
  exit "$status"
}
trap cleanup EXIT

cargo_work=$(mktemp -d "${TMPDIR:-/tmp}/gh-read-cargo.XXXXXX")
binary_work=$(mktemp -d "$binary_root/bin/.gh-read.install.XXXXXX")
skill_work=$(mktemp -d "$skill_root/.gh-read.install.XXXXXX")

cargo_command=${CARGO:-cargo}
CARGO_TARGET_DIR="$cargo_work/target" "$cargo_command" install \
  --path "$repository" \
  --locked \
  --force \
  --root "$cargo_work/root" \
  --quiet

install -m 755 "$cargo_work/root/bin/gh-read" "$binary_work/new"
mkdir "$skill_work/new"
cp -R "$skill_source/." "$skill_work/new/"

staged_version=$("$binary_work/new" --version)
[[ "$staged_version" == "gh-read $package_version" ]] || {
  printf 'error: staged binary version is %s, expected gh-read %s\n' "$staged_version" "$package_version" >&2
  exit 1
}
diff -qr "$skill_source" "$skill_work/new" >/dev/null

if [[ -e "$binary_destination" || -L "$binary_destination" ]]; then
  mv -- "$binary_destination" "$binary_work/previous"
  binary_had_previous=1
  binary_replaced=1
fi
mv -- "$binary_work/new" "$binary_destination"
binary_replaced=1

if [[ -e "$skill_destination" || -L "$skill_destination" ]]; then
  mv -- "$skill_destination" "$skill_work/previous"
  skill_had_previous=1
  skill_replaced=1
fi
mv -- "$skill_work/new" "$skill_destination"
skill_replaced=1

installed_version=$("$binary_destination" --version)
[[ "$installed_version" == "gh-read $package_version" ]] || {
  printf 'error: installed binary version is %s, expected gh-read %s\n' "$installed_version" "$package_version" >&2
  exit 1
}
diff -qr "$skill_source" "$skill_destination" >/dev/null || {
  printf '%s\n' 'error: installed Skill does not match the bundled Skill' >&2
  exit 1
}

committed=1
printf 'Installed gh-read %s and its bundled Skill.\n' "$package_version"
