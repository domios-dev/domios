#!/usr/bin/env bash
# WHAT: run one provider row of the submission matrix through bounded focused oracles.
# WHY: descriptor capabilities, not copied provider assertions, define full versus restricted scope.
# ORACLE: the Rust matrix and focused UI tests exit green while the real codeterm CLI is callable.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
PROVIDER=""
DRY_RUN=false

usage() {
    cat <<'EOF'
Usage: bash scripts/submission-matrix.sh --provider <id> [--dry-run]

Options:
  --provider <id>  Provider descriptor row under plugins/provider-<id>/plugin.json
  --dry-run        Verify the real codeterm CLI and print bounded test commands
  -h, --help       Show this help
EOF
}

while (($#)); do
    case "$1" in
        --provider)
            [[ $# -ge 2 ]] || {
                printf 'error: --provider requires an id\n' >&2
                exit 2
            }
            PROVIDER="$2"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf 'error: unknown argument: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

[[ -n "$PROVIDER" ]] || {
    printf 'error: --provider is required\n' >&2
    usage >&2
    exit 2
}

MANIFEST="$ROOT/plugins/provider-$PROVIDER/plugin.json"
[[ -f "$MANIFEST" ]] || {
    printf 'error: provider descriptor not found: %s\n' "$MANIFEST" >&2
    exit 2
}

readarray -t CAPABILITIES < <(
    node -e '
const descriptor = require(process.argv[1]);
console.log(descriptor.spawn.readinessStrategy.kind);
console.log(descriptor.spawn.verifySubmit.kind);
console.log(Boolean(descriptor.spawn.hasHooks));
' "$MANIFEST"
)

printf 'provider=%s readiness=%s verifySubmit=%s hasHooks=%s\n' \
    "$PROVIDER" "${CAPABILITIES[0]}" "${CAPABILITIES[1]}" "${CAPABILITIES[2]}"

timeout --signal=TERM --kill-after=2s 10s codeterm identify --json >/dev/null
printf 'PASS real codeterm CLI identify\n'

RUST_COMMAND=(
    env TEST_CAP_SECS=90
    bash scripts/test-capped.sh
    --manifest-path src-tauri/Cargo.toml
    --lib "submission_matrix::$PROVIDER"
    -- -- --nocapture
)
UI_COMMANDS=(
    "timeout --signal=TERM --kill-after=5s 30s npm run test:unit -- ChatComposer.submission-keys"
    "timeout --signal=TERM --kill-after=5s 30s npm run test:unit -- chatSendStatus"
    "timeout --signal=TERM --kill-after=5s 30s npm run test:unit -- TabSessionChat.delivery"
)

printf 'RUN'
printf ' %q' "${RUST_COMMAND[@]}"
printf '\n'
if [[ "$DRY_RUN" == false ]]; then
    (
        cd "$ROOT"
        "${RUST_COMMAND[@]}"
    )
fi

if [[ "${CAPABILITIES[2]}" == true ]]; then
    for command in "${UI_COMMANDS[@]}"; do
        printf 'RUN %s\n' "$command"
        if [[ "$DRY_RUN" == false ]]; then
            UI_LOG=$(mktemp)
            if ! (
                cd "$ROOT"
                bash -lc "$command"
            ) | tee "$UI_LOG"; then
                rm -f "$UI_LOG"
                exit 1
            fi
            if ! grep -Eq '✓ all [0-9]+ TS unit test file\(s\) green' "$UI_LOG"; then
                printf 'error: focused TS runner ended without its green summary\n' >&2
                rm -f "$UI_LOG"
                exit 1
            fi
            rm -f "$UI_LOG"
        fi
    done
else
    for dimension in optimistic_reconciliation persistence_across_remount visible_failure_and_retry; do
        printf 'SKIP %s %s: quota constraint: Cursor scope is startup + exact ingestion; submit writes are forbidden\n' \
            "$PROVIDER" "$dimension"
    done
fi
