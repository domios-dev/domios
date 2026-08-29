#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(import.meta.url);
const { gitOutput } = require("./build-identity.cjs");
const pluginsDir = resolve(repo, "plugins");

const pluginIds = readdirSync(pluginsDir, { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name)
  .filter((id) => existsSync(resolve(pluginsDir, id, "src", "plugin.ts")))
  .sort();

for (const id of pluginIds) {
  execFileSync(process.execPath, [resolve(repo, "scripts", "build-plugin.mjs"), id], {
    cwd: repo,
    stdio: "inherit",
  });
}

const bundlePaths = ["plugins/*/plugin.js", "plugins/*/ui/index.html", "plugins/*/ui/app-*.js"];

let diff;
try {
  diff = gitOutput(["diff", "--", ...bundlePaths], { cwd: repo, allowEmpty: true });
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exit(1);
}

if (diff) {
  process.stderr.write(
    "\nPlugin bundles are stale. Run `pnpm build:plugin <plugin-id>` for changed plugins and commit the generated bundle.\n\n",
  );
  process.stdout.write(diff);
  process.exit(1);
}

let untracked;
try {
  untracked = gitOutput(["ls-files", "--others", "--exclude-standard", "--", ...bundlePaths], { cwd: repo, allowEmpty: true });
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exit(1);
}

if (untracked) {
  process.stderr.write(
    "\nPlugin bundles have untracked generated files. Commit the generated bundle output.\n\n",
  );
  process.stdout.write(untracked);
  process.exit(1);
}

console.log(`plugin bundles are up to date (${pluginIds.length} checked)`);
