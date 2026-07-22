#!/usr/bin/env bash

ct_load_release_signing_key() {
  if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
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
