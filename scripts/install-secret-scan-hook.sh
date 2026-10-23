#!/usr/bin/env bash
# Opt-in: installs a pre-commit hook that scans staged changes with gitleaks.
# Writes only this checkout's pre-commit hook; core.hooksPath is left alone. Remove with --uninstall.
set -euo pipefail

hook="$(git rev-parse --path-format=absolute --git-path hooks/pre-commit)"
marker="# domios-secret-scan"

if [[ "${1:-}" == "--uninstall" ]]; then
  if [[ -f "$hook" ]] && grep -q "$marker" "$hook"; then rm "$hook"; echo "removed $hook"; else echo "no secret-scan hook installed"; fi
  exit 0
fi

if [[ -e "$hook" ]] && ! grep -q "$marker" "$hook"; then
  echo "refusing to overwrite existing $hook" >&2
  exit 1
fi

mkdir -p "$(dirname "$hook")"
cat >"$hook" <<'HOOK'
#!/usr/bin/env bash
# domios-secret-scan
if ! command -v gitleaks >/dev/null; then
  echo "secret-scan: gitleaks not installed, skipping (CI still enforces)" >&2
  exit 0
fi
exec gitleaks git --pre-commit --staged --redact --no-banner --verbose --config "$(git rev-parse --show-toplevel)/.gitleaks.toml"
HOOK
chmod +x "$hook"
echo "installed $hook"
