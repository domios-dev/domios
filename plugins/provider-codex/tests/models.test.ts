/**
 * Codex model discovery tests cover live cache, missing-cache reporting, and catalog freshness.
 * Run: npx tsx plugins/provider-codex/tests/models.test.ts
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
  const b = JSON.stringify(expected);
  assert(a === b, name, `${a} !== ${b}`);
}

function loadCodexWithHost(host: unknown) {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

const manifestSeed = {
  models: [
    { id: "seed-model", displayName: "Seed Model", description: "Manifest fallback" },
  ],
};

const bundledManifest = JSON.parse(fs.readFileSync(
  path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.json"),
  "utf8",
)) as { models?: Array<{ id: string; description?: string }>; defaultModel?: string };

eq(bundledManifest.models?.[0]?.id, "gpt-6.1-sol", "new Codex model leads the seed catalogue");
eq(bundledManifest.defaultModel, "gpt-6.1-sol", "new priority-one workhorse is the default");
eq(
  bundledManifest.models?.find((model) => model.id === "gpt-6-sol")?.description,
  "Previous generation workhorse model.",
  "previous generation description matches the current catalogue",
);

const baseHost = (over: Record<string, unknown>) => ({
  manifestJson: () => JSON.stringify(manifestSeed),
  homeDir: () => "/tmp/home",
  platform: () => "linux",
  unixNowMs: () => 1_000_000,
  path: { equal: (a: string, b: string) => a === b },
  expandHome: (v: string) => v.replace(/^~/, "/tmp/home"),
  envGet: () => null,
  log: () => {},
  shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
  fs: {
    fileExists: () => false,
    readFile: () => null,
    readFileHead: () => null,
    readFileTail: () => null,
    readJson: () => null,
    writeJson: () => true,
    writeFile: () => true,
    writeTempFile: () => "/tmp/codeterm task.md",
    walkDir: () => [],
  },
  ...over,
});

console.log("\n--- discoverModels ---\n");

{
  const p = loadCodexWithHost(baseHost({ manifestJson: () => JSON.stringify(bundledManifest) }));
  const manifestIds = (bundledManifest.models || []).map((model) => model.id);
  eq(p.discoverModels(), null, "a missing live cache reports no discovery so the host applies its seed");
  assert(!manifestIds.includes("gpt-5.4-mini"), "a removed model is absent from launch choices");
}

{
  const p = loadCodexWithHost(baseHost({
    fs: {
      fileExists: (p: string) => p.endsWith("/.codex/models_cache.json"),
      readFile: () => null,
      readJson: (p: string) => p.endsWith("/.codex/models_cache.json")
        ? { models: [{ slug: "gpt-live", display_name: "GPT Live", description: "from cache" }] }
        : null,
      writeJson: () => true,
      writeFile: () => true,
    },
  }));
  eq(
    p.discoverModels(),
    [{ id: "gpt-live", displayName: "GPT Live", description: "from cache" }],
    "discoverModels surfaces the stubbed Codex cache probe",
  );
}

{
  const cache = JSON.parse(fs.readFileSync(
    path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "models_cache.json"),
    "utf8",
  )) as { models: unknown[] };
  const p = loadCodexWithHost(baseHost({
    fs: {
      fileExists: (p: string) => p.endsWith("/.codex/models_cache.json"),
      readFile: () => null,
      readJson: (p: string) => p.endsWith("/.codex/models_cache.json") ? cache : null,
      writeJson: () => true,
      writeFile: () => true,
    },
  }));
  const models = p.discoverModels() as Array<{
    id: string;
    reasoningEfforts?: Array<{ id: string; displayName: string; description?: string }>;
    defaultReasoningEffort?: string;
  }>;
  eq(
    models.map((model) => [model.id, model.reasoningEfforts?.map((effort) => effort.id)]),
    [
      ["gpt-6.1-sol", ["low", "medium", "high", "xhigh", "max", "ultra"]],
      ["gpt-6-luna", ["low", "medium", "high", "xhigh", "max"]],
      ["gpt-6-sol", ["low", "medium", "high", "xhigh", "max", "ultra"]],
      ["gpt-5.5", ["low", "medium", "high", "xhigh"]],
      ["gpt-5.6-sol", ["low", "medium", "high", "xhigh"]],
    ],
    "new cache entries are discovered in cache priority order with per-model reasoning",
  );
  assert(!models.some((model) => model.id === "gpt-reserve"), "hidden cache entries stay hidden");
  eq(
    models.find((model) => model.id === "gpt-6.1-sol")?.defaultReasoningEffort,
    "low",
    "new default model carries its cache default reasoning effort",
  );
  eq(
    models.find((model) => model.id === "gpt-5.6-sol")?.defaultReasoningEffort,
    "low",
    "live per-model default reasoning effort is carried from cache",
  );
}

{
  const cache = {
    models: [
      { slug: "gpt-empty", supported_reasoning_levels: [] },
      { slug: "gpt-invalid", supported_reasoning_levels: [{}, { effort: 12 }, { effort: " " }] },
    ],
  };
  const p = loadCodexWithHost(baseHost({
    fs: {
      fileExists: (p: string) => p.endsWith("/.codex/models_cache.json"),
      readFile: () => null,
      readJson: (p: string) => p.endsWith("/.codex/models_cache.json") ? cache : null,
      writeJson: () => true,
      writeFile: () => true,
    },
  }));
  const models = p.discoverModels() as Array<{ reasoningEfforts?: unknown }>;
  eq(
    models.map((model) => model.reasoningEfforts),
    [undefined, undefined],
    "empty or wholly invalid cache levels leave per-model support absent for provider fallback",
  );
}

{
  const p = loadCodexWithHost(baseHost({}));
  eq(p.discoverModels(), null, "discoverModels reports no discovery when no Codex cache exists");
}

// Live cache → clean, current, priority-ordered chat models: hidden + non-chat dropped.
{
  const cache = {
    models: [
      { slug: "gpt-5.4", display_name: "GPT-5.4", description: "everyday", visibility: "list", priority: 16 },
      { slug: "codex-auto-review", display_name: "Auto Review", visibility: "hide", priority: 43 },
      { slug: "text-embedding-3-large", display_name: "Embeds", visibility: "list", priority: 5 },
      { slug: "gpt-5.5", display_name: "GPT-5.5", description: "frontier", visibility: "list", priority: 9 },
    ],
  };
  const p = loadCodexWithHost(baseHost({
    fs: {
      fileExists: (p: string) => p.endsWith("/.codex/models_cache.json"),
      readFile: () => null,
      readJson: (p: string) => p.endsWith("/.codex/models_cache.json") ? cache : null,
      writeJson: () => true,
      writeFile: () => true,
    },
  }));
  const models = p.discoverModels() as Array<{ id: string; available?: boolean }>;
  eq(models.map((m) => m.id), ["gpt-5.5", "gpt-5.4"], "visibility:hide + embedding dropped; sorted by ascending priority");
  assert(models.every((m) => m.available === undefined), "codex has no plan-lockout signal → available unset");
}

console.log("\n--- discoverModels: fallback trigger conditions ---\n");

{
  // Readable cache must win outright: the manifest fallback is never even consulted.
  let manifestCalls = 0;
  const cache = { models: [{ slug: "alpha-model", display_name: "Alpha", visibility: "list", priority: 1 }] };
  const p = loadCodexWithHost(baseHost({
    manifestJson: () => { manifestCalls++; return JSON.stringify(manifestSeed); },
    fs: {
      fileExists: (p: string) => p.endsWith("/.codex/models_cache.json"),
      readFile: () => null,
      readJson: (p: string) => p.endsWith("/.codex/models_cache.json") ? cache : null,
      writeJson: () => true,
      writeFile: () => true,
    },
  }));
  const models = p.discoverModels() as Array<{ id: string }>;
  eq(models.map((m) => m.id), ["alpha-model"], "readable cache wins over the manifest fallback");
  eq(manifestCalls, 0, "manifest fallback is never consulted while the cache is readable");
}

{
  // No cache file on disk -> fallback path is reached.
  let manifestCalls = 0;
  const p = loadCodexWithHost(baseHost({
    manifestJson: () => { manifestCalls++; return JSON.stringify(manifestSeed); },
    fs: {
      fileExists: () => false,
      readFile: () => null,
      readJson: () => null,
      writeJson: () => true,
      writeFile: () => true,
    },
  }));
  eq(p.discoverModels(), null, "missing cache file reports no discovery; the host applies its seed");
  eq(manifestCalls, 0, "the plugin never reads the manifest seed itself");
}

{
  // Cache file exists but is corrupt/unreadable (readJson returns null) -> fallback path is reached.
  let manifestCalls = 0;
  const p = loadCodexWithHost(baseHost({
    manifestJson: () => { manifestCalls++; return JSON.stringify(manifestSeed); },
    fs: {
      fileExists: (p: string) => p.endsWith("/.codex/models_cache.json"),
      readFile: () => null,
      readJson: () => null,
      writeJson: () => true,
      writeFile: () => true,
    },
  }));
  eq(p.discoverModels(), null, "unreadable cache reports no discovery; the host applies its seed");
  eq(manifestCalls, 0, "the plugin never reads the manifest seed for an unreadable cache");
}

console.log("\n--- discoverModels: cache priority order wins over array position ---\n");
{
  const cache = {
    models: [
      { slug: "model-c", display_name: "Model C", visibility: "list", priority: 30 },
      { slug: "model-a", display_name: "Model A", visibility: "list", priority: 5 },
      { slug: "model-d", display_name: "Model D", visibility: "list", priority: 100 },
      { slug: "model-b", display_name: "Model B", visibility: "list", priority: 12 },
    ],
  };
  const p = loadCodexWithHost(baseHost({
    fs: {
      fileExists: (p: string) => p.endsWith("/.codex/models_cache.json"),
      readFile: () => null,
      readJson: (p: string) => p.endsWith("/.codex/models_cache.json") ? cache : null,
      writeJson: () => true,
      writeFile: () => true,
    },
  }));
  const models = p.discoverModels() as Array<{ id: string }>;
  eq(
    models.map((m) => m.id),
    ["model-a", "model-b", "model-c", "model-d"],
    "returned order follows ascending cache priority, not array position",
  );
}


console.log(`
${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
