#!/usr/bin/env sh
set -eu

repo="${LILACCAPS_REPO:-zhuisDEV/lilaccaps}"
git_url="https://github.com/${repo}.git"
package="lilaccaps"

print_usage() {
  cat <<'USAGE'
Install lilaccaps globally from GitHub.

Usage:
  sh install.sh [--fix] [--tag <tag>] [--branch <branch>] [--repo <owner/name>]

Options:
  --fix              Run `lilaccaps install --fix` after installing the binary.
  --tag <tag>        Install a specific Git tag.
  --branch <branch>  Install a specific Git branch.
  --repo <owner/name>
                     Install from a different GitHub repository.
  -h, --help         Show this help.

Environment:
  LILACCAPS_REPO     GitHub repository, default: zhuisDEV/lilaccaps.
  LILACCAPS_TAG      Git tag to install.
  LILACCAPS_BRANCH   Git branch to install.
  LILACCAPS_INSTALL_ROOT
                     Cargo install root, default: CARGO_HOME or ~/.cargo.
USAGE
}

has_command() {
  command -v "$1" >/dev/null 2>&1
}

install_build_prereqs() {
  if has_command cmake && cmake --version >/dev/null 2>&1; then
    return 0
  fi

  if [ "${fix:-0}" = "1" ] && [ "$(uname -s)" = "Darwin" ] && has_command brew; then
    if brew list --versions cmake >/dev/null 2>&1; then
      brew reinstall cmake
    else
      brew install cmake
    fi
    return 0
  fi

  printf '%s\n' "cmake is required before lilaccaps can be built from GitHub." >&2
  printf '%s\n' "Install cmake, then rerun this installer." >&2
  if [ "$(uname -s)" = "Darwin" ]; then
    printf '%s\n' "On macOS with Homebrew: brew install cmake" >&2
  fi
  exit 1
}

fix=0
tag="${LILACCAPS_TAG:-}"
branch="${LILACCAPS_BRANCH:-}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --fix)
      fix=1
      shift
      ;;
    --tag)
      if [ "$#" -lt 2 ]; then
        printf '%s\n' "--tag requires a value" >&2
        exit 1
      fi
      tag="$2"
      shift 2
      ;;
    --branch)
      if [ "$#" -lt 2 ]; then
        printf '%s\n' "--branch requires a value" >&2
        exit 1
      fi
      branch="$2"
      shift 2
      ;;
    --repo)
      if [ "$#" -lt 2 ]; then
        printf '%s\n' "--repo requires a value" >&2
        exit 1
      fi
      repo="$2"
      git_url="https://github.com/${repo}.git"
      shift 2
      ;;
    -h | --help)
      print_usage
      exit 0
      ;;
    *)
      printf '%s\n' "unknown option: $1" >&2
      print_usage >&2
      exit 1
      ;;
  esac
done

if [ -n "$tag" ] && [ -n "$branch" ]; then
  printf '%s\n' "choose either --tag or --branch, not both" >&2
  exit 1
fi

if ! has_command cargo || ! cargo --version >/dev/null 2>&1; then
  printf '%s\n' "cargo is required to install lilaccaps from GitHub." >&2
  printf '%s\n' "Install Rust from https://rustup.rs, then rerun this installer." >&2
  exit 1
fi

install_build_prereqs

if [ -n "${LILACCAPS_INSTALL_ROOT:-}" ]; then
  install_root="$LILACCAPS_INSTALL_ROOT"
elif [ -n "${CARGO_HOME:-}" ]; then
  install_root="$CARGO_HOME"
elif [ -n "${HOME:-}" ]; then
  install_root="$HOME/.cargo"
else
  printf '%s\n' "HOME is not set; set LILACCAPS_INSTALL_ROOT explicitly." >&2
  exit 1
fi

case "$install_root" in
  /)
    printf '%s\n' "refusing to use the filesystem root as LILACCAPS_INSTALL_ROOT" >&2
    exit 1
    ;;
  /*) ;;
  *)
    printf '%s\n' "LILACCAPS_INSTALL_ROOT must be an absolute path: $install_root" >&2
    exit 1
    ;;
esac

set -- cargo install --root "$install_root" --git "$git_url" --locked --force
if [ -n "$tag" ]; then
  set -- "$@" --tag "$tag"
elif [ -n "$branch" ]; then
  set -- "$@" --branch "$branch"
fi
set -- "$@" "$package"

printf 'Installing %s globally from %s\n' "$package" "$git_url"
"$@"

installed_binary="$install_root/bin/lilaccaps"
if [ ! -x "$installed_binary" ]; then
  printf '%s\n' "cargo completed but the installed binary was not found at $installed_binary" >&2
  exit 1
fi

if [ "$fix" = "1" ]; then
  CARGO_HOME="$install_root" "$installed_binary" install --fix
else
  CARGO_HOME="$install_root" "$installed_binary" install
fi

printf 'lilaccaps is installed globally at %s.\n' "$installed_binary"
