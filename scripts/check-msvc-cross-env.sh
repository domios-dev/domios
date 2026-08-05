#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ "${OSTYPE:-}" == darwin* || "${OSTYPE:-}" == linux* ]]; then
  echo "SKIP msvc cross-target link environment (Windows-only build environment)"
  exit 0
fi

probe() {
  local target="$1" var="$2"
  CARGO_BUILD_TARGET="$target" bash -c 'source ./build.sh env >/dev/null 2>&1; printf "%s" "${!1}"' _ "$var"
}

assert_lib_arch() {
  local label="$1" target="$2" want="$3" lib entry leaf count=0
  lib="$(probe "$target" LIB)"
  [ -n "$lib" ] || { echo "FAIL $label: build.sh env exported no LIB" >&2; exit 1; }
  local saved_ifs="$IFS"
  IFS=';'
  for entry in $lib; do
    IFS="$saved_ifs"
    [ -n "$entry" ] || continue
    leaf="${entry##*/}"
    leaf="${leaf##*\\}"
    if [ "$leaf" != "$want" ]; then
      echo "FAIL $label: LIB entry '$entry' is not $want (LIB=$lib)" >&2
      exit 1
    fi
    count=$((count + 1))
    IFS=';'
  done
  IFS="$saved_ifs"
  [ "$count" -ge 3 ] || { echo "FAIL $label: LIB needs VC lib + SDK ucrt + SDK um, got '$lib'" >&2; exit 1; }
  echo "PASS $label: LIB is $want ($count entries)"
}

assert_lib_arch "host build" "" "x64"
assert_lib_arch "aarch64 cross build" "aarch64-pc-windows-msvc" "arm64"

cross_cc="$(probe aarch64-pc-windows-msvc CC_aarch64_pc_windows_msvc)"
case "$cross_cc" in
  */bin/Hostx64/arm64/cl.exe) echo "PASS aarch64 cross build: cc crate targets $cross_cc" ;;
  *) echo "FAIL aarch64 cross build: CC_aarch64_pc_windows_msvc is '$cross_cc', expected Hostx64/arm64/cl.exe" >&2; exit 1 ;;
esac

cross_ar="$(probe aarch64-pc-windows-msvc AR_aarch64_pc_windows_msvc)"
case "$cross_ar" in
  */bin/Hostx64/arm64/lib.exe) echo "PASS aarch64 cross build: archiver is $cross_ar" ;;
  *) echo "FAIL aarch64 cross build: AR_aarch64_pc_windows_msvc is '$cross_ar', expected Hostx64/arm64/lib.exe" >&2; exit 1 ;;
esac

host_cc="$(probe aarch64-pc-windows-msvc CC)"
case "$host_cc" in
  */bin/Hostx64/x64/cl.exe) echo "PASS aarch64 cross build: build scripts keep the host compiler" ;;
  *) echo "FAIL aarch64 cross build: CC is '$host_cc', build scripts must compile for the host" >&2; exit 1 ;;
esac

GUARD_LOG="$(mktemp)"
trap 'rm -f "$GUARD_LOG"' EXIT
if bash -c \
  'source ./build.sh env >/dev/null 2>&1; LIB="C:/vc/lib/x64;C:/sdk/ucrt/arm64" ct_assert_msvc_lib_arch arm64' \
  2> "$GUARD_LOG"; then
  echo "FAIL guard: a host-architecture LIB entry was accepted for an arm64 link" >&2
  exit 1
fi
grep -q "refusing to link arm64 objects against host libraries" "$GUARD_LOG"
echo "PASS guard: host-architecture LIB entries are rejected before an arm64 link"

echo "PASS msvc cross-target link environment follows CARGO_BUILD_TARGET"
