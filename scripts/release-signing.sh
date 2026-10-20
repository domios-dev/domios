#!/usr/bin/env bash
# Caller: build.sh dev|dev-prebuilt|kill|e2e|e2e:llm; CI build-gateway.yml; scripts check-build-gateway-policy.sh, check-release-signing.sh, dev-prep.mjs

ct_load_release_signing_key() {
  if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
    return 0
  fi

  if [ -n "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]; then
    if [ ! -s "$TAURI_SIGNING_PRIVATE_KEY_PATH" ]; then
      echo "ERROR: job updater signing key is missing; refusing to compile an unsigned updater." >&2
      return 2
    fi
    export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
    return 0
  fi

  local key_file="${CODETERM_TAURI_SIGNING_KEY_FILE:-$HOME/.tauri/codeterm.key}"
  if [ ! -s "$key_file" ]; then
    echo "ERROR: updater signing key is missing: $key_file" >&2
    echo "Restore it before the release build; refusing to compile an unsigned updater." >&2
    return 2
  fi

  TAURI_SIGNING_PRIVATE_KEY="$(tr -d '\r\n' < "$key_file")"
  export TAURI_SIGNING_PRIVATE_KEY
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
}
