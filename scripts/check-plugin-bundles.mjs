#!/usr/bin/env node
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
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

const diff = spawnSync("git", ["diff", "--", ...bundlePaths], { cwd: repo, encoding: "utf8" });

if (diff.status !== 0) {
  process.stderr.write(diff.stderr || "git diff failed\n");
  process.exit(diff.status ?? 1);
}

if (diff.stdout.trim()) {
  process.stderr.write(
    "\nPlugin bundles are stale. Run `pnpm build:plugin <plugin-id>` for changed plugins and commit the generated bundle.\n\n",
  );
  process.stdout.write(diff.stdout);
  process.exit(1);
}

const untracked = spawnSync(
  "git",
  ["ls-files", "--others", "--exclude-standard", "--", ...bundlePaths],
  {
  cwd: repo,
  encoding: "utf8",
  },
);

if (untracked.status !== 0) {
  process.stderr.write(untracked.stderr || "git ls-files failed\n");
  process.exit(untracked.status ?? 1);
}

if (untracked.stdout.trim()) {
  process.stderr.write(
    "\nPlugin bundles have untracked generated files. Commit the generated bundle output.\n\n",
  );
  process.stdout.write(untracked.stdout);
  process.exit(1);
}

console.log(`plugin bundles are up to date (${pluginIds.length} checked)`);
