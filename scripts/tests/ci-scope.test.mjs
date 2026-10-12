import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { classifyPaths } from "../ci-scope.mjs";

const plugins = [{ id: "codex", path: "provider-codex" }, { id: "claude", path: "provider-claude" }];
const frontendOnly = { frontend: true, rust: false, plugins: [] };
const rustOnly = { frontend: false, rust: true, plugins: [] };
const both = { frontend: true, rust: true, plugins: [] };

test("frontend-only paths select frontend checks without Rust gates", () => {
  for (const file of [
    "src/App.tsx", "packages/plugin-sdk/src/index.ts", "public/icon.svg", "gen/Message.ts",
    "index.html", "vite.config.ts", "vitest.config.ts", "tsconfig.json", "tsconfig.node.json",
    "package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", "frontend-release-contract.json",
    "scripts/vitest-owned-tests.json", "scripts/run-ts-unit-tests.mjs",
    "scripts/run-ts-unit-tests.test.mjs", "scripts/ts-unit-tests.sh",
    "scripts/ts-test-assets.mjs", "scripts/ts-test-asset-hook.mjs", "biome.json", "knip.json",
    "scripts/frontend-release/archive.mjs", ".github/workflows/frontend-release.yml",
  ]) assert.deepEqual(classifyPaths([file], plugins), frontendOnly, file);
});

test("Rust-only paths select Rust checks", () => {
  for (const file of [
    "src-tauri/src/lib.rs", "src-tauri/crates/codeterm-types/src/lib.rs", "cli/src/main.rs",
    "src-tauri/Cargo.lock", "cli/Cargo.toml", "Cargo.toml", "Cargo.lock", "build.sh", "build.ps1",
    "rust-toolchain", "rust-toolchain.toml", ".cargo/config.toml", "rustfmt.toml", ".rustfmt.toml",
  ]) assert.deepEqual(classifyPaths([file], plugins), rustOnly, file);
});

test("mixed frontend and Rust changes select both scopes regardless of order", () => {
  assert.deepEqual(classifyPaths(["src/App.tsx", "cli/src/main.rs"], plugins), both);
  assert.deepEqual(classifyPaths(["cli/src/main.rs", "src/App.tsx"], plugins), both);
});

test("a plugin-only change selects that plugin without host gates", () => {
  assert.deepEqual(classifyPaths(["plugins/provider-codex/plugin.json", "plugins/provider-codex/src/plugin.ts"], plugins), { frontend: false, rust: false, plugins: [plugins[0]] });
});

test("channel and shared plugin tooling changes verify every plugin", () => {
  for (const file of ["plugins/channel.json", "plugins/provider-shared/updatePrompt.ts", "scripts/release-plugin.mjs", "scripts/ci-scope.mjs", ".github/workflows/tests.yml"]) {
    assert.deepEqual(classifyPaths([file], plugins), { frontend: false, rust: false, plugins });
  }
});

test("host scopes retain plugin suppression in mixed changes", () => {
  for (const [file, expected] of [["src/App.tsx", frontendOnly], ["cli/src/main.rs", rustOnly], ["unknown.txt", both]]) {
    assert.deepEqual(classifyPaths(["plugins/provider-codex/plugin.json", file], plugins), expected);
  }
});

test("docs-only and empty diffs select no host or plugin checks", () => {
  assert.deepEqual(classifyPaths(["AGENTS.md", "CLAUDE.md", "prod-docs/CONFIG.md", "essences/core.md", ".agents/skills/testing/SKILL.md"], plugins), { frontend: false, rust: false, plugins: [] });
  assert.deepEqual(classifyPaths([], plugins), { frontend: false, rust: false, plugins: [] });
});

test("documentation does not broaden plugin checks into host builds", () => {
  assert.deepEqual(classifyPaths(["AGENTS.md", "prod-docs/CONFIG.md", "essences/core.md", "plugins/provider-codex/plugin.json"], plugins), { frontend: false, rust: false, plugins: [plugins[0]] });
});

test("unknown paths conservatively select all host checks", () => {
  for (const file of ["unknown.txt", ".github/workflows/release.yml", "scripts/new-check.mjs", "src-other/module.ts", "tsconfig/other.json"]) {
    assert.deepEqual(classifyPaths([file], plugins), both, file);
  }
});

const workflow = readFileSync(new URL("../../.github/workflows/tests.yml", import.meta.url), "utf8");
const jobConditions = new Map([...workflow.matchAll(/^  ([\w-]+):\n(?:(?!^  [\w-]+:)[\s\S])*?^    if: \$\{\{ (.+) \}\}/gm)].map((match) => [match[1], match[2]]));

function jobRuns(job, scope) {
  const condition = jobConditions.get(job);
  assert.ok(condition, `Missing condition for ${job}`);
  return condition.split(" || ").some((term) => {
    const match = /^needs.changed.outputs.(frontend|rust) == 'true'$/.exec(term);
    assert.ok(match, `Unexpected condition: ${term}`);
    return scope[match[1]];
  });
}

test("workflow wiring skips Rust jobs for frontend diffs and preserves generated types for Rust", () => {
  const frontendJobs = ["ts-static", "ts-unit", "ts-vitest"];
  const rustJobs = ["fmt", "rust-linux", "rust-macos", "rust-windows", "cli-linux", "cli-macos", "cli-windows", "clippy-linux", "ts-plugins", "release-contracts"];
  for (const [scope, frontend, rust] of [[frontendOnly, true, false], [rustOnly, false, true], [both, true, true]]) {
    for (const job of frontendJobs) assert.equal(jobRuns(job, scope), frontend, job);
    for (const job of rustJobs) assert.equal(jobRuns(job, scope), rust, job);
    assert.equal(jobRuns("ts-types", scope), true);
  }
});
