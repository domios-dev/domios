#!/usr/bin/env bash
# Caller: CI release-credentials.yml; skill github-release
# Refuse to cut a release unless every self-hosted runner label the release and its tests need is online.
set -euo pipefail

REPO="${RELEASE_REPO:-rollacode/codeterm}"
variables=$(gh api "repos/$REPO/actions/variables" --paginate --jq '.variables[] | [.name, .value] | @tsv')
label_for() {
    local value
    value=$(printf '%s\n' "$variables" | awk -F'\t' -v key="$1" '$1 == key {print $2}')
    printf '%s\n' "${value:-$2}"
}
REQUIRED_LABELS=(
    "$(label_for RELEASE_MACOS_ARM64_LABEL codeterm-builder)"
    "$(label_for RELEASE_MACOS_X64_LABEL codeterm-builder)"
    "$(label_for RELEASE_LINUX_ARM64_LABEL codeterm-builder)"
    "$(label_for RELEASE_WINDOWS_X64_LABEL windows-x64-build)"
    "$(label_for RELEASE_WINDOWS_ARM64_LABEL windows-build)"
    "$(label_for RELEASE_LINUX_X64_LABEL linux-build)"
    rust-tests rust-shard linux-ci windows-ci light
)

runners=$(gh api "repos/$REPO/actions/runners" --paginate \
    --jq '.runners[] | "\(.status)\t\(.name)\t\([.labels[].name] | join(","))"')

missing=0
for label in $(printf '%s\n' "${REQUIRED_LABELS[@]}" | awk '!seen[$0]++'); do
    online=$(printf '%s\n' "$runners" | awk -F'\t' -v l="$label" \
        '$1 == "online" && index("," $3 ",", "," l ",") > 0 { print $2 }')
    if [ -n "$online" ]; then
        printf 'ok       %-18s %s\n' "$label" "$(printf '%s' "$online" | tr '\n' ' ')"
    else
        known=$(printf '%s\n' "$runners" | awk -F'\t' -v l="$label" \
            'index("," $3 ",", "," l ",") > 0 { print $2 " (" $1 ")" }')
        printf 'OFFLINE  %-18s %s\n' "$label" "${known:-no runner registered}"
        missing=1
    fi
done

if [ "$missing" -ne 0 ]; then
    printf '\nA required release runner is offline. Bring it up before tagging.\n' >&2
    exit 1
fi
