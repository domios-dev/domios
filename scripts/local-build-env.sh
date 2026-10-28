ct_uses_local_build_env() {
  [ -z "${CI:-}" ] || return 1
  case "${CMD:-}" in dev|dev-server) return 0 ;; *) return 1 ;; esac
}

ct_load_local_build_env() {
  local key value complete=0 restore_xtrace=0
  case "$-" in *x*) restore_xtrace=1; set +x ;; esac
  while IFS= read -r -d '' key && IFS= read -r -d '' value; do
    if [ "$key" = __COMPLETE__ ]; then complete=1; continue; fi
    if [[ "$key" != DOMIOS_* || "$key" == *[!A-Z0-9_]* ]]; then return 2; fi
    printf -v "CT_LOCAL_BUILD_ENV_$key" '%s' "$value"
  done < <(node scripts/local-build-env.mjs read)
  CT_LOCAL_BUILD_ENV_LOADED="$complete"
  [ "$restore_xtrace" = 0 ] || set -x
  [ "$complete" = 1 ]
}

ct_local_build_env_value() {
  local variable="CT_LOCAL_BUILD_ENV_$1"
  if [ "${CT_LOCAL_BUILD_ENV_LOADED:-0}" != 1 ] || [ -z "${!variable:-}" ]; then
    printf 'Missing local build key %s; run ./build.sh dev-env once\n' "$1" >&2
    return 2
  fi
  printf '%s' "${!variable}"
}
