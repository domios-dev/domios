import assert from "node:assert/strict";
import { test } from "node:test";
import { parseModelVariants, variantLaunch } from "../src/modelVariants";

test("TUI launch carries variants in a process-local agent override", () => {
  assert.deepEqual(variantLaunch(["-m", "vendor/model", "--variant", "high"]), {
    args: ["-m", "vendor/model", "--agent", "build"], config: { agent: { build: { variant: "high" } } },
  });
});
test("selected agents keep their identity when applying a variant", () => {
  assert.deepEqual(variantLaunch(["--agent", "preset", "--variant=max"]), {
    args: ["--agent", "preset"], config: { agent: { preset: { variant: "max" } } },
  });
});
test("a launch without a variant preserves its arguments and defaults", () => {
  assert.deepEqual(variantLaunch(["-m", "vendor/model"]), { args: ["-m", "vendor/model"], config: {} });
});

test("verbose catalogue exposes enabled variants and the default", () => {
  const stdout = ["vendor/model", JSON.stringify({ variants: { high: {}, max: {}, disabled: { disabled: true } } }, null, 2)].join("\n");
  assert.deepEqual(parseModelVariants(stdout).get("vendor/model"), [
    { id: "default", displayName: "Default" }, { id: "high", displayName: "High" }, { id: "max", displayName: "Max" },
  ]);
});
test("variant-free metadata disables the reasoning selector", () => {
  assert.deepEqual(parseModelVariants('vendor/plain\n{"name":"plain"}\n').get("vendor/plain"), []);
});
test("plain and malformed discovery output does not fabricate variants", () => {
  assert.equal(parseModelVariants("vendor/plain\nvendor/broken\n{invalid}").size, 0);
});
test("metadata belongs only to its preceding model", () => {
  const variants = parseModelVariants('a/one\n{"variants":{"high":{}}}\nb/two\n{"variants":{"low":{}}}\n');
  assert.deepEqual(variants.get("a/one")?.map((v) => v.id), ["default", "high"]);
  assert.deepEqual(variants.get("b/two")?.map((v) => v.id), ["default", "low"]);
});
