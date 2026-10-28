# Extend the shared bench boundary with the checkout's actual CLI name and env contract.
core_journey_build_field() {
  node "$HERE/lib/build-identity.mjs" --checkout "$1" --field "$2"
}

core_journey_select_build_identity() {
  local checkout="$1"
  export CORE_JOURNEY_CLI_PATH="$(core_journey_build_field "$checkout" executable)"
  export CORE_JOURNEY_CLI_NAME="$(core_journey_build_field "$checkout" binaryName)"
  export CORE_JOURNEY_CLI_INSTANCE_ENV="$(core_journey_build_field "$checkout" cli.instance)"
  export CORE_JOURNEY_CLI_PORT_ENV="$(core_journey_build_field "$checkout" cli.port)"
  export CORE_JOURNEY_BUILD_INSTANCE_ENV="$(core_journey_build_field "$checkout" development.instance)"
  export CORE_JOURNEY_BUILD_PORT_ENV="$(core_journey_build_field "$checkout" development.port)"
  export CORE_JOURNEY_REQUEST_INSTANCE_ENV="$(core_journey_build_field "$checkout" request.instance)"
  export CORE_JOURNEY_REQUEST_PORT_ENV="$(core_journey_build_field "$checkout" request.port)"
}

core_journey_product_env_for() {
  local instance_env="$1" suffix="$2" prefix="${1%_INSTANCE}"
  [ -n "$prefix" ] && [ "$prefix" != "$instance_env" ] || return 7
  printf '%s_%s' "$prefix" "$suffix"
}

core_journey_product_env() { core_journey_product_env_for "$CORE_JOURNEY_CLI_INSTANCE_ENV" "$1"; }
if [ -z "${CORE_JOURNEY_CLI_PATH:-}" ]; then
  core_journey_select_build_identity "$ROOT" || return 7
fi

dev_bootstrap_cli() { printf '%s\n' "$CORE_JOURNEY_CLI_PATH"; }

dev_resolve_identity() {
  local requested_port output parsed cli_instance_env cli_port_env data_dir host port socket instance suffix
  instance="${EVAL_LIB_REQUESTED_INSTANCE:-}"
  requested_port="${EVAL_LIB_REQUESTED_API_PORT:-}"
  [ -n "$instance" ] || { echo 'ERROR [dev-resolution]: named instance is required.' >&2; return 7; }
  [[ "$requested_port" =~ ^[0-9]{1,5}$ ]] || { echo 'ERROR [dev-resolution]: named instance needs a numeric API port.' >&2; return 7; }
  requested_port=$((10#$requested_port))
  [ "$requested_port" -ge 1 ] && [ "$requested_port" -le 65535 ] \
    && [ "$requested_port" != 7685 ] && [ "$requested_port" != 7686 ] || {
      echo 'ERROR [dev-resolution]: refusing production, shared DEV or invalid port.' >&2; return 7;
    }
  [ -x "$CORE_JOURNEY_CLI_PATH" ] || { echo "ERROR [dev-resolution]: expected checkout CLI is unavailable: $CORE_JOURNEY_CLI_PATH" >&2; return 7; }
  cli_instance_env="$CORE_JOURNEY_CLI_INSTANCE_ENV"
  cli_port_env="$CORE_JOURNEY_CLI_PORT_ENV"
  local -a unset_identity=()
  for suffix in INSTANCE API_PORT API_URL API_TOKEN DATA_DIR SOCKET_PATH TAB_ID PARENT_TAB_ID; do
    unset_identity+=(-u "$(core_journey_product_env "$suffix")")
  done
  if ! output="$(env "${unset_identity[@]}" "$cli_instance_env=$instance" "$cli_port_env=$requested_port" \
    "$CORE_JOURNEY_CLI_PATH" identify --json)"; then
    echo 'ERROR [dev-resolution]: checkout CLI could not identify its own named instance.' >&2; return 7;
  fi
  if ! parsed="$(printf '%s\n' "$output" | python3 -c '
import json, sys
connection = json.load(sys.stdin)["connection"]
print("\t".join([connection["data_dir"], connection["host"], str(connection["port"]),
                  connection["socket_path"], (connection.get("instance") or "").strip()]))
')"; then
    echo 'ERROR [dev-resolution]: checkout CLI identity is malformed.' >&2; return 7;
  fi
  IFS=$'\t' read -r data_dir host port socket instance <<<"$parsed"
  [ "$instance" = "$EVAL_LIB_REQUESTED_INSTANCE" ] && [ "$port" = "$requested_port" ] \
    && { [ "$host" = 127.0.0.1 ] || [ "$host" = localhost ]; } || {
    echo 'ERROR [dev-resolution]: checkout CLI selected a different instance, port or non-loopback host.' >&2; return 7;
  }
  EVAL_LIB_RESOLVED_DATA_DIR="$(dev_shell_path "$data_dir")"
  EVAL_LIB_DEV_HOST="$host"
  EVAL_LIB_DEV_API_PORT="$port"
  EVAL_LIB_RESOLVED_SOCKET_PATH="$(dev_shell_path "$socket")"
  EVAL_LIB_RESOLVED_INSTANCE="$instance"
  EVAL_LIB_DEV_VITE_PORT="$(dev_vite_port_from_instance "$instance")"
  EVAL_LIB_DEV_CDP_PORT="$(dev_cdp_port_from_instance "$instance")"
  EVAL_LIB_TARGET_DATA_DIR="$EVAL_LIB_RESOLVED_DATA_DIR"
  EVAL_LIB_TARGET_API_PORT="$EVAL_LIB_DEV_API_PORT"
  EVAL_LIB_TARGET_VITE_PORT="$EVAL_LIB_DEV_VITE_PORT"
  EVAL_LIB_TARGET_CDP_PORT="$EVAL_LIB_DEV_CDP_PORT"
  EVAL_LIB_TARGET_SOCKET_PATH="$EVAL_LIB_RESOLVED_SOCKET_PATH"
  EVAL_LIB_TARGET_INSTANCE="$EVAL_LIB_RESOLVED_INSTANCE"
  EVAL_LIB_TARGET_IDENTITY_VERIFIED=1
}

dev_api_url() { printf 'http://%s:%s' "$EVAL_LIB_DEV_HOST" "$EVAL_LIB_DEV_API_PORT"; }

dev_assert_target() {
  local health
  health="$(curl -sS --max-time 5 "$(dev_api_url)/api/health")" || { echo 'ERROR [dev-target]: named instance health is unavailable.' >&2; return 7; }
  printf '%s\n' "$health" | python3 -c '
import json, re, sys
health = json.load(sys.stdin)
instance = health.get("instance", {})
name, port = sys.argv[1:]
valid = health.get("status") == "ok" and re.fullmatch("[a-f0-9]{40}", health.get("build_sha", "")) is not None
valid = valid and (instance.get("name") or "").strip() == name and str(instance.get("api_port", "")) == port
sys.exit(0 if valid else 7)
' "$EVAL_LIB_TARGET_INSTANCE" "$EVAL_LIB_TARGET_API_PORT" || { echo 'ERROR [dev-target]: health identity does not match the named instance.' >&2; return 7; }
}

dev_run() {
  local caller_tab_id=""
  if [ "${1:-}" = --as-tab ]; then
    caller_tab_id="${2:-}"; [ -n "$caller_tab_id" ] || return 7; shift 2
  fi
  dev_resolve || return 7
  local product_instance_env product_port_env product_api_url_env product_api_token_env product_data_dir_env product_socket_env suffix
  product_instance_env="$(core_journey_product_env INSTANCE)"
  product_port_env="$(core_journey_product_env API_PORT)"
  product_api_url_env="$(core_journey_product_env API_URL)"
  product_api_token_env="$(core_journey_product_env API_TOKEN)"
  product_data_dir_env="$(core_journey_product_env DATA_DIR)"
  product_socket_env="$(core_journey_product_env SOCKET_PATH)"
  local -a vars=(
    "$CORE_JOURNEY_CLI_INSTANCE_ENV=$EVAL_LIB_RESOLVED_INSTANCE"
    "$CORE_JOURNEY_CLI_PORT_ENV=$EVAL_LIB_DEV_API_PORT"
    "CORE_JOURNEY_INSTANCE=$EVAL_LIB_RESOLVED_INSTANCE"
    "CORE_JOURNEY_API_PORT=$EVAL_LIB_DEV_API_PORT"
    "CORE_JOURNEY_API_URL=http://$EVAL_LIB_DEV_HOST:$EVAL_LIB_DEV_API_PORT"
    "CORE_JOURNEY_API_TOKEN=$EVAL_LIB_RESOLVED_API_TOKEN"
    "CORE_JOURNEY_DATA_DIR=$EVAL_LIB_RESOLVED_DATA_DIR"
    "CORE_JOURNEY_CLI_PATH=$CORE_JOURNEY_CLI_PATH"
    "CORE_JOURNEY_CLI_NAME=$CORE_JOURNEY_CLI_NAME"
    "CORE_JOURNEY_CLI_INSTANCE_ENV=$CORE_JOURNEY_CLI_INSTANCE_ENV"
    "CORE_JOURNEY_CLI_PORT_ENV=$CORE_JOURNEY_CLI_PORT_ENV"
    "$product_instance_env=$EVAL_LIB_RESOLVED_INSTANCE"
    "$product_port_env=$EVAL_LIB_DEV_API_PORT"
    "$product_api_url_env=http://$EVAL_LIB_DEV_HOST:$EVAL_LIB_DEV_API_PORT"
    "$product_api_token_env=$EVAL_LIB_RESOLVED_API_TOKEN"
    "$product_data_dir_env=$EVAL_LIB_RESOLVED_DATA_DIR"
    "$product_socket_env=$EVAL_LIB_RESOLVED_SOCKET_PATH"
  )
  local -a unset_context=()
  for suffix in INSTANCE API_PORT API_URL API_TOKEN DATA_DIR SOCKET_PATH VITE_PORT CDP_PORT DEV_INSTANCE DEV_API_PORT DEV_VITE_PORT DEV_CDP_PORT DEV_DATA_DIR DEV_IDENTIFIER \
    STARTUP_ATTEMPT_ID SESSION_BINDING_NONCE SESSION_ID TEAM_ID PLAN_ID TASK_ID ATTEMPT_ID TAB_ID PARENT_TAB_ID; do
    unset_context+=(-u "$(core_journey_product_env "$suffix")")
  done
  env "${unset_context[@]}" "${vars[@]}" "$@"
}

dc() {
  local caller_tab_id="" cli
  if [ "${1:-}" = --as-tab ]; then
    caller_tab_id="${2:-}"; [ -n "$caller_tab_id" ] || { echo 'ERROR [dc]: --as-tab requires a tab id.' >&2; return 7; }; shift 2
  fi
  dev_resolve || return 7
  cli="$CORE_JOURNEY_CLI_PATH"
  dev_assert_target || return 7
  dev_assert_cached_identity || return 7
  if [ -n "$caller_tab_id" ]; then
    local inspect session nonce binding=()
    inspect="$(dev_run "$cli" tab inspect "$caller_tab_id" --json 2>/dev/null)" || true
    session="$(printf '%s' "$inspect" | { grep -m1 -oE '"agent_session_id": *"[^"]+"' || true; } | sed -E 's/.*"([^"]+)"$/\1/')"
    nonce="$(printf '%s' "$inspect" | { grep -m1 -oE '"session_binding_nonce": *"[^"]+"' || true; } | sed -E 's/.*"([^"]+)"$/\1/')"
      [ -z "$session" ] || binding+=("$(core_journey_product_env SESSION_ID)=$session")
      [ -z "$nonce" ] || binding+=("$(core_journey_product_env SESSION_BINDING_NONCE)=$nonce")
    dev_run --as-tab "$caller_tab_id" env ${binding[@]+"${binding[@]}"} "$cli" "$@"
  else
    dev_run "$cli" "$@"
  fi
}
