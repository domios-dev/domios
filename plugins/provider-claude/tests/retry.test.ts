import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import {test} from "node:test";
import type {ProviderSessionObservations} from "@codeterm/plugin-sdk";

const sandbox = {module:{exports:{} as {default?: {parseSessionObservations(value: unknown): ProviderSessionObservations | null}}}, exports:{}, host:{}};
vm.runInNewContext(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8"), sandbox);
const plugin = sandbox.module.exports.default!;
const cases = JSON.parse(fs.readFileSync(new URL("fixtures/retry.json", import.meta.url), "utf8"));
for (const fixture of cases) {
  test(fixture.name, () => {
    const signal = plugin.parseSessionObservations(fixture.record)?.retry_error;
    const normalized = signal ? {...JSON.parse(JSON.stringify(signal)), retryAtMs:signal.retryAtMs ?? null} : null;
    assert.deepEqual(normalized, fixture.signal);
  });
}
