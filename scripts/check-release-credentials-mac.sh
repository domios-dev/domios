#!/usr/bin/env bash
# Caller: CI release-credentials.yml, release-macos-build.yml; scripts tests/release-job-graph.test.mjs; skill github-release
set -uo pipefail
rc=0
: "${RELEASE_KEYCHAIN:?Import the job signing keychain first}"
: "${APPLE_SIGNING_IDENTITY:?Provide APPLE_SIGNING_IDENTITY}"
: "${TAURI_SIGNING_PRIVATE_KEY_PATH:?Prepare the job updater key first}"
: "${RELEASE_SIGNING_DIR:?Prepare job signing material first}"
python3 scripts/release-signing-material.py check-identity \
  && echo "Developer ID present and valid in job keychain" || rc=1
python3 - <<'PY'
import base64, os
from pathlib import Path
key = Path(os.environ['TAURI_SIGNING_PRIVATE_KEY_PATH'])
assert key.stat().st_mode & 0o777 == 0o600
assert base64.b64decode(key.read_text()).startswith(b'untrusted comment:')
PY
if [ "$?" = 0 ]; then echo "Updater key decodes"; else echo "::error::Invalid updater key"; rc=1; fi
gh auth status >/dev/null 2>&1 || { echo "::error::gh not authenticated"; rc=1; }
log="$RELEASE_SIGNING_DIR/notary-preflight.log"
if xcrun notarytool history --keychain-profile "${APPLE_KEYCHAIN_PROFILE:?}" --keychain "$RELEASE_KEYCHAIN" >"$log" 2>&1; then
  echo "Apple notary credentials accepted"
else
  echo "::error::Apple notary check failed; check repository credentials and Developer agreement"
  rc=1
fi
rm -f "$log"
exit "$rc"
