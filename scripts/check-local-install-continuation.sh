#!/usr/bin/env bash
# Caller: build.sh script-guards
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

fake_bin="$(mktemp -d)"
trap 'rm -rf "$fake_bin"' EXIT
printf '#!/usr/bin/env bash\n[ "$1 $2" != "pad path" ] || printf %%s %q\n' "$fake_bin" > "$fake_bin/codeterm"
chmod +x "$fake_bin/codeterm"

# The live CLI and inherited route selectors would deliver this guard's receipts to the owner's tabs.
isolated_build() (
  for name in $(compgen -e); do
    case "$name" in CODETERM_*|BUILD_NOTIFY_*|DEV_NOTIFY_*) unset "$name" ;; esac
  done
  PATH="$fake_bin:$PATH" env "$@"
)

[ "$(isolated_build bash -c 'command -v codeterm')" = "$fake_bin/codeterm" ]

sha="$(git rev-parse HEAD)"
default_output="$(
  isolated_build CODETERM_TAB_ID=tab-default \
    CODETERM_TEST_PRINT_LOCAL_INSTALL_CONTINUATION=1 \
    ./build.sh install-local
)"
grep -Fqx "enabled=1" <<< "$default_output"
grep -Fqx "tab=tab-default" <<< "$default_output"
grep -Fqx "prompt=Local CodeTerm install completed for build $sha. Verify the production health endpoint reports this SHA, then continue the work that requested the reinstall." <<< "$default_output"

custom_output="$(
  isolated_build CODETERM_TEST_PRINT_LOCAL_INSTALL_CONTINUATION=1 \
    ./build.sh install-local \
      --continue-tab tab-custom \
      --continue-prompt "resume custom work"
)"
grep -Fqx "enabled=1" <<< "$custom_output"
grep -Fqx "tab=tab-custom" <<< "$custom_output"
grep -Fqx "prompt=resume custom work" <<< "$custom_output"

disabled_output="$(
  isolated_build CODETERM_TAB_ID=tab-default \
    CODETERM_TEST_PRINT_LOCAL_INSTALL_CONTINUATION=1 \
    ./build.sh install-local --no-continue
)"
grep -Fqx "enabled=0" <<< "$disabled_output"
grep -Fqx "tab=" <<< "$disabled_output"
grep -Fqx "prompt=" <<< "$disabled_output"

if isolated_build CODETERM_TEST_PRINT_LOCAL_INSTALL_CONTINUATION=1 \
  ./build.sh install-local --continue-prompt "missing target" >/dev/null 2>&1; then
  echo "continuation prompt without a target must fail" >&2
  exit 1
fi

for variable in \
  CODETERM_INSTANCE \
  CODETERM_VITE_PORT \
  CODETERM_DEV_IDENTIFIER \
  CODETERM_CDP_PORT \
  CARGO_TARGET_DIR \
  CARGO_INCREMENTAL \
  RUSTC_WRAPPER \
  SCCACHE_BASEDIRS; do
  grep -Fq -- "-u $variable" build.sh
done

grep -Fq 'FRESH_CLI="$(ls src-tauri/bundled-cli/domios*.exe' build.sh
grep -Fq '"$FRESH_CLI" "${INSTALL_ARGS[@]}"' build.sh

gateway_release_line="$(grep -n '^[[:space:]]*complete_build_gateway_before_handoff$' build.sh | tail -1 | cut -d: -f1)"
installer_handoff_line="$(grep -n '^[[:space:]]*"$FRESH_CLI" "${INSTALL_ARGS\[@\]}"' build.sh | tail -1 | cut -d: -f1)"
[ -n "$gateway_release_line" ]
[ -n "$installer_handoff_line" ]
[ "$gateway_release_line" -lt "$installer_handoff_line" ] || {
  echo "local install must release the build gateway before installer handoff" >&2
  exit 1
}

echo "local install continuation checks passed"
