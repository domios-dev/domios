import assert from "node:assert/strict";
import { applyDomiosProviders } from "../src/opencodeConfig";
import { readModelProviders } from "../src/modelProviders";
import { opencodeConfigPath } from "../src/helpers";

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


const priorHost = Object.getOwnPropertyDescriptor(globalThis, "host");
try {
  for (const [environment, expected] of [
    [{ OPENCODE_CONFIG: "/fixture/custom.json", OPENCODE_CONFIG_DIR: "/ignored" }, "/fixture/custom.json"],
    [{ OPENCODE_CONFIG_DIR: "/fixture/config" }, "/fixture/config/opencode.json"],
    [{ XDG_CONFIG_HOME: "/fixture/xdg" }, "/fixture/xdg/opencode/opencode.json"],
    [{}, "/fixture/home/.config/opencode/opencode.json"],
  ] as Array<[Record<string, string>, string]>) {
    Object.defineProperty(globalThis, "host", { configurable: true, value: {
      platform: () => "linux",
      envGet: (name: string) => environment[name] ?? null,
      expandHome: (path: string) => path.replace("~", "/fixture/home"),
    }});
    assert.equal(opencodeConfigPath(), expected);
  }
} finally {
  if (priorHost) Object.defineProperty(globalThis, "host", priorHost);
  else Reflect.deleteProperty(globalThis, "host");
}
console.log("4 effective config source checks passed");
