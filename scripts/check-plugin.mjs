import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { channelPlugins } from "./plugin-release-metadata.mjs";

const root = path.resolve(import.meta.dirname, "..");
const plugin = channelPlugins(root).find((entry) => entry.id === process.argv[2]);
if (!plugin) throw new Error("Usage: node scripts/check-plugin.mjs <official-plugin-id>");

function node(...args) {
  execFileSync(process.execPath, args, { cwd: root, stdio: "inherit" });
}

function tests(directory) {
  if (!fs.existsSync(directory)) return [];
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(directory, entry.name);
    return entry.isDirectory() ? tests(file) : /\.test\.[cm]?[jt]sx?$/.test(entry.name) ? [file] : [];
  }).sort();
}

node("scripts/build-plugin.mjs", plugin.id);
const assets = [`plugins/${plugin.path}/plugin.js`, `plugins/${plugin.path}/ui/index.html`, `plugins/${plugin.path}/ui/app-*.js`];
execFileSync("git", ["diff", "--exit-code", "--", ...assets], { cwd: root, stdio: "inherit" });
const untracked = execFileSync("git", ["ls-files", "--others", "--exclude-standard", "--", ...assets], { cwd: root, encoding: "utf8" }).trim();
if (untracked) throw new Error(`Commit generated plugin assets before publication:\n${untracked}`);

const temp = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-plugin-types-"));
try {
  const config = path.join(temp, "tsconfig.json");
  fs.writeFileSync(config, JSON.stringify({
    extends: path.join(root, "tsconfig.json"),
    include: [path.join(plugin.directory, "src/plugin.ts"), path.join(plugin.directory, "ui/src/**/*.tsx"), path.join(plugin.directory, "ui/src/**/*.ts")],
    exclude: ["**/*.test.ts", "**/*.test.tsx"],
    references: [],
  }));
  node("node_modules/typescript/bin/tsc", "--project", config, "--noEmit");
} finally {
  fs.rmSync(temp, { recursive: true, force: true });
}

const ownedTests = tests(path.join(plugin.directory, "tests"));
for (const file of ownedTests) node("node_modules/tsx/dist/cli.mjs", file);
console.log(`${plugin.id} ${plugin.version}: metadata, bundle, types and ${ownedTests.length} test files passed`);
