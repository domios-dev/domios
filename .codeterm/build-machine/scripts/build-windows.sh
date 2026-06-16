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
VER="${TAG#v}"
ARCH="${2:?arch: x86_64|aarch64}"
WIN_SSH="${WIN_SSH:-test@127.0.0.1}"
WIN_PORT="${WIN_PORT:-2222}"
case "$ARCH" in
  x86_64|x64)     TRIPLE=x86_64-pc-windows-msvc; LABEL=x64 ;;
  aarch64|arm64)  TRIPLE=aarch64-pc-windows-msvc; LABEL=arm64 ;;
  *) echo "bad arch $ARCH" >&2; exit 2 ;;
esac
# ConnectTimeout bounds ONLY connection establishment (not the established session), so it is safe
# on the long build SSH yet makes the readiness probe fail fast instead of hanging ~75s on a VM
# whose guest sshd isn't up yet — the stability-gate loop timing depends on a quick probe.
SSH="sshpass -e ssh -p $WIN_PORT -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 -o ServerAliveInterval=30 -o ServerAliveCountMax=1000 -o PreferredAuthentications=password -o PubkeyAuthentication=no -o NumberOfPasswordPrompts=1 $WIN_SSH"
SCP="sshpass -e scp -P $WIN_PORT -o StrictHostKeyChecking=accept-new -o PreferredAuthentications=password -o PubkeyAuthentication=no"

OUT="${OUT:-/tmp/codeterm-${TAG}-mirror}"; mkdir -p "$OUT"

# Bring the Windows VM up ON DEMAND — start it if it's not already answering SSH, then wait for
# it to boot. Lets the whole build machine spin the VM up only when a release actually arrives.
VM_DIR="$HOME/VMs/codeterm-win11-arm"
vm_ssh_ok() { $SSH "echo ok" 2>/dev/null | grep -q ok; }

# A fresh Windows cold boot answers SSH while still settling (services/updates finishing), then
# bounces sshd or reboots — which drops a build mid-flight. Don't trust the FIRST ok; require the
# VM to answer on several consecutive probes spaced apart, so the build only starts once stable.
wait_vm_stable() {
  local need=4 hits=0 i
  for i in $(seq 1 90); do
    if vm_ssh_ok; then
      hits=$((hits+1))
      [ "$hits" -ge "$need" ] && { echo "✅ Windows VM SSH stable (${hits} consecutive, ~$((i*5))s)"; return 0; }
    else
      hits=0
    fi
    sleep 5
  done
  return 1
}

if ! vm_ssh_ok; then
  echo "Windows VM not reachable on :$WIN_PORT — starting $VM_DIR/vm.sh run (on demand)..."
  ( cd "$VM_DIR" && nohup ./vm.sh run >/tmp/win-vm-boot.log 2>&1 & ) 2>/dev/null || true
  echo "waiting for Windows SSH to become STABLE (cold boot can take 1-3 min)..."
else
  echo "Windows VM answering — confirming it is stable before building..."
fi
wait_vm_stable || { echo "::error::Windows VM did not become stably reachable after wait" >&2; exit 1; }

$SCP "/tmp/codeterm-${TAG}.bundle" "${WIN_SSH}:codeterm-${TAG}.bundle"

run_win_build() {
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
Get-ChildItem "src-tauri\target\${TRIPLE}\release\bundle\nsis\*${VER}*-setup.exe" | Select Name,Length | Format-List
PS
}

# run_win_build ALWAYS exits non-zero: tauri.conf has an updater pubkey but no private key on the
# VM (by design — the minisign key never leaves the Mac, the .sig is signed there later), so the
# updater-signing step errors. That error is AFTER the NSIS .exe is bundled, so the installer is
# already produced. The .exe's existence — not run_win_build's exit code — is the success oracle.
#
# A dropped SSH ("Connection closed by remote host") DURING the cargo build is the real failure and
# is recoverable: the cargo target cache persists on the VM, so a retry resumes most of the work.
EXE_REMOTE="codeterm/src-tauri/target/${TRIPLE}/release/bundle/nsis/CodeTerm_${VER}_${LABEL}-setup.exe"
exe_present() {
  # SSH PowerShell sessions start in the user home dir; check the repo-relative path from there.
  $SSH "powershell -NoProfile -Command \"if (Test-Path \$HOME\\${EXE_REMOTE//\//\\}) { 'EXE_PRESENT' }\"" 2>/dev/null | grep -q EXE_PRESENT
}
build_ok=""
for attempt in 1 2; do
  echo "### windows build attempt $attempt/2 ($TRIPLE) — updater-signing exit-1 is expected, .exe presence is the oracle"
  run_win_build || true
  if exe_present; then build_ok=1; echo "✅ installer present after attempt $attempt"; break; fi
  echo "::warning::installer not present after attempt $attempt — re-checking VM stability before retry"
  wait_vm_stable || { echo "::error::VM unstable, cannot retry windows build" >&2; break; }
done
[ -n "$build_ok" ] || { echo "::error::windows installer not produced after retries" >&2; exit 1; }

$SCP "${WIN_SSH}:${EXE_REMOTE}" "$OUT/" \
  && echo "WINDOWS_${LABEL}_DONE $OUT/CodeTerm_${VER}_${LABEL}-setup.exe"
# Sign on the Mac afterwards:  pnpm tauri signer sign "$OUT/CodeTerm_${VER}_${LABEL}-setup.exe"
