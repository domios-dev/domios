#!/usr/bin/env bash
# One-time: install the Mac build key on the Windows VM for key-based SSH auth.
# Run this once; the key persists on the VM disk.
#
# Usage: setup-win-ssh-key.sh [pubkey-path]
#   pubkey-path defaults to ~/.ssh/codeterm-win-build.pub
set -euo pipefail

WIN_SSH="${WIN_SSH:-test@127.0.0.1}"
WIN_PORT="${WIN_PORT:-2222}"
SSHPASS="${SSHPASS:-test}"
PUBKEY="${1:-$HOME/.ssh/codeterm-win-build.pub}"

if [ ! -f "$PUBKEY" ]; then
  echo "::error::pubkey not found: $PUBKEY" >&2
  echo "Generate it with: ssh-keygen -t ed25519 -f ~/.ssh/codeterm-win-build -N '' -C codeterm-build" >&2
  exit 1
fi

PUB="$(cat "$PUBKEY")"
SSH_OPTS=(-p "$WIN_PORT" -o StrictHostKeyChecking=accept-new -o ConnectTimeout=15
          -o PreferredAuthentications=password -o PubkeyAuthentication=no
          -o NumberOfPasswordPrompts=1)

echo "Installing key on Windows VM ($WIN_SSH:$WIN_PORT)..."

# Windows OpenSSH: admin users require administrators_authorized_keys (not per-user authorized_keys).
# Permissions must be locked to SYSTEM+Administrators only — any extra ACE silently disables auth.
sshpass -p "$SSHPASS" ssh "${SSH_OPTS[@]}" "$WIN_SSH" "powershell -NoProfile -Command -" <<PS
\$ErrorActionPreference = 'Stop'
\$keyFile = 'C:\\ProgramData\\ssh\\administrators_authorized_keys'
\$pubKey  = '$PUB'

# Write the key (overwrite so this script is idempotent)
New-Item -Force -ItemType File -Path \$keyFile | Out-Null
Set-Content -Path \$keyFile -Value \$pubKey -NoNewline -Encoding UTF8

# Strip inherited ACEs; grant only SYSTEM + Administrators — ANYTHING else breaks Windows OpenSSH key auth.
icacls \$keyFile /inheritance:r /grant 'SYSTEM:(F)' /grant 'Administrators:(F)' | Out-Null

\$check = Get-Content \$keyFile
Write-Host "administrators_authorized_keys written: \$(\$check.Length) chars"
\$acl = (Get-Acl \$keyFile).Access | ForEach-Object { "\$(\$_.IdentityReference) \$(\$_.FileSystemRights)" }
Write-Host "ACL: \$(\$acl -join ', ')"
Write-Host "Done. Test with: ssh -i ~/.ssh/codeterm-win-build -p $WIN_PORT $WIN_SSH echo ok"
PS
