/**
 * Tests for the pure OpenCode config writer.
 * Run: npx tsx plugins/provider-opencode/tests/opencodeConfig.test.ts
 */

import {
  applyDomiosProviders,
  providerKeyEnvName,
  type DomiosProvider,
} from "../src/opencodeConfig";

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}${detail ? `: ${detail}` : ""}`);
  }
}

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}, got ${a}`);
}

console.log("\n--- applyDomiosProviders: add owned entries ---\n");
{
  const provider: DomiosProvider = {
    id: "myllm",
    kind: "openai",
    baseUrl: "https://api.example.com/v1",
    keyEnv: "MYLLM_API_KEY",
    models: ["model-a", "model-b"],
    name: "My LLM",
  };
  const { config, changed } = applyDomiosProviders({}, [provider]);
  assert(changed === true, "changed is true when adding a new provider");
  const cfg = config as { provider: Record<string, Record<string, unknown>> };
  const entry = cfg.provider["myllm"];
  eq(entry.npm, "@ai-sdk/openai-compatible", "openai kind uses openai-compatible npm");
  eq(entry.name, "My LLM", "name is set");
  eq(entry.options, { baseURL: "https://api.example.com/v1", apiKey: "{env:MYLLM_API_KEY}" }, "options has baseURL and env apiKey");
  eq(entry.models, { "model-a": {}, "model-b": {} }, "models are set");
  eq(entry["x-domios"], true, "ownership marker is set");
}

console.log("\n--- applyDomiosProviders: anthropic kind ---\n");
{
  const provider: DomiosProvider = {
    id: "claude",
    kind: "anthropic",
    baseUrl: "https://api.anthropic.com",
    keyEnv: "ANTHROPIC_KEY",
  };
  const { config } = applyDomiosProviders({}, [provider]);
  const cfg = config as { provider: Record<string, Record<string, unknown>> };
  eq(cfg.provider["claude"].npm, "@ai-sdk/anthropic", "anthropic kind uses anthropic npm");
}

console.log("\n--- applyDomiosProviders: lmstudio kind ---\n");
{
  const provider: DomiosProvider = {
    id: "local",
    kind: "lmstudio",
    baseUrl: "http://localhost:1234/v1",
  };
  const { config } = applyDomiosProviders({}, [provider]);
  const cfg = config as { provider: Record<string, Record<string, unknown>> };
  eq(cfg.provider["local"].npm, "@ai-sdk/openai-compatible", "lmstudio kind uses openai-compatible npm");
  eq(cfg.provider["local"].options, { baseURL: "http://localhost:1234/v1" }, "no apiKey when keyEnv is missing");
}

console.log("\n--- applyDomiosProviders: update existing owned entry ---\n");
{
  const provider: DomiosProvider = {
    id: "myllm",
    kind: "openai",
    baseUrl: "https://new.example.com/v1",
    keyEnv: "NEW_KEY",
    models: ["m1"],
    name: "Updated",
  };
  const initial = applyDomiosProviders({}, [provider]);
  const updated = applyDomiosProviders(initial.config, [provider]);
  assert(updated.changed === false, "idempotent second apply gives changed=false");
}

console.log("\n--- applyDomiosProviders: remove owned entries no longer in providers ---\n");
{
  const p1: DomiosProvider = { id: "a", kind: "openai", baseUrl: "https://a.com" };
  const p2: DomiosProvider = { id: "b", kind: "openai", baseUrl: "https://b.com" };
  const initial = applyDomiosProviders({}, [p1, p2]);
  const cfg1 = initial.config as { provider: Record<string, unknown> };
  assert("a" in cfg1.provider && "b" in cfg1.provider, "both providers present initially");
  const removed = applyDomiosProviders(initial.config, [p1]);
  const cfg2 = removed.config as { provider: Record<string, unknown> };
  assert("a" in cfg2.provider, "kept provider remains");
  assert(!("b" in cfg2.provider), "removed provider is gone");
  assert(removed.changed === true, "changed is true when removing");
}

console.log("\n--- applyDomiosProviders: foreign entries preserved ---\n");
{
  const foreignEntry = { npm: "@ai-sdk/some-other", name: "Foreign", options: { baseURL: "https://foreign.com" } };
  const config = { provider: { foreign: foreignEntry } };
  const provider: DomiosProvider = { id: "mine", kind: "openai", baseUrl: "https://mine.com" };
  const { config: result, conflicts } = applyDomiosProviders(config, [provider]);
  const cfg = result as { provider: Record<string, unknown> };
  eq(cfg.provider["foreign"], foreignEntry, "foreign entry is byte-identical");
  assert("mine" in cfg.provider, "owned entry added alongside foreign");
  eq(conflicts, [], "conflicts is empty in the normal case");
}

console.log("\n--- applyDomiosProviders: foreign entry with same id is not overwritten ---\n");
{
  const foreignEntry = { npm: "@ai-sdk/some-other", name: "Foreign", options: { baseURL: "https://foreign.com" } };
  const config = { provider: { myllm: foreignEntry } };
  const provider: DomiosProvider = { id: "myllm", kind: "openai", baseUrl: "https://mine.com" };
  const { config: result, conflicts } = applyDomiosProviders(config, [provider]);
  const cfg = result as { provider: Record<string, unknown> };
  eq(cfg.provider["myllm"], foreignEntry, "foreign entry with same id stays identical");
  eq(conflicts, ["myllm"], "conflict id is reported");
}

console.log("\n--- applyDomiosProviders: no literal secret ever ---\n");
{
  const provider: DomiosProvider = {
    id: "secret",
    kind: "openai",
    baseUrl: "https://api.example.com",
    keyEnv: "SECRET_KEY",
  };
  const { config } = applyDomiosProviders({}, [provider]);
  const json = JSON.stringify(config);
  // Check that no value looks like an api key (long random string)
  const apiKeyPattern = /sk-[a-zA-Z0-9]{20,}/;
  assert(!apiKeyPattern.test(json), "no literal api key in output");
  // Check that apiKey is only ever {env:NAME}
  const cfg = config as { provider: Record<string, Record<string, unknown>> };
  const entry = cfg.provider["secret"];
  const options = entry.options as { apiKey?: string };
  eq(options.apiKey, "{env:SECRET_KEY}", "apiKey is env reference, not literal");
}

console.log("\n--- applyDomiosProviders: no apiKey when keyEnv missing ---\n");
{
  const provider: DomiosProvider = {
    id: "nokey",
    kind: "openai",
    baseUrl: "https://api.example.com",
  };
  const { config } = applyDomiosProviders({}, [provider]);
  const cfg = config as { provider: Record<string, Record<string, unknown>> };
  const entry = cfg.provider["nokey"];
  const options = entry.options as { apiKey?: string };
  assert(options.apiKey === undefined, "apiKey omitted when keyEnv is missing");
}

console.log("\n--- applyDomiosProviders: changed=false when result deep-equals input ---\n");
{
  const provider: DomiosProvider = {
    id: "stable",
    kind: "openai",
    baseUrl: "https://stable.com",
    keyEnv: "STABLE_KEY",
    models: ["m1"],
    name: "Stable",
  };
  const first = applyDomiosProviders({}, [provider]);
  const second = applyDomiosProviders(first.config, [provider]);
  assert(second.changed === false, "second apply with same providers gives changed=false");
}

console.log("\n--- providerKeyEnvName ---\n");
{
  eq(providerKeyEnvName("myllm"), "DOMIOS_PROVIDER_MYLLM_KEY", "simple id");
  eq(providerKeyEnvName("my-llm"), "DOMIOS_PROVIDER_MY_LLM_KEY", "hyphenated id");
  eq(providerKeyEnvName("myLLM"), "DOMIOS_PROVIDER_MY_LLM_KEY", "camelCase id");
  eq(providerKeyEnvName("my_llm"), "DOMIOS_PROVIDER_MY_LLM_KEY", "underscored id");
  eq(providerKeyEnvName("MyLLM"), "DOMIOS_PROVIDER_MY_LLM_KEY", "PascalCase id");
  eq(providerKeyEnvName("a.b.c"), "DOMIOS_PROVIDER_A_B_C_KEY", "dotted id");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
