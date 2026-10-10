import assert from "node:assert/strict";
import { applyDomiosPresets, readDomiosPresets, type DomiosPreset } from "../src/opencodePresets";

const preset: DomiosPreset = {
  name: "demo",
  model: "endpoint/model",
  prompt: "opaque prompt fixture",
  params: { temperature: 0, top_p: 0.8, steps: 12 },
};
const foreign = { model: "foreign/model", prompt: "foreign", temperature: 0.5 };
const initial = { agent: { foreign }, provider: { foreign: { npm: "foreign" } } };
const snapshot = JSON.stringify(initial);
const added = applyDomiosPresets(initial, [preset]);
assert.deepEqual(added, {
  config: {
    ...initial,
    agent: {
      foreign,
      demo: {
        model: preset.model,
        prompt: preset.prompt,
        mode: "primary",
        temperature: 0,
        top_p: 0.8,
        steps: 12,
        "x-domios": true,
      },
    },
  },
  changed: true,
  conflicts: [],
});
assert.equal(JSON.stringify(initial), snapshot);
assert.equal(applyDomiosPresets(added.config, [preset]).changed, false);
assert.deepEqual(applyDomiosPresets(added.config, []).config, initial);
assert.deepEqual(applyDomiosPresets(initial, [{ ...preset, name: "foreign" }]), {
  config: initial,
  changed: false,
  conflicts: ["foreign"],
});
const updated = applyDomiosPresets(added.config, [{ ...preset, prompt: "replacement", params: {} }]);
assert.deepEqual((updated.config as { agent: Record<string, unknown> }).agent.demo, {
  model: preset.model,
  prompt: "replacement",
  mode: "primary",
  "x-domios": true,
});
assert.deepEqual(applyDomiosPresets(applyDomiosPresets({}, [preset]).config, []).config, {});
const prototypeName = applyDomiosPresets({}, [{ ...preset, name: "__proto__" }]);
const agents = (prototypeName.config as { agent: Record<string, unknown> }).agent;
assert.equal(Object.prototype.hasOwnProperty.call(agents, "__proto__"), true);
assert.equal(Object.getPrototypeOf(agents), Object.prototype);
assert.equal(applyDomiosPresets({}, []).changed, false);
assert.deepEqual(readDomiosPresets(added.config), [preset]);
assert.deepEqual(readDomiosPresets(initial), []);
assert.throws(() => readDomiosPresets({ agent: [] }));
assert.throws(() => readDomiosPresets({ agent: { demo: { "x-domios": true, model: "m" } } }));
assert.throws(() => readDomiosPresets({ agent: { demo: { "x-domios": true, model: "m", prompt: "p", steps: "12" } } }));
console.log("15 preset writer checks passed");
