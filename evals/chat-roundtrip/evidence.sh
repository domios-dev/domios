#!/usr/bin/env bash

roundtrip_evidence_dir() {
  case "$1-$2" in *[!a-zA-Z0-9_-]*) return 2 ;; esac
  local directory="$OUT_DIR/evidence/$1-$2"
  mkdir -p "$directory" || return
  printf '%s\n' "$directory"
}

capture_chat_roundtrip_evidence() {
  local provider="$1" tab="$2" stage="$3" directory result=0
  case "$stage" in ''|*[!a-zA-Z0-9_-]*) return 2 ;; esac
  directory="$(roundtrip_evidence_dir "$provider" "$tab")" || return
  dev_get "/api/tabs/$tab/text" > "$directory/$stage-screen.json" || result=1
  dev_get "/api/tabs/$tab/text?formatted=true" > "$directory/$stage-formatted-screen.json" || result=1
  dev_get /api/mux/state > "$directory/$stage-mux.json" || result=1
  dev_get "/api/tabs/$tab/status" > "$directory/$stage-status.json" || result=1
  return "$result"
}

picker_probe_trigger() {
  local provider="$1" tab="$2" directory result=0 body request
  directory="$(roundtrip_evidence_dir "$provider" "$tab")" || return
  capture_chat_roundtrip_evidence "$provider" "$tab" picker-before || result=1
  for request in command submit; do
    if [ "$request" = command ]; then body='{"text":"/model","raw":true}'
    else body='{"text":"\r","raw":true}'; fi
    printf '%s\n' "$body" > "$directory/picker-$request-request.json" || result=1
    dev_post "/api/tabs/$tab/send" "$body" > "$directory/picker-$request-response.json" || result=1
  done
  capture_chat_roundtrip_evidence "$provider" "$tab" picker-after-trigger || result=1
  return "$result"
}

capture_detached_send_evidence() {
  local provider="$1" tab="$2" stage="$3" send_id="$4" payload="$5" response="$6"
  local directory result=0
  case "$stage" in ''|*[!a-zA-Z0-9_-]*) return 2 ;; esac
  directory="$(roundtrip_evidence_dir "$provider" "$tab")" || return
  "$PY" -c 'import json,sys; print(json.dumps({"provider":sys.argv[1], "tab_id":sys.argv[2], "stage":sys.argv[3], "send_id":sys.argv[4], "payload":sys.argv[5]}))' \
    "$provider" "$tab" "$stage" "$send_id" "$payload" > "$directory/$stage-send.json" || result=1
  printf '%s\n' "$response" > "$directory/$stage-response.json" || result=1
  capture_chat_roundtrip_evidence "$provider" "$tab" "$stage" || result=1
  dev_get "/api/tabs/$tab/chat?since=0" > "$directory/$stage-own-transcript.json" || result=1
  # A ledger read reconciles; retain it only after the original oracle sampled its result.
  if [ "$stage" = detached-final ]; then
    dev_get "/api/tabs/$tab/chat-send" > "$directory/$stage-ledger-after-oracle.json" || result=1
  fi
  return "$result"
}
