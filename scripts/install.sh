#!/bin/sh
set -eu

repository="beejmaxx/polymarket-mcp-rs"
install_dir="${POLYMARKET_MCP_INSTALL_DIR:-${HOME}/.local/bin}"
requested_version="${POLYMARKET_MCP_VERSION:-latest}"

case "$(uname -s)" in
  Linux) platform="linux" ;;
  Darwin) platform="macos" ;;
  *) echo "unsupported operating system: $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  x86_64|amd64) architecture="x86_64" ;;
  arm64|aarch64) architecture="aarch64" ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

archive="polymarket-mcp-rs-${platform}-${architecture}.tar.gz"
if [ "$requested_version" = "latest" ]; then
  release_url="https://github.com/${repository}/releases/latest/download"
else
  case "$requested_version" in
    v*) tag="$requested_version" ;;
    *) tag="v${requested_version}" ;;
  esac
  release_url="https://github.com/${repository}/releases/download/${tag}"
fi

temporary_dir="$(mktemp -d)"
trap 'rm -rf "$temporary_dir"' EXIT INT TERM

echo "Downloading ${archive}..."
curl --fail --location --silent --show-error \
  "${release_url}/${archive}" --output "${temporary_dir}/${archive}"
curl --fail --location --silent --show-error \
  "${release_url}/${archive}.sha256" --output "${temporary_dir}/${archive}.sha256"

expected="$(awk '{print $1}' "${temporary_dir}/${archive}.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "${temporary_dir}/${archive}" | awk '{print $1}')"
else
  actual="$(shasum -a 256 "${temporary_dir}/${archive}" | awk '{print $1}')"
fi
if [ "$expected" != "$actual" ]; then
  echo "checksum verification failed for ${archive}" >&2
  exit 1
fi

tar -xzf "${temporary_dir}/${archive}" -C "$temporary_dir"
mkdir -p "$install_dir"
install -m 0755 "${temporary_dir}/polymarket-mcp-rs" "${install_dir}/polymarket-mcp-rs"
"${install_dir}/polymarket-mcp-rs" --version
echo "Installed to ${install_dir}/polymarket-mcp-rs"
case ":${PATH}:" in
  *":${install_dir}:"*) ;;
  *) echo "Add ${install_dir} to PATH, or use the absolute path in your MCP client." ;;
esac
