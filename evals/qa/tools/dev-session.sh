# Source only after evals/_lib/lib.sh, from the repo root.
: "${QA_OUT:?Set QA_OUT to a fresh evidence directory}"
mkdir -p "$QA_OUT" || return 1
qa_run() {
  local label="$1"; shift
  [[ "$label" =~ ^[A-Za-z0-9_-]+$ ]] || return 2
  case "${1:-}" in dc|dev_get|dev_run) ;; *) return 2 ;; esac
  case " $* " in *" mem secret "*|*" account "*|*" web link "*)
    printf '%s\n' 'Refused: secret/account/link capture needs a separate redacted flow' >&2
    return 7 ;;
  esac
  local stem="$QA_OUT/$label"
  [ ! -e "$stem.command.txt" ] || { printf '%s\n' 'Refused: evidence label already exists' >&2; return 2; }
  printf '%q ' "$@" > "$stem.command.txt"
  printf '\n' >> "$stem.command.txt"
  local result=0
  "$@" > "$stem.stdout.txt" 2> "$stem.stderr.txt" || result=$?
  printf '%s\n' "$result" > "$stem.exit.txt"
  cat "$stem.stdout.txt"
  cat "$stem.stderr.txt" >&2
  printf 'exit=%s\n' "$result"
  return "$result"
}
