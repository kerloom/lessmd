#!/bin/sh
# Install lessmd from GitHub Releases.
#
#   curl -fsSL https://raw.githubusercontent.com/kerloom/lessmd/master/install.sh | sh
#
# Environment:
#   LESSMD_VERSION   Pin a release (e.g. 0.2.3 or v0.2.3). Default: latest.
#   LESSMD_INSTALL   Install directory. Default: ~/.local/bin
#   LESSMD_REPO      GitHub repo slug. Default: kerloom/lessmd

set -eu

REPO="${LESSMD_REPO:-kerloom/lessmd}"
INSTALL_DIR="${LESSMD_INSTALL:-${HOME}/.local/bin}"
BIN_NAME="lessmd"

err() {
  printf 'lessmd install: %s\n' "$1" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || err "missing required command: $1"
}

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux)
      case "$arch" in
        x86_64 | amd64) target="x86_64-unknown-linux-gnu" ;;
        aarch64 | arm64) target="aarch64-unknown-linux-gnu" ;;
        *) err "unsupported Linux architecture: $arch" ;;
      esac
      archive_ext="tar.gz"
      ;;
    Darwin)
      case "$arch" in
        x86_64) target="x86_64-apple-darwin" ;;
        arm64 | aarch64) target="aarch64-apple-darwin" ;;
        *) err "unsupported macOS architecture: $arch" ;;
      esac
      archive_ext="tar.gz"
      ;;
    MINGW* | MSYS* | CYGWIN*)
      err "native Windows is not supported by install.sh; use install.ps1 instead"
      ;;
    *)
      err "unsupported operating system: $os"
      ;;
  esac
}

resolve_version() {
  if [ -n "${LESSMD_VERSION:-}" ]; then
    version="${LESSMD_VERSION#v}"
  else
    need_cmd curl
    version="$(
      curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
        | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\([^"]*\)".*/\1/p' \
        | head -n 1
    )"
    [ -n "$version" ] || err "could not resolve latest release version"
  fi
  tag="v${version#v}"
}

verify_checksum() {
  sums_file="$1"
  archive_name="$2"
  archive_path="$3"

  expected="$(
    grep " ${archive_name}\$" "$sums_file" | awk '{print $1}' | head -n 1
  )"
  [ -n "$expected" ] || err "checksum for ${archive_name} not found in SHA256SUMS"

  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$archive_path" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$archive_path" | awk '{print $1}')"
  else
    err "missing sha256sum or shasum (required to verify download)"
  fi

  [ "$actual" = "$expected" ] || err "checksum mismatch for ${archive_name}"
}

sign_macos_binary() {
  if [ "$(uname -s)" = "Darwin" ] && command -v codesign >/dev/null 2>&1; then
    codesign --force --sign - "$1"
  fi
}

main() {
  need_cmd curl
  need_cmd tar
  need_cmd install
  need_cmd mkdir

  detect_target
  resolve_version

  archive_name="${BIN_NAME}-${tag}-${target}.${archive_ext}"
  download_url="https://github.com/${REPO}/releases/download/${tag}/${archive_name}"
  sums_url="https://github.com/${REPO}/releases/download/${tag}/SHA256SUMS"

  tmpdir="${TMPDIR:-/tmp}/lessmd-install.$$"
  mkdir -p "$tmpdir"
  trap 'rm -rf "$tmpdir"' EXIT INT HUP TERM

  printf 'Installing lessmd %s (%s)\n' "$tag" "$target"

  curl -fsSL "$download_url" -o "${tmpdir}/${archive_name}"
  curl -fsSL "$sums_url" -o "${tmpdir}/SHA256SUMS"
  verify_checksum "${tmpdir}/SHA256SUMS" "$archive_name" "${tmpdir}/${archive_name}"

  tar -xzf "${tmpdir}/${archive_name}" -C "$tmpdir"
  [ -f "${tmpdir}/${BIN_NAME}" ] || err "archive did not contain ${BIN_NAME}"

  mkdir -p "$INSTALL_DIR"
  install -m 755 "${tmpdir}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
  sign_macos_binary "${INSTALL_DIR}/${BIN_NAME}"

  printf '\nlessmd %s installed to %s\n' "$tag" "${INSTALL_DIR}/${BIN_NAME}"

  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
      printf 'Add %s to your PATH, for example:\n' "$INSTALL_DIR"
      printf '  export PATH=%s:$PATH\n' "$INSTALL_DIR"
      ;;
  esac
}

main "$@"
