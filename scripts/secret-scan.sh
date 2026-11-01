#!/usr/bin/env bash
# Usage: secret-scan.sh BASE_SHA
# Scans BASE_SHA..HEAD plus the working tree; a missing base is refused.
# Findings are always redacted; the script never prints a secret value.
set -euo pipefail

GITLEAKS_VERSION="8.30.1"
SHA256_LINUX_X64="551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb"
SHA256_LINUX_ARM64="e4a487ee7ccd7d3a7f7ec08657610aa3606637dab924210b3aee62570fb4b080"

cd "$(git rev-parse --show-toplevel)"

install_gitleaks() {
  local arch sha dir
  case "$(uname -m)" in
    x86_64) arch=x64; sha="$SHA256_LINUX_X64" ;;
    aarch64 | arm64) arch=arm64; sha="$SHA256_LINUX_ARM64" ;;
    *) echo "secret-scan: install gitleaks ${GITLEAKS_VERSION} manually (unsupported arch $(uname -m))" >&2; exit 2 ;;
  esac
  dir="${RUNNER_TEMP:-$(mktemp -d)}/gitleaks-${GITLEAKS_VERSION}"
  mkdir -p "$dir"
  curl -fsSL -o "$dir/gitleaks.tar.gz" \
    "https://github.com/gitleaks/gitleaks/releases/download/v${GITLEAKS_VERSION}/gitleaks_${GITLEAKS_VERSION}_linux_${arch}.tar.gz"
  echo "${sha}  $dir/gitleaks.tar.gz" | sha256sum -c - >/dev/null
  tar -xzf "$dir/gitleaks.tar.gz" -C "$dir" gitleaks
  PATH="$dir:$PATH"
  export PATH
}

base="${1:-}"
common=(--config .gitleaks.toml --redact --no-banner --verbose)

if [[ -z "$base" || "$base" =~ ^0+$ ]]; then
  echo "secret-scan: a nonzero base commit is required" >&2
  exit 2
else
  git cat-file -e "${base}^{commit}" 2>/dev/null || { echo "secret-scan: base ${base} not fetched; use fetch-depth: 0" >&2; exit 2; }
  log_opts=("--log-opts=${base}..HEAD")
fi

command -v gitleaks >/dev/null || install_gitleaks

gitleaks git "${common[@]}" "${log_opts[@]}" .
gitleaks dir "${common[@]}" .
