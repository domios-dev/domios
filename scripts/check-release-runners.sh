#!/usr/bin/env bash
# Refuse to cut a release unless every self-hosted runner label the release needs is online.
set -euo pipefail

REPO="${RELEASE_REPO:-rollacode/codeterm}"
REQUIRED_LABELS=(codeterm-builder windows-build linux-build)

runners=$(gh api "repos/$REPO/actions/runners" --paginate \
    --jq '.runners[] | "\(.status)\t\(.name)\t\([.labels[].name] | join(","))"')

missing=0
for label in "${REQUIRED_LABELS[@]}"; do
    online=$(printf '%s\n' "$runners" | awk -F'\t' -v l="$label" \
        '$1 == "online" && ("," $3 ",") ~ ("," l ",") { print $2 }')
    if [ -n "$online" ]; then
        printf 'ok       %-18s %s\n' "$label" "$(printf '%s' "$online" | tr '\n' ' ')"
    else
        known=$(printf '%s\n' "$runners" | awk -F'\t' -v l="$label" \
            '("," $3 ",") ~ ("," l ",") { print $2 " (" $1 ")" }')
        printf 'OFFLINE  %-18s %s\n' "$label" "${known:-no runner registered}"
        missing=1
    fi
done

if [ "$missing" -ne 0 ]; then
    printf '\nA required release runner is offline. Bring it up before tagging.\n' >&2
    exit 1
fi
