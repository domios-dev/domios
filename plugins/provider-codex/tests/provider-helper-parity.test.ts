/**
 * Cross-provider QuickJS helper parity guard.
 * Run: npx tsx plugins/provider-codex/tests/provider-helper-parity.test.ts
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}${detail ? `: ${detail}` : ""}`); }
}

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}, got ${a}`);
}

function loadPlugin(relativePluginPath: string) {
  const here = path.dirname(fileURLToPath(import.meta.url));
  const pluginPath = path.resolve(here, relativePluginPath);
  const source = fs.readFileSync(pluginPath, "utf8") + `\nmodule.exports.__parity = { validateEntry };\n`;
  const context = {
    host: {
      manifestJson: () => "null",
      homeDir: () => "/tmp",
      path: { equal: () => true },
      platform: () => "linux",
      unixNowMs: () => 0,
      envGet: () => null,
      shell: {
        quoteFor: (value: unknown) => String(value),
        quote: (value: unknown) => String(value),
      },
      fs: {
        fileExists: () => false,
        readFile: () => null,
        readJson: () => null,
        walkDir: () => [],
        writeJson: () => true,
        writeTempFile: () => "/tmp/task",
      },
      expandHome: (value: string) => value,
      writeFile: () => true,
      removeFile: () => true,
      fileExists: () => false,
      log: () => {},
    },
    console,
    module: { exports: {} as Record<string, any> },
    exports: {},
  };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports;
}

const claudeModule = loadPlugin("../../provider-claude/plugin.js");
const codexModule = loadPlugin("../plugin.js");
const claude = claudeModule.default;
const codex = codexModule.default;

console.log("\n--- cross-provider helper parity ---\n");

const noiseCases = [
  "",
  "   ",
  "────",
  "═══",
  "hello world",
  "function main() {}",
];

for (const input of noiseCases) {
  eq(
    claude.isOutputNoise(input),
    codex.isOutputNoise(input),
    `isOutputNoise parity: ${JSON.stringify(input)}`,
  );
}

const invalidEntries = [
  null,
  undefined,
  {},
  { type: "unknown" },
  ["nested", ["array"]],
  { content: 42 },
];

for (const input of invalidEntries) {
  eq(
    claudeModule.__parity.validateEntry(input),
    codexModule.__parity.validateEntry(input),
    `validateEntry invalid-edge parity: ${JSON.stringify(input)}`,
  );
}

const invalidChunks = [
  "",
  "not json",
  JSON.stringify(["nested", ["array"]]),
  JSON.stringify({ content: 42 }),
  JSON.stringify({ type: "unknown" }),
];

for (const chunk of invalidChunks) {
  const originalWarn = console.warn;
  try {
    console.warn = () => {};
    eq(
      claude.parseSessionDelta(chunk).messages.length,
      codex.parseSessionDelta(chunk).messages.length,
      `parseSessionDelta invalid-edge parity: ${JSON.stringify(chunk)}`,
    );
  } finally {
    console.warn = originalWarn;
  }
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
