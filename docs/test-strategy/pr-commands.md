# Pull request command parity

Pull requests run the hosted, read-only, secret-free tests-pr workflow with heavy=false. Protected tests-fast and tests-rust jobs qualify trusted pushes only. The fast tier resolves scope once and passes it to the Rust tier. Workflow and shared CI tooling changes conservatively select host checks; plugin release tooling retains plugin-only scope.

The frontend producer owns strict TypeScript, Vite and the warning ratchet. Windows runs a focused workspace symlink and subst path identity fixture with a TS2451 negative control. Static checks retain unused exports, Knip, API effects and Biome. Each command below has one catalogue owner or an explicit infrastructure exception; mutation tests reject unlisted commands and scope drift.

Pushes to rename/domios, develop and master qualify every SHA with the full Rust tier. Nightly runs target rename/domios at 02:17 UTC and develop at 03:17 UTC, sharing per-ref cancellation with pushes. A qualification receipt joins fast checks, full Linux coverage, macOS/Windows subsets, embeddings and warnings. Release prepare requires that exact SHA and publication rechecks it. Version bumps must therefore be committed and qualified on dev before a release branch is cut; merge ancestry or equal trees do not qualify another SHA. Expired or missing receipts require qualification again.

The full suite is pinned to the heavy BLACKSTAR WSL runner; Windows native tests use ROG on an independent physical host. Air owns native macOS then embeddings, with separate feature targets. Ubuntu's native build runner serializes Clippy, fast CLI and generated types. Private guards use host-blackstar and ts-unit uses host-air; other light checks use host-ubunta. Final targets remain per workspace; no mutable target is shared.

The fast CLI lane runs every CLI target except isolated_server. The full Linux CLI lane runs only that real pair test; native platform lanes keep cfg-gated CLI coverage and the pair test on each native OS. Linux platform duplication and release tier polling are removed. Signing, notarization, bundle smoke, signature and served-byte verification stay in platform/publication workflows.

This is source-contract evidence. Fleet execution and wall-time measurements require the Orchestrator's authorized CI run after integration.

```json
[
  {
    "job": "changed",
    "run": "echo \"sha=$(git rev-parse HEAD)\" >> \"$GITHUB_OUTPUT\"",
    "if": null,
    "job_if": "${{ github.event_name != 'pull_request' || !startsWith(github.head_ref, 'release/') }}",
    "reason": "One scope resolver and exact source identity"
  },
  {
    "job": "changed",
    "run": "set -euo pipefail\nif [ \"$CI_RELEASE_CALL\" = true ] || [ \"${{ github.event_name }}\" != push ]; then\n  echo \"run=true\" >> \"$GITHUB_OUTPUT\"\n  exit 0\nfi\ntree=$(git rev-parse 'HEAD^{tree}')\nfor parent in $(git rev-parse 'HEAD^@'); do\n  if [ \"$(git rev-parse \"$parent^{tree}\")\" = \"$tree\" ]; then\n    echo \"HEAD tree $tree already carried by parent $parent\"\n    echo \"run=false\" >> \"$GITHUB_OUTPUT\"\n    exit 0\n  fi\ndone\necho \"run=true\" >> \"$GITHUB_OUTPUT\"",
    "if": null,
    "job_if": "${{ github.event_name != 'pull_request' || !startsWith(github.head_ref, 'release/') }}",
    "reason": "One scope resolver and exact source identity"
  },
  {
    "job": "changed",
    "run": "node scripts/ci-scope.mjs HEAD^ HEAD",
    "if": "steps.tree.outputs.run == 'true'",
    "job_if": "${{ github.event_name != 'pull_request' || !startsWith(github.head_ref, 'release/') }}",
    "reason": "One scope resolver and exact source identity"
  },
  {
    "job": "guards",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "job_if": null,
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "guards",
    "run": "bash scripts/check-file-sizes.sh --fail",
    "if": null,
    "job_if": null,
    "reason": "Runner preparation, compiler evidence and owned cleanup"
  },
  {
    "job": "guards",
    "run": "node scripts/check-installed-identity.mjs",
    "if": "${{ !cancelled() }}",
    "job_if": null,
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "guards",
    "entry": "plugin-release-contracts",
    "if": "${{ !cancelled() }}",
    "job_if": null,
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "guards",
    "run": "node --test scripts/tests/plugin-releases.test.mjs scripts/tests/ci-scope.test.mjs",
    "if": "${{ !cancelled() }}",
    "job_if": null,
    "reason": "Runner preparation, compiler evidence and owned cleanup"
  },
  {
    "job": "guards",
    "run": "if command -v cargo >/dev/null 2>&1; then exit 0; fi\nfor dir in \"${CARGO_HOME:-}/bin\" \"$HOME/.cargo/bin\" /home/*/.cargo/bin /Users/*/.cargo/bin; do\n  if [ -x \"$dir/cargo\" ]; then echo \"$dir\" >> \"$GITHUB_PATH\"; exit 0; fi\ndone\necho \"cargo not found; restore Rust on the runner\" >&2\nexit 1",
    "if": "${{ !cancelled() }}",
    "job_if": null,
    "reason": "Runner preparation, compiler evidence and owned cleanup"
  },
  {
    "job": "guards",
    "run": "bash scripts/check-build-gateway-serialization.sh\nbash scripts/check-build-gateway-policy.sh\nbash scripts/check-build-gateway-mutex-cleanup.sh\nnode scripts/test-catalogue.mjs run --entry node-script-tests-serial",
    "if": "${{ !cancelled() }}",
    "job_if": null,
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "guards",
    "entry": "rust-fmt",
    "if": "${{ !cancelled() && needs.changed.outputs.rust == 'true' }}",
    "job_if": null,
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "guards",
    "run": "node scripts/check-archive-portable-test-paths.mjs",
    "if": "${{ !cancelled() && needs.changed.outputs.rust == 'true' }}",
    "job_if": null,
    "reason": "Runner preparation, compiler evidence and owned cleanup"
  },
  {
    "job": "guards",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": "${{ !cancelled() && needs.changed.outputs.rust == 'true' }}",
    "job_if": null,
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "guards",
    "entry": "node-script-tests",
    "if": "${{ !cancelled() && needs.changed.outputs.rust == 'true' }}",
    "job_if": null,
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "guards",
    "entry": "python-script-tests",
    "if": "${{ !cancelled() && needs.changed.outputs.rust == 'true' }}",
    "job_if": null,
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "guards",
    "entry": "core-journeys-offline",
    "if": "${{ !cancelled() && needs.changed.outputs.rust == 'true' }}",
    "job_if": null,
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "plugins",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "job_if": "${{ needs.changed.outputs.plugins != '[]' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "plugins",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "job_if": "${{ needs.changed.outputs.plugins != '[]' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "plugins",
    "entry": "plugin-check",
    "if": null,
    "job_if": "${{ needs.changed.outputs.plugins != '[]' }}",
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "ts-static",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "ts-static",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "ts-static",
    "entry": "ts-unused-exports",
    "if": "${{ !cancelled() }}",
    "job_if": "${{ needs.changed.outputs.frontend == 'true' }}",
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "ts-static",
    "entry": "ts-knip-report",
    "if": "${{ !cancelled() }}",
    "job_if": "${{ needs.changed.outputs.frontend == 'true' }}",
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "ts-static",
    "entry": "ts-api-effects",
    "if": "${{ !cancelled() }}",
    "job_if": "${{ needs.changed.outputs.frontend == 'true' }}",
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "ts-static",
    "entry": "ts-biome",
    "if": "${{ !cancelled() }}",
    "job_if": "${{ needs.changed.outputs.frontend == 'true' }}",
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "frontend-dist",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "frontend-dist",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "frontend-dist",
    "entry": "ts-typecheck",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true' }}",
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "frontend-windows",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "frontend-windows",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "frontend-windows",
    "entry": "frontend-windows-build",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true' }}",
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "ts-types",
    "run": "if command -v cargo >/dev/null 2>&1; then exit 0; fi\nfor dir in \"${CARGO_HOME:-}/bin\" \"$HOME/.cargo/bin\" /home/*/.cargo/bin /Users/*/.cargo/bin; do\n  if [ -x \"$dir/cargo\" ]; then echo \"$dir\" >> \"$GITHUB_PATH\"; exit 0; fi\ndone\necho \"cargo not found; restore Rust on the runner\" >&2\nexit 1",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true' }}",
    "reason": "Runner preparation, compiler evidence and owned cleanup"
  },
  {
    "job": "ts-types",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "ts-types",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "ts-types",
    "run": "node -e \"require('fs').writeFileSync('ci-rust-warnings.log', '')\"",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true' }}",
    "reason": "Runner preparation, compiler evidence and owned cleanup"
  },
  {
    "job": "ts-types",
    "entry": "ts-generated-types",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true' }}",
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "ts-types",
    "run": "bash build.sh cache-release",
    "if": "always()",
    "job_if": "${{ needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true' }}",
    "reason": "Runner preparation, compiler evidence and owned cleanup"
  },
  {
    "job": "ts-types",
    "run": "node scripts/ci-workspace-cleanup.mjs",
    "if": "always()",
    "job_if": "${{ needs.changed.outputs.frontend == 'true' || needs.changed.outputs.rust == 'true' }}",
    "reason": "Runner preparation, compiler evidence and owned cleanup"
  },
  {
    "job": "ts-unit",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "ts-unit",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "ts-unit",
    "entry": "ts-unit",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' }}",
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "ts-unit",
    "entry": "ts-unit-runner-tests",
    "if": "${{ !cancelled() }}",
    "job_if": "${{ needs.changed.outputs.frontend == 'true' }}",
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "ts-vitest",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "ts-vitest",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "ts-vitest",
    "entry": "ts-vitest",
    "if": null,
    "job_if": "${{ needs.changed.outputs.frontend == 'true' }}",
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "ts-plugins",
    "run": "pnpm install --frozen-lockfile --store-dir \"$RUNNER_TOOL_CACHE/pnpm-store\"",
    "if": null,
    "job_if": "${{ needs.changed.outputs.rust == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "ts-plugins",
    "run": "node scripts/test-catalogue.mjs select --tier pr --format json > \"$RUNNER_TEMP/pr-catalogue.json\"\nnode -e 'const fs = require(\"node:fs\"); const selected = JSON.parse(fs.readFileSync(process.env.RUNNER_TEMP + \"/pr-catalogue.json\", \"utf8\")); fs.appendFileSync(process.env.GITHUB_OUTPUT, \"commands=\" + JSON.stringify(Object.fromEntries(selected.runners.map(entry => [entry.id, entry.runner]))) + \"\\n\");'",
    "if": null,
    "job_if": "${{ needs.changed.outputs.rust == 'true' }}",
    "reason": "Frozen dependencies and catalogue command selection"
  },
  {
    "job": "ts-plugins",
    "entry": "plugin-bundles",
    "if": null,
    "job_if": "${{ needs.changed.outputs.rust == 'true' }}",
    "reason": "Catalogue command ownership and change scope"
  },
  {
    "job": "ts-plugins",
    "entry": "plugin-tests",
    "if": "${{ !cancelled() }}",
    "job_if": "${{ needs.changed.outputs.rust == 'true' }}",
    "reason": "Catalogue command ownership and change scope"
  }
]
```
