#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

sha="$(git rev-parse HEAD)"
default_output="$(
  CODETERM_PANE_ID=pane-default \
    CODETERM_TEST_PRINT_LOCAL_INSTALL_CONTINUATION=1 \
    ./build.sh install-local
)"
grep -Fqx "enabled=1" <<< "$default_output"
grep -Fqx "tab=pane-default" <<< "$default_output"
grep -Fqx "prompt=Local CodeTerm install completed for build $sha. Verify the production health endpoint reports this SHA, then continue the work that requested the reinstall." <<< "$default_output"

custom_output="$(
  CODETERM_TEST_PRINT_LOCAL_INSTALL_CONTINUATION=1 \
    ./build.sh install-local \
      --continue-tab pane-custom \
      --continue-prompt "resume custom work"
)"
grep -Fqx "enabled=1" <<< "$custom_output"
grep -Fqx "tab=pane-custom" <<< "$custom_output"
grep -Fqx "prompt=resume custom work" <<< "$custom_output"

disabled_output="$(
  CODETERM_PANE_ID=pane-default \
    CODETERM_TEST_PRINT_LOCAL_INSTALL_CONTINUATION=1 \
    ./build.sh install-local --no-continue
)"
grep -Fqx "enabled=0" <<< "$disabled_output"
grep -Fqx "tab=" <<< "$disabled_output"
grep -Fqx "prompt=" <<< "$disabled_output"

if CODETERM_PANE_ID= CODETERM_TEST_PRINT_LOCAL_INSTALL_CONTINUATION=1 \
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

echo "local install continuation checks passed"
