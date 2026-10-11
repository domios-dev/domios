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
]);

export function classifyPaths(files, plugins) {
  const selected = new Set();
  let host = false;
  for (const file of files) {
    if (file.startsWith("plugins/")) {
      const plugin = plugins.find((entry) => file.startsWith(`plugins/${entry.path}/`));
      if (plugin) selected.add(plugin.id);
      else for (const entry of plugins) selected.add(entry.id);
    } else if (pluginTooling.has(file)) {
      for (const entry of plugins) selected.add(entry.id);
    } else if (!/^(AGENTS\.md|CLAUDE\.md|(?:prod-docs|essences|\.agents\/skills)\/)/.test(file)) {
      host = true;
    }
  }
  return { host, plugins: host ? [] : plugins.filter((entry) => selected.has(entry.id)).map(({ id, path }) => ({ id, path })) };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const root = path.resolve(import.meta.dirname, "..");
  const [base, head = "HEAD"] = process.argv.slice(2);
  if (!base) throw new Error("Usage: node scripts/ci-scope.mjs BASE [HEAD]");
  const files = execFileSync("git", ["diff", "--name-only", base, head], { cwd: root, encoding: "utf8" }).trim().split("\n").filter(Boolean);
  const plugins = JSON.parse(fs.readFileSync(path.join(root, "plugins/channel.json"), "utf8")).plugins;
  const scope = classifyPaths(files, plugins);
  if (process.env.GITHUB_OUTPUT) {
    fs.appendFileSync(process.env.GITHUB_OUTPUT, `host=${scope.host}\nplugins=${JSON.stringify(scope.plugins)}\n`);
  }
  console.log(JSON.stringify(scope));
}
