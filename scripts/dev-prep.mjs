#!/usr/bin/env node
import { existsSync, statSync, utimesSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const repo = join(dirname(fileURLToPath(import.meta.url)), "..");
const isWin = process.platform === "win32";
const pnpm = isWin ? "pnpm.cmd" : "pnpm";
const cargo = isWin ? "cargo.exe" : "cargo";
const node = process.execPath;

function run(cmd, args, opts = {}) {
  const res = spawnSync(cmd, args, {
    cwd: repo,
    stdio: "inherit",
    env: process.env,
    shell: isWin && cmd.endsWith(".cmd"),
    ...opts,
  });
  if (res.error) throw res.error;
  if (res.status !== 0) process.exit(res.status ?? 1);
}

function output(cmd, args) {
  const res = spawnSync(cmd, args, {
    cwd: repo,
    encoding: "utf8",
    env: process.env,
  });
  if (res.error) throw res.error;
  if (res.status !== 0) process.exit(res.status ?? 1);
  return res.stdout;
}

function touch(path) {
  const full = join(repo, path);
  const now = new Date();
  utimesSync(full, now, now);
}

const host = output(cargo, ["-vV"])
  .split(/\r?\n/)
  .find((line) => line.startsWith("host:"))
  ?.split(":")[1]
  ?.trim();

if (!host) {
  console.error("[dev-prep] failed to detect cargo host triple");
  process.exit(1);
}

const embedBundle = join(
  repo,
  "src-tauri",
  "bundled-embed",
  `codeterm-embed-${host}${isWin ? ".exe" : ""}`,
);

touch("cli/src/main.rs");
run(cargo, ["build", "--release", "--manifest-path", "cli/Cargo.toml"]);
// Dev bundling: ad-hoc sign the CLI, never require a Developer ID identity.
process.env.CODETERM_DEV = "1";
run(node, ["scripts/bundle-cli.js"]);
run(node, ["scripts/prepare-embed-stub.js"]);

const embedSize = existsSync(embedBundle) ? statSync(embedBundle).size : 0;
if (process.env.REBUILD_EMBED === "1" || embedSize < 1_048_576) {
  console.log(`[dev-prep] building codeterm-embed (size=${embedSize}, REBUILD_EMBED=${process.env.REBUILD_EMBED ?? "0"})`);
  run(cargo, ["build", "--release", "--manifest-path", "src-tauri/Cargo.toml", "--bin", "codeterm-embed"]);
  run(node, ["scripts/bundle-embed.js"]);
} else {
  console.log(`[dev-prep] skipping codeterm-embed rebuild (bundle exists, ${embedSize} bytes; set REBUILD_EMBED=1 to force)`);
}

run(pnpm, ["dev"]);
