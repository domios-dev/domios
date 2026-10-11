import assert from "node:assert/strict";
import test from "node:test";
import { classifyPaths } from "../ci-scope.mjs";

const plugins = [{ id: "codex", path: "provider-codex" }, { id: "claude", path: "provider-claude" }];

test("a plugin-only change selects that plugin without host gates", () => {
  assert.deepEqual(classifyPaths(["plugins/provider-codex/plugin.json", "plugins/provider-codex/src/plugin.ts"], plugins), { host: false, plugins: [plugins[0]] });
});

test("channel and shared plugin tooling changes verify every plugin", () => {
  for (const file of ["plugins/channel.json", "plugins/provider-shared/updatePrompt.ts", "scripts/release-plugin.mjs"]) {
    assert.deepEqual(classifyPaths([file], plugins), { host: false, plugins });
  }
});

test("host, SDK, lockfile and mixed changes keep the complete host gates", () => {
  for (const file of ["src-tauri/src/lib.rs", "src/App.tsx", "packages/plugin-sdk/src/index.ts", "package.json", "pnpm-lock.yaml", ".github/workflows/release.yml"]) {
    assert.deepEqual(classifyPaths(["plugins/provider-codex/plugin.json", file], plugins), { host: true, plugins: [] });
  }
});

test("documentation does not broaden plugin checks into host builds", () => {
  assert.deepEqual(classifyPaths(["AGENTS.md", "prod-docs/CONFIG.md", "essences/core.md", "plugins/provider-codex/plugin.json"], plugins), { host: false, plugins: [plugins[0]] });
  assert.deepEqual(classifyPaths(["essences/core.md"], plugins), { host: false, plugins: [] });
});
