#!/usr/bin/env bash
# Caller: build.sh script-guards
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

gate="scripts/clippy-gate.sh"
for lint in dead-code unused-imports unused-mut unused-variables; do
    if grep -Eq -- "-A[[:space:]]+${lint}" "$gate" scripts/clippy-allow.sh; then
        echo "clippy gate still suppresses ${lint}" >&2
        exit 1
    fi
done
grep -Fq -- '--all-targets' "$gate"

fixture="$(mktemp -d)"
trap 'status=$?; rm -rf "$fixture"; exit "$status"' EXIT
mkdir -p "$fixture/bin"
cat >"$fixture/bin/cargo" <<'EOF'
#!/usr/bin/env bash
echo "seeded clippy failure" >&2
exit 23
EOF
chmod +x "$fixture/bin/cargo"
if PATH="$fixture/bin:$PATH" bash "$gate" >"$fixture/gate-failure.log" 2>&1; then
    echo "clippy gate swallowed a seeded cargo failure" >&2
    exit 1
fi
grep -Fq 'seeded clippy failure' "$fixture/gate-failure.log"

cat >"$fixture/bin/cargo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$CARGO_CALL_LOG"
EOF
export CARGO_CALL_LOG="$fixture/cargo-calls.log"
PATH="$fixture/bin:$PATH" bash "$gate"
if [ "$(grep -c -- '--all-targets' "$CARGO_CALL_LOG")" != 2 ] || [ "$(wc -l <"$CARGO_CALL_LOG")" != 2 ]; then
    echo "clippy gate must run exactly one --all-targets pass per workspace" >&2
    exit 1
fi

mkdir -p "$fixture/src"
cat >"$fixture/Cargo.toml" <<'EOF'
[package]
name = "codeterm-clippy-gate-fixture"
version = "0.0.0"
edition = "2021"
EOF
cat >"$fixture/src/lib.rs" <<'EOF'
fn test_only_guard() -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_only_usage_does_not_make_production_code_live() {
        assert!(super::test_only_guard());
    }
}
EOF

if cargo clippy --quiet --manifest-path "$fixture/Cargo.toml" --all-targets -- -D warnings \
    >"$fixture/all-targets.log" 2>&1; then
    echo "all-targets clippy pass failed to detect production dead code" >&2
    exit 1
fi
grep -Fq 'test_only_guard' "$fixture/all-targets.log"
if cargo clippy --quiet --manifest-path "$fixture/Cargo.toml" --lib -- -D warnings \
    >"$fixture/non-test.log" 2>&1; then
    echo "non-test clippy pass failed to detect test-only dead code" >&2
    exit 1
fi
grep -Fq 'test_only_guard' "$fixture/non-test.log"
echo "clippy gate regression checks passed"
