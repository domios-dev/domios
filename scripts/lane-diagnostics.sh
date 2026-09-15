lane_diagnostics_tool() {
  local name="$1" rc=0
  shift
  printf 'CODETERM_DIAGNOSTIC_TOOL\t%s\n' "$name"
  set +e
  "$@" 2>&1
  rc=$?
  set -e
  printf 'CODETERM_DIAGNOSTIC_EXIT\t%s\t%s\n' "$name" "$rc"
  return 0
}

lane_diagnostics_note() {
  printf 'CODETERM_DIAGNOSTIC_TOOL\tlane-capability\n'
  printf 'CODETERM_DIAGNOSTIC_EXIT\tlane-capability\t0\n'
  printf '%s\n' "$1"
}

lane_diagnostics_changed() {
  printf '%s\n' "${CODETERM_DIAGNOSTICS_FILES:-}" | grep -Eq "$1"
}

lane_diagnostics_rust() {
  local crate
  IFS=',' read -ra crates <<< "${CODETERM_DIAGNOSTICS_CRATES:-}"
  for crate in "${crates[@]}"; do
    case "$crate" in
      app)
        lane_diagnostics_tool cargo-check cargo check --manifest-path src-tauri/Cargo.toml --package codeterm --message-format=json
        ;;
      plan)
        lane_diagnostics_tool cargo-check cargo check --manifest-path src-tauri/Cargo.toml --package codeterm-plan --message-format=json
        ;;
      types)
        lane_diagnostics_tool cargo-check cargo check --manifest-path src-tauri/Cargo.toml --package codeterm-types --message-format=json
        ;;
      cli)
        lane_diagnostics_tool cargo-check cargo check --manifest-path cli/Cargo.toml --message-format=json
        ;;
    esac
  done
  if lane_diagnostics_changed '(^|/)(.*\.rs|Cargo\.toml|Cargo\.lock)$'; then
    if lane_diagnostics_changed '^cli/'; then
      lane_diagnostics_tool fmt cargo fmt --manifest-path cli/Cargo.toml --check
    fi
    if lane_diagnostics_changed '^src-tauri/'; then
      lane_diagnostics_tool fmt cargo fmt --manifest-path src-tauri/Cargo.toml --check
    fi
    if command -v cargo >/dev/null 2>&1; then
      for crate in "${crates[@]}"; do
        case "$crate" in
          app)
            lane_diagnostics_tool clippy cargo clippy --manifest-path src-tauri/Cargo.toml --package codeterm --message-format=json
            ;;
          plan)
            lane_diagnostics_tool clippy cargo clippy --manifest-path src-tauri/Cargo.toml --package codeterm-plan --message-format=json
            ;;
          types)
            lane_diagnostics_tool clippy cargo clippy --manifest-path src-tauri/Cargo.toml --package codeterm-types --message-format=json
            ;;
          cli)
            lane_diagnostics_tool clippy cargo clippy --manifest-path cli/Cargo.toml --message-format=json
            ;;
        esac
      done
    fi
    if lane_diagnostics_changed '(^|/)(Cargo\.toml|Cargo\.lock)$'; then
      if command -v cargo-machete >/dev/null 2>&1; then
        lane_diagnostics_tool machete cargo machete
      else
        lane_diagnostics_note 'cargo-machete unavailable; dependency analysis skipped'
      fi
    fi
    if command -v bacon >/dev/null 2>&1; then
      lane_diagnostics_tool bacon bacon --headless --job check
    else
      lane_diagnostics_note 'bacon unavailable; headless background analysis skipped'
    fi
  fi
}

lane_diagnostics_typescript() {
  local tsc="node_modules/typescript/bin/tsc"
  if [ ! -f "$tsc" ]; then
    lane_diagnostics_note 'typescript compiler unavailable; TypeScript analysis skipped'
    return
  fi
  lane_diagnostics_tool tsc-build node "$tsc" -b tsconfig.json --pretty false --noEmit --tsBuildInfoFile .codeterm-diagnostics.tsbuildinfo
  lane_diagnostics_tool tsc node "$tsc" --noEmit -p tsconfig.json --pretty false
  if command -v tsgo >/dev/null 2>&1; then
    lane_diagnostics_tool tsgo tsgo --noEmit -p tsconfig.json --pretty false
  else
    lane_diagnostics_note 'tsgo unavailable; tsc remains the TypeScript authority'
  fi
  if lane_diagnostics_changed '^packages/chat-core/'; then
    if command -v knip >/dev/null 2>&1; then
      lane_diagnostics_tool knip knip --workspace @codeterm/chat-core --exports
    elif command -v pnpm >/dev/null 2>&1; then
      lane_diagnostics_tool knip pnpm exec knip --workspace @codeterm/chat-core --exports
    else
      lane_diagnostics_note 'knip unavailable; unused-export analysis skipped'
    fi
  fi
  if command -v eslint >/dev/null 2>&1 && { [ -f eslint.config.js ] || [ -f eslint.config.mjs ] || [ -f .eslintrc ]; }; then
    local file
    local -a lint_files=()
    while IFS= read -r file; do
      case "$file" in
        *.ts|*.tsx) [ -f "$file" ] && lint_files+=("$file") ;;
      esac
    done <<< "${CODETERM_DIAGNOSTICS_FILES:-}"
    if [ "${#lint_files[@]}" -gt 0 ]; then
      lane_diagnostics_tool eslint eslint "${lint_files[@]}"
    fi
  fi
}

lane_diagnostics_duplicate_code() {
  command -v jscpd >/dev/null 2>&1 || {
    lane_diagnostics_note 'jscpd unavailable; duplicate-code analysis skipped'
    return
  }
  local file
  local -a changed_files=()
  while IFS= read -r file; do
    case "$file" in
      *.rs|*.ts|*.tsx) [ -f "$file" ] && changed_files+=("$file") ;;
    esac
  done <<< "${CODETERM_DIAGNOSTICS_FILES:-}"
  if [ "${#changed_files[@]}" -gt 0 ]; then
    lane_diagnostics_tool jscpd jscpd --reporters console --min-lines 5 "${changed_files[@]}"
  fi
}

lane_diagnostics_gates() {
  lane_diagnostics_tool file-sizes bash scripts/check-file-sizes.sh --fail
  if [ -f scripts/check-prompts-doc.sh ]; then
    lane_diagnostics_tool prompts-gate bash scripts/check-prompts-doc.sh
  fi
  if [ -f src/lib/webForks.test.ts ] && [ -f scripts/run-ts-unit-tests.mjs ]; then
    lane_diagnostics_tool webforks-gate node scripts/run-ts-unit-tests.mjs src/lib/webForks.test.ts
  fi
  if lane_diagnostics_changed '^cli/src/(args|main\.rs)' || lane_diagnostics_changed '^cli/src/args/'; then
    if git diff --quiet -- docs/cli-manifest.json docs/CLI.md packages/chat-ui/src/codeterm/commandPaths.ts; then
      lane_diagnostics_tool cli-manifest-gate bash -c 'printf "%s\n" "CLI manifest artifacts are stale; run codeterm cli-manifest --write in canonical QA"; exit 1'
    else
      lane_diagnostics_note 'CLI manifest artifacts changed with the lane; canonical freshness test remains authoritative'
    fi
  fi
}

lane_diagnostics_run() {
  if [ "${CODETERM_DIAGNOSTICS_OWNER:-}" != "daemon" ] || [ "${CODETERM_DIAGNOSTICS_SCOPE:-}" != "warm" ]; then
    printf '%s\n' 'lane diagnostics is daemon-owned and disabled outside the warm checkout' >&2
    return 77
  fi
  enter_build_gateway 'lane-diagnostics'
  ct_configure_sccache
  ensure_deps
  lane_diagnostics_rust
  lane_diagnostics_typescript
  lane_diagnostics_duplicate_code
  lane_diagnostics_gates
}

lane_diagnostics_run
