import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { bumpPlugin, channelPlugins, checkPluginTag, compareVersions } from "../plugin-release-metadata.mjs";

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-plugin-release-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const write = (file, value) => {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), typeof value === "string" ? value : JSON.stringify(value));
  };
  write("package.json", { version: "4.0.0" });
  write("src-tauri/tauri.conf.json", { version: "4.0.0" });
  for (const directory of ["src-tauri", "src-tauri/crates/codeterm-service-core", "cli"]) write(`${directory}/Cargo.toml`, '[package]\nversion = "4.0.0"\n');
  for (const directory of ["chat-core", "chat-ui", "chat-engine", "plugin-sdk"]) write(`packages/${directory}/package.json`, { version: "4.0.0" });
  const plugins = ["claude", "codex", "opencode"].map((id) => ({ id, path: `provider-${id}`, version: "7.1.0", minCodeterm: "3.2.0" }));
  write("plugins/channel.json", { plugins });
  for (const plugin of plugins) {
    write(`plugins/${plugin.path}/plugin.json`, { id: plugin.id, version: plugin.version, minCodeterm: plugin.minCodeterm });
    write(`plugins/${plugin.path}/package.json`, { version: plugin.version });
  }
  fs.mkdirSync(path.join(root, "scripts"));
  for (const script of ["sync-plugin-versions.mjs", "plugin-release-metadata.mjs"]) {
    fs.copyFileSync(path.resolve(import.meta.dirname, "..", script), path.join(root, "scripts", script));
  }
  return { root, write, read: (file) => JSON.parse(fs.readFileSync(path.join(root, file), "utf8")) };
}

test("an app bump preserves independent provider versions and host requirements", (t) => {
  const { root, read } = fixture(t);
  const before = channelPlugins(root);
  execFileSync(process.execPath, [path.join(root, "scripts/sync-plugin-versions.mjs"), "4.1.0"]);
  execFileSync(process.execPath, [path.join(root, "scripts/sync-plugin-versions.mjs"), "--check"]);
  assert.equal(read("package.json").version, "4.1.0");
  assert.equal(read("packages/chat-ui/package.json").version, "4.1.0");
  assert.deepEqual(channelPlugins(root), before);
});

test("a plugin bump changes only that plugin and preserves its minimum host", (t) => {
  const { root, read } = fixture(t);
  bumpPlugin(root, "codex", "7.1.1");
  const codex = checkPluginTag(root, "plugin-codex-v7.1.1");
  assert.equal(codex.minCodeterm, "3.2.0");
  assert.equal(codex.packageJson.version, "7.1.1");
  assert.equal(read("package.json").version, "4.0.0");
  assert.equal(channelPlugins(root).find((entry) => entry.id === "claude").version, "7.1.0");
});

test("a new host requirement is explicit and agrees in manifest and channel", (t) => {
  const { root } = fixture(t);
  bumpPlugin(root, "opencode", "7.2.0", "4.0.0");
  assert.equal(checkPluginTag(root, "plugin-opencode-v7.2.0").manifest.minCodeterm, "4.0.0");
});

test("metadata checks reject channel, package and manifest disagreement", (t) => {
  const { root, write, read } = fixture(t);
  write("plugins/provider-codex/package.json", { version: "7.0.0" });
  assert.throws(() => channelPlugins(root));
  assert.throws(() => bumpPlugin(root, "claude", "7.1.1"));
  assert.equal(read("plugins/provider-claude/plugin.json").version, "7.1.0");
});

test("release refuses downgrades, equal versions and mismatched tags", (t) => {
  const { root } = fixture(t);
  assert.throws(() => bumpPlugin(root, "codex", "7.0.0"));
  assert.throws(() => bumpPlugin(root, "codex", "7.1.0"));
  assert.throws(() => checkPluginTag(root, "release-7.1.0"));
  assert.throws(() => checkPluginTag(root, "plugin-codex-v7.1.1"));
  assert.throws(() => bumpPlugin(root, "../codex", "7.1.1"));
});

test("release orders stable and prerelease versions using semver precedence", () => {
  assert.equal(compareVersions("1.14.2", "1.14.1"), 1);
  assert.equal(compareVersions("1.14.2", "1.14.2-rc.1"), 1);
  assert.equal(compareVersions("1.14.2-rc.10", "1.14.2-rc.2"), 1);
  assert.equal(compareVersions("1.14.2+build", "1.14.2"), 0);
  assert.throws(() => compareVersions("1.14.2-rc.01", "1.14.2"));
});
