# Pull request command parity

Baseline: `d20a0a4ed`. Captured before editing tests.yml. Each run step is recorded below; checkout/setup/upload actions are retained. Release-only cancellation and comment steps are included for completeness; their inputs make them unreachable from PR. Conditions combine the job and step expressions. `inputs` are empty on pull_request; `changed` succeeds, and `scopeForRun(files, plugins, false, true)` makes heavy=false. Thus Rust archive, shard, platform, input-platform, coverage and clippy jobs are unreachable.

Full Rust coverage runs through release CI (`workflow_call`) and manual dispatch, subject to change scope. There is no schedule trigger.

The tests.yml projection is selected by catalogue `ci: [tests.yml:<job>]` ownership. Other workflows keep their own PR path filters and commands. The selector emits commands from `select --tier pr --format json`; each execution step reads its entry from that output. Existing job conditions remain in place; individual catalogue execution steps retain their conditions. The earlier catalogue migration consolidates release-contracts file groups: node:test executes all discovered Node files despite individual test failures, and the Python group uses !cancelled() so later contracts still run after Node failures. This widens failure-path coverage while preserving change scope. Selected core-journeys-offline is added to release-contracts by Orchestrator decision.

`rust-warnings` calls `rust-warnings-summary.yml:report`, whose timeout remains 5; reusable callers cannot declare timeout-minutes. Other PR jobs are capped at 8; `changed` stays at 5. Heavy-only job timeouts are unchanged.

| Job | Condition (job; step) | Before | After | Result / justification |
|---|---|---|---|---|
| changed | `always after successful preceding steps` | `set -euo pipefail<br>if [ "$CI_RELEASE_CALL" = true ] &#124;&#124; [ "${{ github.event_name }}" != push ]; then<br>  echo "run=true" >> "$GITHUB_OUTPUT"<br>  exit 0<br>fi<br>tree=$(git rev-parse 'HEAD^{tree}')<br>for parent in $(git rev-parse 'HEAD^@'); do<br>  if [ "$(git rev-parse "$parent^{tree}")" = "$tree" ]; then<br>    echo "HEAD tree $tree already carried by parent $parent"<br>    echo "run=false" >> "$GITHUB_OUTPUT"<br>    exit 0<br>  fi<br>done<br>echo "run=true" >> "$GITHUB_OUTPUT"` | `set -euo pipefail<br>if [ "$CI_RELEASE_CALL" = true ] &#124;&#124; [ "${{ github.event_name }}" != push ]; then<br>  echo "run=true" >> "$GITHUB_OUTPUT"<br>  exit 0<br>fi<br>tree=$(git rev-parse 'HEAD^{tree}')<br>for parent in $(git rev-parse 'HEAD^@'); do<br>  if [ "$(git rev-parse "$parent^{tree}")" = "$tree" ]; then<br>    echo "HEAD tree $tree already carried by parent $parent"<br>    echo "run=false" >> "$GITHUB_OUTPUT"<br>    exit 0<br>  fi<br>done<br>echo "run=true" >> "$GITHUB_OUTPUT"` | equal |
| changed | `steps.tree.outputs.run == 'true'` | `node scripts/ci-scope.mjs HEAD^ HEAD` | `node scripts/ci-scope.mjs HEAD^ HEAD` | equal |
| changed | `${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | equal |
| plugin-metadata | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.result == 'success') }}` | `node scripts/release-plugin.mjs check` | `${{ fromJSON(steps.catalogue.outputs.commands)['plugin-release-contracts'] }}` | equal command via catalogue output |
| plugin-metadata | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.result == 'success') }}` | `node --test scripts/tests/plugin-releases.test.mjs scripts/tests/ci-scope.test.mjs` | `node --test scripts/tests/plugin-releases.test.mjs scripts/tests/ci-scope.test.mjs` | equal |
| plugin-metadata | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.result == 'success') }}; ${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | equal |
| plugins | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.plugins != '[]') }}` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | equal |
| plugins | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.plugins != '[]') }}` | `node scripts/check-plugin.mjs '${{ matrix.plugin.id }}'` | `${PLUGIN_COMMAND//<plugin-id>/"$PLUGIN_ID"}` | equal command via catalogue output |
| plugins | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.plugins != '[]') }}; ${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | equal |
| fmt | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}` | `if command -v cargo >/dev/null 2>&1; then exit 0; fi<br>for dir in "${CARGO_HOME:-}/bin" "$HOME/.cargo/bin" /home/*/.cargo/bin /Users/*/.cargo/bin; do<br>  if [ -x "$dir/cargo" ]; then echo "$dir" >> "$GITHUB_PATH"; exit 0; fi<br>done<br>echo "cargo not found; restore Rust on the runner" >&2<br>exit 1` | `if command -v cargo >/dev/null 2>&1; then exit 0; fi<br>for dir in "${CARGO_HOME:-}/bin" "$HOME/.cargo/bin" /home/*/.cargo/bin /Users/*/.cargo/bin; do<br>  if [ -x "$dir/cargo" ]; then echo "$dir" >> "$GITHUB_PATH"; exit 0; fi<br>done<br>echo "cargo not found; restore Rust on the runner" >&2<br>exit 1` | equal |
| fmt | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}` | `cargo fmt --manifest-path src-tauri/Cargo.toml --check<br>cargo fmt --manifest-path cli/Cargo.toml --check` | `${{ fromJSON(steps.catalogue.outputs.commands)['rust-fmt'] }}` | equal command via catalogue output |
| fmt | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}; ${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | equal |
| ts-static | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | equal |
| ts-static | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}` | `pnpm exec tsc --noEmit` | `${{ fromJSON(steps.catalogue.outputs.commands)['ts-typecheck'] }}` | equal command via catalogue output |
| ts-static | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}; ${{ !cancelled() }}` | `pnpm check:unused-exports` | `${{ fromJSON(steps.catalogue.outputs.commands)['ts-unused-exports'] }}` | equal command via catalogue output |
| ts-static | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}; ${{ !cancelled() }}` | `pnpm knip:report` | `${{ fromJSON(steps.catalogue.outputs.commands)['ts-knip-report'] }}` | equal command via catalogue output |
| ts-static | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}; ${{ !cancelled() }}` | `pnpm check:api-effects` | `${{ fromJSON(steps.catalogue.outputs.commands)['ts-api-effects'] }}` | equal command via catalogue output |
| ts-static | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}; ${{ !cancelled() }}` | `pnpm lint` | `${{ fromJSON(steps.catalogue.outputs.commands)['ts-biome'] }}` | equal command via catalogue output |
| ts-static | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}; ${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | equal |
| ts-types | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' &#124;&#124; needs.changed.outputs.rust == 'true') }}` | `if command -v cargo >/dev/null 2>&1; then exit 0; fi<br>for dir in "${CARGO_HOME:-}/bin" "$HOME/.cargo/bin" /home/*/.cargo/bin /Users/*/.cargo/bin; do<br>  if [ -x "$dir/cargo" ]; then echo "$dir" >> "$GITHUB_PATH"; exit 0; fi<br>done<br>echo "cargo not found; restore Rust on the runner" >&2<br>exit 1` | `if command -v cargo >/dev/null 2>&1; then exit 0; fi<br>for dir in "${CARGO_HOME:-}/bin" "$HOME/.cargo/bin" /home/*/.cargo/bin /Users/*/.cargo/bin; do<br>  if [ -x "$dir/cargo" ]; then echo "$dir" >> "$GITHUB_PATH"; exit 0; fi<br>done<br>echo "cargo not found; restore Rust on the runner" >&2<br>exit 1` | equal |
| ts-types | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' &#124;&#124; needs.changed.outputs.rust == 'true') }}` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | equal |
| ts-types | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' &#124;&#124; needs.changed.outputs.rust == 'true') }}` | `node -e "require('fs').writeFileSync('ci-rust-warnings.log', '')"` | `node -e "require('fs').writeFileSync('ci-rust-warnings.log', '')"` | equal |
| ts-types | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' &#124;&#124; needs.changed.outputs.rust == 'true') }}` | `node scripts/collect-rust-warnings.mjs run --log ci-rust-warnings.log -- bash -c 'pnpm types:check'` | `node scripts/collect-rust-warnings.mjs run --log ci-rust-warnings.log -- bash -c '${{ fromJSON(steps.catalogue.outputs.commands)['ts-generated-types'] }}'` | equal command via catalogue output |
| ts-types | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' &#124;&#124; needs.changed.outputs.rust == 'true') }}; ${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | equal |
| ts-unit | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | equal |
| ts-unit | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}` | `pnpm test:unit` | `${{ fromJSON(steps.catalogue.outputs.commands)['ts-unit'] }}` | equal command via catalogue output |
| ts-unit | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}; ${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | equal |
| ts-vitest | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | equal |
| ts-vitest | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}` | `pnpm test:vitest --maxWorkers=4` | `${{ fromJSON(steps.catalogue.outputs.commands)['ts-vitest'] }}` | equal command via catalogue output |
| ts-vitest | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}; ${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | equal |
| ts-plugins | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | equal |
| ts-plugins | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}` | `pnpm check:plugin-bundles` | `${{ fromJSON(steps.catalogue.outputs.commands)['plugin-bundles'] }}` | equal command via catalogue output |
| ts-plugins | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}; ${{ !cancelled() }}` | `pnpm test:plugins` | `${{ fromJSON(steps.catalogue.outputs.commands)['plugin-tests'] }}` | equal command via catalogue output |
| ts-plugins | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}; ${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | equal |
| release-contracts | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | `pnpm install --frozen-lockfile --store-dir "$RUNNER_TOOL_CACHE/pnpm-store"` | equal |
| release-contracts | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}` | `node --test scripts/tests/release-test-gate.test.mjs scripts/tests/ci-scope.test.mjs scripts/tests/release-job-graph.test.mjs scripts/tests/release-bundle-smoke.test.mjs scripts/tests/release-asset-scope.test.mjs` | `node scripts/test-catalogue.mjs run --entry node-script-tests` | equal coverage; added discovered contracts justified by catalogue migration (existing files retained; overlay platform_red cases remain explicit) |
| release-contracts | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}` | `node --test scripts/tests/ci-rust-shards.test.mjs scripts/tests/ci-runner-paths.test.mjs scripts/tests/ci-rust-archives.test.mjs` | `node scripts/test-catalogue.mjs run --entry node-script-tests` | equal coverage; added discovered contracts justified by catalogue migration (existing files retained; overlay platform_red cases remain explicit) |
| release-contracts | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}` | `python3 -m unittest scripts.tests.test_release_issue_contract` | `node scripts/test-catalogue.mjs run --entry python-script-tests` | equal coverage; added discovered contracts justified by catalogue migration (existing files retained; overlay platform_red cases remain explicit) |
| release-contracts | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}; ${{ !cancelled() }}` | `python3 -m unittest scripts.tests.test_release_workflow_contract` | `node scripts/test-catalogue.mjs run --entry python-script-tests` | equal coverage; added discovered contracts justified by catalogue migration (existing files retained; overlay platform_red cases remain explicit) |
| release-contracts | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}; ${{ !cancelled() }}` | `node --test scripts/tests/updater-manifest.test.mjs` | `node scripts/test-catalogue.mjs run --entry node-script-tests` | equal coverage; added discovered contracts justified by catalogue migration (existing files retained; overlay platform_red cases remain explicit) |
| release-contracts | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}; ${{ !cancelled() }}` | `node --test scripts/tests/release-closure-authority.test.mjs` | `node scripts/test-catalogue.mjs run --entry node-script-tests` | equal coverage; added discovered contracts justified by catalogue migration (existing files retained; overlay platform_red cases remain explicit) |
| release-contracts | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}; ${{ !cancelled() }}` | `node --test scripts/tests/release-notes-sync.test.mjs` | `node scripts/test-catalogue.mjs run --entry node-script-tests` | equal coverage; added discovered contracts justified by catalogue migration (existing files retained; overlay platform_red cases remain explicit) |
| release-contracts | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}; ${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | equal |
| rust-warnings → report | `caller: always(); success()` | `node scripts/rust-warnings-report.mjs ci-rust-warnings` | `node scripts/rust-warnings-report.mjs ci-rust-warnings` | equal |
| rust-warnings → report | `caller: always(); ${{ failure() && inputs.cancel_release_on_failure }}` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | `gh api --method POST "repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel"` | equal |
| rust-warnings → report | `caller: always(); ${{ always() && inputs.comment }}` | `node scripts/rust-warnings-comment.mjs rust-warnings-report/rust-warnings-report.md` | `node scripts/rust-warnings-comment.mjs rust-warnings-report/rust-warnings-report.md` | equal |
| plugin-metadata | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.result == 'success') }}` | — | `node scripts/test-catalogue.mjs select --tier pr --format json` plus GITHUB_OUTPUT mapping | added: catalogue selection infrastructure |
| plugins | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.plugins != '[]') }}` | — | `node scripts/test-catalogue.mjs select --tier pr --format json` plus GITHUB_OUTPUT mapping | added: catalogue selection infrastructure |
| fmt | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}` | — | `node scripts/test-catalogue.mjs select --tier pr --format json` plus GITHUB_OUTPUT mapping | added: catalogue selection infrastructure |
| ts-static | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}` | — | `node scripts/test-catalogue.mjs select --tier pr --format json` plus GITHUB_OUTPUT mapping | added: catalogue selection infrastructure |
| ts-types | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' &#124;&#124; needs.changed.outputs.rust == 'true') }}` | — | `node scripts/test-catalogue.mjs select --tier pr --format json` plus GITHUB_OUTPUT mapping | added: catalogue selection infrastructure |
| ts-unit | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}` | — | `node scripts/test-catalogue.mjs select --tier pr --format json` plus GITHUB_OUTPUT mapping | added: catalogue selection infrastructure |
| ts-vitest | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}` | — | `node scripts/test-catalogue.mjs select --tier pr --format json` plus GITHUB_OUTPUT mapping | added: catalogue selection infrastructure |
| ts-plugins | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}` | — | `node scripts/test-catalogue.mjs select --tier pr --format json` plus GITHUB_OUTPUT mapping | added: catalogue selection infrastructure |
| release-contracts | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}` | — | `node scripts/test-catalogue.mjs select --tier pr --format json` plus GITHUB_OUTPUT mapping | added: catalogue selection infrastructure |
| release-contracts | `${{ (inputs.phase == '' &#124;&#124; inputs.phase == 'all' &#124;&#124; inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}; !cancelled()` | — | `node scripts/test-catalogue.mjs run --entry core-journeys-offline` | added: previously ungated offline contracts; Orchestrator decision |

| ts-unit | `needs.changed.outputs.frontend == true; !cancelled()` | — | `node scripts/test-catalogue.mjs run --entry ts-unit-runner-tests` | added: previously falsely claimed frontend-release owner; executes under TS change scope (34-39s on Linux) |

The plugin-metadata two-file command is an explicit subset exception to node-script-tests: `plugin-releases.test.mjs` and `ci-scope.test.mjs` run on every PR, including plugin-only changes. The full Node runner stays Rust-scoped in release-contracts; selecting the full runner in plugin-metadata would widen existing skips. `plugins` substitutes only the selected plugin id into the catalogue command. Generated types retain the warning-capture wrapper.

The JSON below is the exact infrastructure/subset exception set used by the drift guard. Run text and step conditions must match; adding arbitrary run steps is rejected. Catalogue commands are compared separately in both directions for every change class, including plugin-only and docs-only PRs.

```json
[
  {
    "job": "changed",
    "run": "set -euo pipefail\nif [ \"$CI_RELEASE_CALL\" = true ] || [ \"${{ github.event_name }}\" != push ]; then\n  echo \"run=true\" >> \"$GITHUB_OUTPUT\"\n  exit 0\nfi\ntree=$(git rev-parse 'HEAD^{tree}')\nfor parent in $(git rev-parse 'HEAD^@'); do\n  if [ \"$(git rev-parse \"$parent^{tree}\")\" = \"$tree\" ]; then\n    echo \"HEAD tree $tree already carried by parent $parent\"\n    echo \"run=false\" >> \"$GITHUB_OUTPUT\"\n    exit 0\n  fi\ndone\necho \"run=true\" >> \"$GITHUB_OUTPUT\"",
    "if": null,
    "reason": "Unchanged tree/scope gate and release cancellation",
    "job_if": "${{ github.event_name != 'pull_request' || !startsWith(github.head_ref, 'release/') }}"
  },
  {
    "job": "changed",
    "run": "node scripts/ci-scope.mjs HEAD^ HEAD",
    "if": "steps.tree.outputs.run == 'true'",
    "reason": "Unchanged tree/scope gate and release cancellation",
    "job_if": "${{ github.event_name != 'pull_request' || !startsWith(github.head_ref, 'release/') }}"
  },
  {
    "job": "changed",
    "run": "gh api --method POST \"repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel\"",
    "if": "${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}",
    "reason": "Unchanged tree/scope gate and release cancellation",
    "job_if": "${{ github.event_name != 'pull_request' || !startsWith(github.head_ref, 'release/') }}"
  },
  {
    "job": "plugin-metadata",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "reason": "Catalogue selection and GITHUB_OUTPUT transport",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.result == 'success') }}"
  },
  {
    "job": "plugin-metadata",
    "run": "node --test scripts/tests/plugin-releases.test.mjs scripts/tests/ci-scope.test.mjs",
    "if": null,
    "reason": "Explicit two-file plugin-metadata subset",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.result == 'success') }}"
  },
  {
    "job": "plugin-metadata",
    "run": "gh api --method POST \"repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel\"",
    "if": "${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}",
    "reason": "Release cancellation (unreachable on PR)",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.result == 'success') }}"
  },
  {
    "job": "plugins",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "reason": "Frozen dependency installation",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.plugins != '[]') }}"
  },
  {
    "job": "plugins",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "reason": "Catalogue selection and GITHUB_OUTPUT transport",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.plugins != '[]') }}"
  },
  {
    "job": "plugins",
    "run": "gh api --method POST \"repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel\"",
    "if": "${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}",
    "reason": "Release cancellation (unreachable on PR)",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.plugins != '[]') }}"
  },
  {
    "job": "fmt",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "reason": "Catalogue selection and GITHUB_OUTPUT transport",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "fmt",
    "run": "if command -v cargo >/dev/null 2>&1; then exit 0; fi\nfor dir in \"${CARGO_HOME:-}/bin\" \"$HOME/.cargo/bin\" /home/*/.cargo/bin /Users/*/.cargo/bin; do\n  if [ -x \"$dir/cargo\" ]; then echo \"$dir\" >> \"$GITHUB_PATH\"; exit 0; fi\ndone\necho \"cargo not found; restore Rust on the runner\" >&2\nexit 1",
    "if": null,
    "reason": "Runner Rust PATH preparation",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "fmt",
    "run": "gh api --method POST \"repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel\"",
    "if": "${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}",
    "reason": "Release cancellation (unreachable on PR)",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "ts-static",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "reason": "Frozen dependency installation",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}"
  },
  {
    "job": "ts-static",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "reason": "Catalogue selection and GITHUB_OUTPUT transport",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}"
  },
  {
    "job": "ts-static",
    "run": "gh api --method POST \"repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel\"",
    "if": "${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}",
    "reason": "Release cancellation (unreachable on PR)",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}"
  },
  {
    "job": "ts-types",
    "run": "if command -v cargo >/dev/null 2>&1; then exit 0; fi\nfor dir in \"${CARGO_HOME:-}/bin\" \"$HOME/.cargo/bin\" /home/*/.cargo/bin /Users/*/.cargo/bin; do\n  if [ -x \"$dir/cargo\" ]; then echo \"$dir\" >> \"$GITHUB_PATH\"; exit 0; fi\ndone\necho \"cargo not found; restore Rust on the runner\" >&2\nexit 1",
    "if": null,
    "reason": "Runner Rust PATH preparation",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "ts-types",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "reason": "Frozen dependency installation",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "ts-types",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "reason": "Catalogue selection and GITHUB_OUTPUT transport",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "ts-types",
    "run": "node -e \"require('fs').writeFileSync('ci-rust-warnings.log', '')\"",
    "if": null,
    "reason": "Warning capture reset",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "ts-types",
    "run": "gh api --method POST \"repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel\"",
    "if": "${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}",
    "reason": "Release cancellation (unreachable on PR)",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "ts-unit",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "reason": "Frozen dependency installation",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}"
  },
  {
    "job": "ts-unit",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "reason": "Catalogue selection and GITHUB_OUTPUT transport",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}"
  },
  {
    "job": "ts-unit",
    "run": "gh api --method POST \"repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel\"",
    "if": "${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}",
    "reason": "Release cancellation (unreachable on PR)",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}"
  },
  {
    "job": "ts-vitest",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "reason": "Frozen dependency installation",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}"
  },
  {
    "job": "ts-vitest",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "reason": "Catalogue selection and GITHUB_OUTPUT transport",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}"
  },
  {
    "job": "ts-vitest",
    "run": "gh api --method POST \"repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel\"",
    "if": "${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}",
    "reason": "Release cancellation (unreachable on PR)",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}"
  },
  {
    "job": "ts-plugins",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "reason": "Frozen dependency installation",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "ts-plugins",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "reason": "Catalogue selection and GITHUB_OUTPUT transport",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "ts-plugins",
    "run": "gh api --method POST \"repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel\"",
    "if": "${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}",
    "reason": "Release cancellation (unreachable on PR)",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "release-contracts",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "reason": "Frozen dependency installation",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "release-contracts",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "reason": "Catalogue selection and GITHUB_OUTPUT transport",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "release-contracts",
    "run": "gh api --method POST \"repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel\"",
    "if": "${{ failure() && inputs.cancel_release_on_failure && inputs.source_ref != '' }}",
    "reason": "Release cancellation (unreachable on PR)",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "rust-warnings → report",
    "run": "node scripts/rust-warnings-report.mjs ci-rust-warnings",
    "if": null,
    "reason": "Existing reusable warning summary",
    "job_if": "always()"
  },
  {
    "job": "rust-warnings → report",
    "run": "gh api --method POST \"repos/$GH_REPO/actions/runs/$GH_RUN_ID/cancel\"",
    "if": "${{ failure() && inputs.cancel_release_on_failure }}",
    "reason": "Existing reusable warning summary",
    "job_if": "always()"
  },
  {
    "job": "rust-warnings → report",
    "run": "node scripts/rust-warnings-comment.mjs rust-warnings-report/rust-warnings-report.md",
    "if": "${{ always() && inputs.comment }}",
    "reason": "Existing reusable warning summary",
    "job_if": "always()"
  },
  {
    "job": "plugin-metadata",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.result == 'success') }}",
    "entry": "plugin-release-contracts",
    "if": null
  },
  {
    "job": "plugins",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.plugins != '[]') }}",
    "entry": "plugin-check",
    "if": null
  },
  {
    "job": "fmt",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}",
    "entry": "rust-fmt",
    "if": null
  },
  {
    "job": "ts-static",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}",
    "entry": "ts-typecheck",
    "if": null
  },
  {
    "job": "ts-static",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}",
    "entry": "ts-unused-exports",
    "if": "${{ !cancelled() }}"
  },
  {
    "job": "ts-static",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}",
    "entry": "ts-knip-report",
    "if": "${{ !cancelled() }}"
  },
  {
    "job": "ts-static",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}",
    "entry": "ts-api-effects",
    "if": "${{ !cancelled() }}"
  },
  {
    "job": "ts-static",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}",
    "entry": "ts-biome",
    "if": "${{ !cancelled() }}"
  },
  {
    "job": "ts-types",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true') }}",
    "entry": "ts-generated-types",
    "if": null
  },
  {
    "job": "ts-unit",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}",
    "entry": "ts-unit-runner-tests",
    "if": "${{ !cancelled() }}"
  },
  {
    "job": "ts-unit",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}",
    "entry": "ts-unit",
    "if": null
  },
  {
    "job": "ts-vitest",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true') }}",
    "entry": "ts-vitest",
    "if": null
  },
  {
    "job": "ts-plugins",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}",
    "entry": "plugin-bundles",
    "if": null
  },
  {
    "job": "ts-plugins",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}",
    "entry": "plugin-tests",
    "if": "${{ !cancelled() }}"
  },
  {
    "job": "release-contracts",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}",
    "entry": "node-script-tests",
    "if": null
  },
  {
    "job": "release-contracts",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}",
    "entry": "python-script-tests",
    "if": "${{ !cancelled() }}"
  },
  {
    "job": "release-contracts",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}",
    "entry": "core-journeys-offline",
    "if": "${{ !cancelled() }}"
  },
  {
    "job": "file-sizes",
    "run": "bash scripts/check-file-sizes.sh --fail",
    "if": null,
    "reason": "Source size gate; runs first and gates every other fast check",
    "job_if": "${{ inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast' }}"
  },
  {
    "job": "ts-types",
    "run": "node scripts/ci-workspace-cleanup.mjs",
    "if": "always()",
    "reason": "Removes job outputs and sweeps old Rust targets",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "release-contracts",
    "run": "node scripts/check-archive-portable-test-paths.mjs",
    "if": null,
    "reason": "Rust test paths stay portable across runners",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "frontend-windows",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "reason": "Catalogue selector",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true') }}"
  },
  {
    "job": "frontend-windows",
    "job_if": "${{ (inputs.phase == '' || inputs.phase == 'all' || inputs.phase == 'fast') && (needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true') }}",
    "entry": "frontend-windows-build",
    "if": null
  }
]
```
