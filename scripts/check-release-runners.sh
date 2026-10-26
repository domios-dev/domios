#!/usr/bin/env bash
# Caller: CI release-credentials.yml; skill github-release
# Refuse to cut a release unless every self-hosted runner label the release and its tests need is online.
set -euo pipefail

REPO="$(node scripts/source-repository.mjs)"
variables=$(gh api "repos/$REPO/actions/variables" --paginate --jq '.variables[] | [.name, .value] | @tsv')
label_for() {
    local value
    value=$(printf '%s\n' "$variables" | awk -F'\t' -v key="$1" '$1 == key {print $2}')
    printf '%s\n' "${value:-$2}"
}
# arm64 releases build on GitHub-hosted runners; these are the self-hosted labels the release and its tests use.
REQUIRED_LABELS=(
    "$(label_for RELEASE_MACOS_ARM64_LABEL codeterm-builder)"
    "$(label_for RELEASE_MACOS_X64_LABEL codeterm-builder)"
    "$(label_for RELEASE_WINDOWS_X64_LABEL windows-x64-build)"
    "$(label_for RELEASE_LINUX_X64_LABEL linux-build)"
    rust-shard light
)

# Most runners are registered to the organization's runner group, so both levels are listed.
row='.runners[] | "\(.status)\t\(.name)\t\([.labels[].name] | join(","))"'
runners=$(gh api "repos/$REPO/actions/runners" --paginate --jq "$row")
if ! org_runners=$(gh api "orgs/${REPO%%/*}/actions/runners" --paginate --jq "$row" 2>&1); then
    printf 'Cannot list organization runners: %s\nThe token needs organization "Self-hosted runners: Read".\n' "$org_runners" >&2
    exit 1
fi
runners=$(printf '%s\n%s\n' "$runners" "$org_runners")

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
