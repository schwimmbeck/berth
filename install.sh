#!/usr/bin/env bash
set -euo pipefail

REPO="${BERTH_REPO:-berth-dev/berth}"
VERSION="${BERTH_VERSION:-}"
BIN_NAME="berth"

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

detect_os() {
  case "$(uname -s)" in
    Linux) echo "linux" ;;
    Darwin) echo "macos" ;;
    *)
      echo "Unsupported OS: $(uname -s)" >&2
      exit 1
      ;;
  esac
}

resolve_version() {
  if [[ -n "${VERSION}" ]]; then
    echo "${VERSION}"
    return
  fi

  need_cmd curl
  curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' \
    | head -n1
}

download_asset() {
  local url="$1"
  local out="$2"

  if command -v curl >/dev/null 2>&1; then
    curl -fL "$url" -o "$out"
  elif command -v wget >/dev/null 2>&1; then
    wget -O "$out" "$url"
  else
    echo "Need curl or wget to download releases." >&2
    exit 1
  fi
}

install_bin() {
  local src="$1"
  local install_dir="${BERTH_INSTALL_DIR:-}"

  if [[ -z "${install_dir}" ]]; then
    if [[ -w "/usr/local/bin" ]]; then
      install_dir="/usr/local/bin"
    else
      install_dir="${HOME}/.local/bin"
    fi
  fi

  mkdir -p "${install_dir}"
  install -m 0755 "${src}" "${install_dir}/${BIN_NAME}"
  echo "Installed ${BIN_NAME} to ${install_dir}/${BIN_NAME}"

  case ":${PATH}:" in
    *":${install_dir}:"*) ;;
    *)
      echo "Add ${install_dir} to PATH if needed."
      ;;
  esac
}

main() {
  local os tag asset url tmp
  os="$(detect_os)"
  tag="$(resolve_version)"
  if [[ -z "${tag}" ]]; then
    echo "Could not resolve release version." >&2
    exit 1
  fi

  asset="${BIN_NAME}-${tag}-${os}"
  url="https://github.com/${REPO}/releases/download/${tag}/${asset}"
  tmp="$(mktemp)"
  trap 'rm -f "${tmp}"' EXIT

  echo "Downloading ${url}"
  download_asset "${url}" "${tmp}"
  install_bin "${tmp}"
  echo "${BIN_NAME} ${tag} installation complete."
}

main "$@"
