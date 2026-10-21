if [ -n "${CODETERM_CLI_PATH:-}" ]; then
  case $CODETERM_CLI_PATH in
    [a-zA-Z]:*) __ct_cli_dir=$(/usr/bin/cygpath -u "$CODETERM_CLI_PATH") ;;
    *) __ct_cli_dir=$CODETERM_CLI_PATH ;;
  esac
  __ct_cli_dir=${__ct_cli_dir%/*}
  __ct_cli_rest=${PATH-}
  __ct_cli_path=$__ct_cli_dir
  while :; do
    case $__ct_cli_rest in
      *:*) __ct_cli_entry=${__ct_cli_rest%%:*}; __ct_cli_rest=${__ct_cli_rest#*:}; __ct_cli_more=1 ;;
      *) __ct_cli_entry=$__ct_cli_rest; __ct_cli_more=0 ;;
    esac
    if [ "$__ct_cli_entry" != "$__ct_cli_dir" ]; then
      __ct_cli_path=$__ct_cli_path:$__ct_cli_entry
    fi
    [ "$__ct_cli_more" = 1 ] || break
  done
  export PATH=$__ct_cli_path
  unset __ct_cli_dir __ct_cli_rest __ct_cli_path __ct_cli_entry __ct_cli_more
fi
