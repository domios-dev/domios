ct_firebase_build_secret() {
  if [ -n "${DEV_NOTIFY_PARENT_CLI:-}" ]; then
    dev_notify_parent_cli mem secret get --name "$1"
  else
    codeterm mem secret get --name "$1"
  fi
}

ct_configure_dev_qa_project() {
  case "${CMD:-}" in dev|dev-server) ;; *) return 2 ;; esac
  local qa_project="" restore_xtrace=0
  case "$-" in *x*) restore_xtrace=1; set +x ;; esac
  qa_project="$(ct_firebase_build_secret DOMIOS_QA_PROJECT_ID 2>/dev/null)" || qa_project=""
  CT_DEV_QA_PROJECT="${qa_project//[[:space:]]/}"
  CT_DEV_QA_PROJECT_PINNED=1
  export DOMIOS_QA_PROJECT_ID="$CT_DEV_QA_PROJECT"
  [ "$restore_xtrace" = 0 ] || set -x
}

ct_with_firebase_build_config() {
  [ "$#" -gt 0 ] || return 2

  local billing_url="" secret_billing_url="" env_billing_url="" qa_project="" qa_api_key="" qa_app_id=""
  local restore_xtrace=0 status=0
  local secret_api_key="" secret_project_id="" secret_app_id="" secret_redeem_url=""
  local env_api_key="" env_project_id="" env_app_id="" env_redeem_url=""
  local api_key="" project_id="" app_id="" redeem_url="" cloud_auth=0

  case "$-" in
    *x*) restore_xtrace=1; set +x ;;
  esac

  env_billing_url="${DOMIOS_BILLING_FUNCTIONS_URL:-}"
  env_api_key="${DOMIOS_FIREBASE_API_KEY:-}"
  env_project_id="${DOMIOS_FIREBASE_PROJECT_ID:-}"
  env_app_id="${DOMIOS_FIREBASE_APP_ID:-}"
  env_redeem_url="${DOMIOS_ACCOUNT_REDEEM_URL:-}"

  secret_api_key="$(ct_firebase_build_secret DOMIOS_FIREBASE_API_KEY 2>/dev/null)" || secret_api_key=""
  secret_project_id="$(ct_firebase_build_secret DOMIOS_FIREBASE_PROJECT_ID 2>/dev/null)" || secret_project_id=""
  secret_app_id="$(ct_firebase_build_secret DOMIOS_FIREBASE_APP_ID 2>/dev/null)" || secret_app_id=""
  secret_redeem_url="$(ct_firebase_build_secret DOMIOS_ACCOUNT_REDEEM_URL 2>/dev/null)" || secret_redeem_url=""

  if [ "${CT_DEV_QA_PROJECT_PINNED:-0}" = 1 ] &&
      { [ "${CMD:-}" = dev ] || [ "${CMD:-}" = dev-server ]; }; then
    qa_project="$CT_DEV_QA_PROJECT"
  else
    qa_project="$(ct_firebase_build_secret DOMIOS_QA_PROJECT_ID 2>/dev/null)" || qa_project=""
  fi
  qa_api_key="$(ct_firebase_build_secret DOMIOS_QA_FIREBASE_API_KEY 2>/dev/null)" || qa_api_key=""
  qa_app_id="$(ct_firebase_build_secret DOMIOS_QA_FIREBASE_APP_ID 2>/dev/null)" || qa_app_id=""
  secret_billing_url="$(ct_firebase_build_secret DOMIOS_BILLING_FUNCTIONS_URL 2>/dev/null)" || secret_billing_url=""
  if [[ -n "${secret_billing_url//[[:space:]]/}" ]]; then
    billing_url="$secret_billing_url"
  elif [[ -n "${env_billing_url//[[:space:]]/}" ]]; then
    billing_url="$env_billing_url"
  fi

  if [[ -n "${secret_api_key//[[:space:]]/}" && -n "${secret_project_id//[[:space:]]/}" &&
    -n "${secret_app_id//[[:space:]]/}" && -n "${secret_redeem_url//[[:space:]]/}" ]]; then
    api_key="$secret_api_key"
    project_id="$secret_project_id"
    app_id="$secret_app_id"
    redeem_url="$secret_redeem_url"
    cloud_auth=1
  elif [[ -n "${env_api_key//[[:space:]]/}" && -n "${env_project_id//[[:space:]]/}" &&
    -n "${env_app_id//[[:space:]]/}" && -n "${env_redeem_url//[[:space:]]/}" ]]; then
    api_key="$env_api_key"
    project_id="$env_project_id"
    app_id="$env_app_id"
    redeem_url="$env_redeem_url"
    cloud_auth=1
  fi

  if [ "$cloud_auth" = 1 ]; then
    printf 'Firebase config: cloud-auth enabled\n'
  else
    printf 'Firebase config: local-only\n'
  fi

  if [ -n "$billing_url" ]; then
    printf 'Billing config: present\n'
  else
    printf 'Billing config: absent\n'
  fi

  if [[ -n "${qa_project//[[:space:]]/}" ]]; then
    printf 'QA project config: present\n'
  else
    printf 'QA project not configured in this build\n'
  fi

  # Read only by debug builds; release pins the production entitlement project.
  DOMIOS_QA_PROJECT_ID="${qa_project//[[:space:]]/}" \
    DOMIOS_QA_FIREBASE_API_KEY="${qa_api_key//[[:space:]]/}" \
    DOMIOS_QA_FIREBASE_APP_ID="${qa_app_id//[[:space:]]/}" \
    DOMIOS_BILLING_FUNCTIONS_URL="$billing_url" \
    DOMIOS_FIREBASE_API_KEY="$api_key" \
    DOMIOS_FIREBASE_PROJECT_ID="$project_id" \
    DOMIOS_FIREBASE_APP_ID="$app_id" \
    DOMIOS_ACCOUNT_REDEEM_URL="$redeem_url" \
    "$@" || status=$?

  unset secret_api_key secret_project_id secret_app_id secret_redeem_url
  unset env_api_key env_project_id env_app_id env_redeem_url
  unset api_key project_id app_id redeem_url billing_url secret_billing_url env_billing_url qa_project qa_api_key qa_app_id
  [ "$restore_xtrace" = 0 ] || set -x
  return "$status"
}
