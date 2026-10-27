home=$1
path=$2
case "$path" in /localapi/v0/status|/localapi/v0/serve-config) ;; *) exit 1 ;; esac

fetch() {
    case "$port" in ''|*[!0-9]*) return 1 ;; esac
    [ "$port" -gt 0 ] && [ "$port" -le 65535 ] || return 1
    case "$proof" in ''|*[!0-9a-fA-F]*) return 1 ;; esac
    printf 'user = ":%s"\n' "$proof" | \
        /usr/bin/curl --config - --silent --fail --max-time 3 --noproxy '*' \
        "http://127.0.0.1:$port$path"
}

for file in "$home"/Library/Group\ Containers/*group.io.tailscale.ipn.macos/sameuserproof-*; do
    [ -f "$file" ] || continue
    rest=${file##*/sameuserproof-}
    port=${rest%%-*}
    proof=${rest#*-}
    result=$(fetch) && { printf '%s' "$result"; exit 0; }
done

port=$(/usr/bin/readlink /Library/Tailscale/ipnport) || exit 1
case "$port" in ''|*[!0-9]*) exit 1 ;; esac
proof=$(/bin/cat "/Library/Tailscale/sameuserproof-$port") || exit 1
fetch
