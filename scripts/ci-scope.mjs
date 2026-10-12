import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const pluginTooling = new Set([
  "scripts/ci-scope.mjs", "scripts/check-plugin.mjs", "scripts/release-plugin.mjs",
  "scripts/plugin-release-metadata.mjs", "scripts/sync-plugin-versions.mjs",
  "scripts/build-plugin.mjs", "scripts/build-plugin-ui.mjs", "scripts/plugin-tests.sh",
  "scripts/tests/plugin-releases.test.mjs", "scripts/tests/ci-scope.test.mjs",
  "scripts/tests/sync-plugin-versions-min-codeterm.test.mjs",
  ".github/workflows/plugin-release.yml", ".github/workflows/tests.yml",
  ".github/workflows/cli-source-of-truth.yml",
  ".github/workflows/ready-for-release.yml",
]);

const frontendFiles = new Set([
  "index.html", "vite.config.ts", "vitest.config.ts", "package.json",
  "pnpm-lock.yaml", "pnpm-workspace.yaml", "frontend-release-contract.json",
  "biome.json", "knip.json", "scripts/vitest-owned-tests.json",
  "scripts/run-ts-unit-tests.mjs", "scripts/run-ts-unit-tests.test.mjs",
  "scripts/ts-unit-tests.sh", "scripts/ts-test-assets.mjs", "scripts/ts-test-asset-hook.mjs",
]);

function isFrontendPath(file) {
  return /^(?:src|packages|public|gen)\//.test(file) ||
    /^tsconfig[^/]*\.json$/.test(file) || frontendFiles.has(file) ||
    file.startsWith("scripts/frontend-release/") || file === ".github/workflows/frontend-release.yml";
}

function isRustPath(file) {
  return /^(?:src-tauri|cli|\.cargo)\//.test(file) ||
    /^(?:Cargo\.(?:toml|lock)|build\.(?:sh|ps1)|rust-toolchain(?:\.toml)?|rustfmt\.toml|\.rustfmt\.toml)$/.test(file);
}

export function classifyPaths(files, plugins) {
  const selected = new Set();
  let frontend = false;
  let rust = false;
  for (const file of files) {
    if (file.startsWith("plugins/")) {
      const plugin = plugins.find((entry) => file.startsWith(`plugins/${entry.path}/`));
      if (plugin) selected.add(plugin.id);
      else for (const entry of plugins) selected.add(entry.id);
    } else if (pluginTooling.has(file)) {
      for (const entry of plugins) selected.add(entry.id);
    } else if (isFrontendPath(file)) {
      frontend = true;
    } else if (isRustPath(file)) {
      rust = true;
    } else if (!/^(AGENTS\.md|CLAUDE\.md|(?:prod-docs|essences|\.agents\/skills)\/)/.test(file)) {
      frontend = true;
      rust = true;
    }
  }
  return { frontend, rust, plugins: frontend || rust ? [] : plugins.filter((entry) => selected.has(entry.id)).map(({ id, path }) => ({ id, path })) };
}

// A release PR keeps the fast checks; its tag run owns the heavy Rust and CLI matrix.
export function scopeForRun(files, plugins, fullMatrix = false, releasePr = false) {
  const scope = fullMatrix ? { frontend: true, rust: true, plugins: [] } : classifyPaths(files, plugins);
  return { ...scope, heavy: scope.rust && (fullMatrix || !releasePr) };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const root = path.resolve(import.meta.dirname, "..");
  const [base, head = "HEAD"] = process.argv.slice(2);
  if (!base) throw new Error("Usage: node scripts/ci-scope.mjs BASE [HEAD]");
  const fullMatrix = process.env.CI_FULL_MATRIX === "true";
  const files = fullMatrix ? [] : execFileSync("git", ["diff", "--name-only", base, head], { cwd: root, encoding: "utf8" }).trim().split("\n").filter(Boolean);
  const plugins = JSON.parse(fs.readFileSync(path.join(root, "plugins/channel.json"), "utf8")).plugins;
  const scope = scopeForRun(files, plugins, fullMatrix, process.env.CI_RELEASE_PR === "true");
  if (process.env.GITHUB_OUTPUT) {
    fs.appendFileSync(process.env.GITHUB_OUTPUT, `frontend=${scope.frontend}\nrust=${scope.rust}\nheavy=${scope.heavy}\nplugins=${JSON.stringify(scope.plugins)}\n`);
  }
  console.log(JSON.stringify(scope));
}
