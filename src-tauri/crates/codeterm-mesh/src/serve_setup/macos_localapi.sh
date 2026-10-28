task_home=$1
path=$2
valid_endpoint "$path" || exit 1

fetch() {
    response=$(/usr/bin/curl --disable --config - --include --raw --silent --fail \
        --max-time 3 --max-filesize 1048576 --noproxy '*' && printf '.') || return 1
    printf '%s' "${response%?}" | /usr/bin/base64
}

for file in "$task_home"/Library/Group\ Containers/*group.io.tailscale.ipn.macos/sameuserproof-*; do
    [ -f "$file" ] || continue
    config=$(app_store_request_config "${file##*/}" "$path") || continue
    result=$(printf '%s\n' "$config" | fetch && printf '.') && { printf '%s' "${result%?}"; exit 0; }
done

port=$(/usr/bin/readlink /Library/Tailscale/ipnport) || exit 1
valid_port "$port" || exit 1
proof=$(/bin/cat "/Library/Tailscale/sameuserproof-$port") || exit 1
config=$(request_config "$port" "$proof" "$path") || exit 1
printf '%s\n' "$config" | fetch
