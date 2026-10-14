#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
eval "$(sed -n '/^ct_msvc_build_tools_base()/,/^}/p' "$ROOT/build.sh")"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
preferred="$WORK/default"
discovered="$WORK/other edition"
mkdir -p "$discovered/VC/Tools/MSVC"
export FIXTURE_DISCOVERED="$discovered" FIXTURE_QUERY_LOG="$WORK/query"
cat > "$WORK/vswhere" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
[ "$*" = '-latest -products * -version [17.0,18.0) -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath' ]
printf 'query\n' >> "$FIXTURE_QUERY_LOG"
printf '%s\r\n' "$FIXTURE_DISCOVERED"
FAKE
chmod +x "$WORK/vswhere"
mkdir -p "$preferred/VC/Tools/MSVC"
[ "$(ct_msvc_build_tools_base "$preferred" "$WORK/vswhere")" = "$preferred" ]
[ ! -e "$WORK/query" ]
printf 'PASS preferred tools avoid discovery\n'
rmdir "$preferred/VC/Tools/MSVC"
[ "$(ct_msvc_build_tools_base "$preferred" "$WORK/vswhere")" = "$discovered" ]
[ -e "$WORK/query" ]
printf 'PASS relocated VS2022 C++ tools are discovered with CRLF normalization\n'
export FIXTURE_DISCOVERED="$WORK/invalid"
[ "$(ct_msvc_build_tools_base "$preferred" "$WORK/vswhere")" = "$preferred" ]
[ "$(ct_msvc_build_tools_base "$preferred" "$WORK/missing")" = "$preferred" ]
printf 'PASS missing and invalid discovery preserve the explicit setup refusal\n'
