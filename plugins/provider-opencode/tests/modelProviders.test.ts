import assert from "node:assert/strict";
import { applyDomiosProviders } from "../src/opencodeConfig";
import { readModelProviders } from "../src/modelProviders";

const provider = {
  id: "demo", kind: "openai-compatible" as const, baseUrl: "http://localhost:1234/v1",
  keyEnv: "DOMIOS_PROVIDER_DEMO_KEY", models: ["model-a"], secretName: "slot",
};
const original = { agent: { foreign: { prompt: "opaque" } }, provider: { foreign: { options: { apiKey: "foreign" } } } };
const applied = applyDomiosProviders(original, [provider]);
assert.deepEqual(readModelProviders(applied.config), [provider]);
assert.deepEqual(applyDomiosProviders(applied.config, []).config, original);
assert.equal(applyDomiosProviders(applied.config, [provider]).changed, false);
assert.deepEqual(readModelProviders(original), []);
assert.throws(() => readModelProviders({ provider: [] }));
assert.throws(() => readModelProviders({ provider: { bad: { "x-domios": true, npm: "unsupported", options: { baseURL: "http://localhost" } } } }));
assert.throws(() => readModelProviders({ provider: { bad: { "x-domios": true, npm: "@ai-sdk/anthropic", options: { baseURL: "http://localhost", apiKey: "literal" } } } }));
assert.throws(() => readModelProviders({ provider: { bad: { "x-domios": true, npm: "@ai-sdk/anthropic", options: { baseURL: "http://localhost" }, "x-domios-secret": { name: "slot" } } } }));
assert.equal(JSON.stringify(applied.config).includes("{env:DOMIOS_PROVIDER_DEMO_KEY}"), true);
console.log("9 model provider config checks passed");
