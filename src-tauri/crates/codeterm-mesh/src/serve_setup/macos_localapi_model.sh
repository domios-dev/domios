valid_port() {
    case "$1" in ''|*[!0-9]*) return 1 ;; esac
    [ "${#1}" -le 5 ] && [ "$1" -gt 0 ] && [ "$1" -le 65535 ]
}

valid_endpoint() {
    case "$1" in /localapi/v0/status|/localapi/v0/serve-config) return 0 ;; *) return 1 ;; esac
}

request_config() {
    valid_port "$1" && valid_endpoint "$3" || return 1
    case "$2" in ''|*[!0-9a-fA-F]*) return 1 ;; esac
    printf 'user = ":%s"\nheader = "Host: 127.0.0.1:%s"\nurl = "http://127.0.0.1:%s%s"\n' "$2" "$1" "$1" "$3"
}

app_store_request_config() {
    case "$1" in sameuserproof-*-*) ;; *) return 1 ;; esac
    rest=${1#sameuserproof-}
    request_config "${rest%%-*}" "${rest#*-}" "$2"
}
