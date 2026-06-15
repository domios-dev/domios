#!/usr/bin/env bash
# Windows build step, driven from the Mac over SSH into the Windows builder VM/peer.
# The VM has Rust(+MSVC), node/pnpm, git, LLVM. We push a git bundle (auth-free), build
# the NSIS installer for the requested arch, and pull the .exe back. Updater signing is
# done LATER on the Mac (minisign key never leaves the Mac).
#
# Env:
#   WIN_SSH        ssh target, e.g. "test@127.0.0.1"   (default: test@127.0.0.1)
#   WIN_PORT       ssh port (default: 2222)
#   SSHPASS        password for the VM (used via sshpass -e); or set up key auth
# Usage:  build-windows.sh <tag> <arch>   arch: x86_64 | aarch64
set -euo pipefail
TAG="${1:?usage: build-windows.sh <tag> <arch>}"
ARCH="${2:?arch: x86_64|aarch64}"
WIN_SSH="${WIN_SSH:-test@127.0.0.1}"
WIN_PORT="${WIN_PORT:-2222}"
case "$ARCH" in
  x86_64|x64)     TRIPLE=x86_64-pc-windows-msvc; LABEL=x64 ;;
  aarch64|arm64)  TRIPLE=aarch64-pc-windows-msvc; LABEL=arm64 ;;
  *) echo "bad arch $ARCH" >&2; exit 2 ;;
esac
SSH="sshpass -e ssh -p $WIN_PORT -o StrictHostKeyChecking=accept-new -o ServerAliveInterval=30 -o ServerAliveCountMax=1000 -o PreferredAuthentications=password -o PubkeyAuthentication=no -o NumberOfPasswordPrompts=1 $WIN_SSH"
SCP="sshpass -e scp -P $WIN_PORT -o StrictHostKeyChecking=accept-new -o PreferredAuthentications=password -o PubkeyAuthentication=no"

OUT="${OUT:-/tmp/codeterm-${TAG}-mirror}"; mkdir -p "$OUT"
$SCP "/tmp/codeterm-${TAG}.bundle" "${WIN_SSH}:codeterm-${TAG}.bundle"

$SSH "powershell -NoProfile -Command -" <<PS
\$ErrorActionPreference="Continue"
\$env:PATH = "C:\Program Files\nodejs;C:\Users\test\.cargo\bin;C:\Program Files\Git\cmd;C:\Program Files\LLVM\bin;" + \$env:PATH
\$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
\$git = "C:\Program Files\Git\cmd\git.exe"
Set-Location "\$env:USERPROFILE\codeterm"
& \$git fetch "\$env:USERPROFILE\codeterm-${TAG}.bundle" "refs/tags/${TAG}:refs/tags/${TAG}" 2>&1 | Out-String
& \$git checkout -f "${TAG}" 2>&1 | Out-String
& "C:\Program Files\nodejs\pnpm.cmd" install 2>&1 | Out-String
# pre-create externalBin/resource stubs (breaks tauri-build chicken-and-egg for cross targets)
New-Item -ItemType File -Force src-tauri\bundled-embed\codeterm-embed-${TRIPLE}.exe | Out-Null
New-Item -ItemType File -Force src-tauri\bundled-cli\codeterm.exe | Out-Null
\$env:CARGO_BUILD_TARGET="${TRIPLE}"
& "C:\Program Files\nodejs\pnpm.cmd" tauri build --target ${TRIPLE} 2>&1 | Out-String
Get-ChildItem "src-tauri\target\${TRIPLE}\release\bundle\nsis\*${TAG}*-setup.exe" | Select Name,Length | Format-List
PS

$SCP "${WIN_SSH}:codeterm/src-tauri/target/${TRIPLE}/release/bundle/nsis/CodeTerm_${TAG}_${LABEL}-setup.exe" "$OUT/" \
  && echo "WINDOWS_${LABEL}_DONE $OUT/CodeTerm_${TAG}_${LABEL}-setup.exe"
# Sign on the Mac afterwards:  pnpm tauri signer sign "$OUT/CodeTerm_${TAG}_${LABEL}-setup.exe"
