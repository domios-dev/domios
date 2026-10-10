import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type {ProviderSessionObservations} from "@codeterm/plugin-sdk";

const sandbox = {module:{exports:{} as {default?: {parseSessionObservations(value: unknown): ProviderSessionObservations | null}}},exports:{},host:{}};
vm.runInNewContext(fs.readFileSync(new URL("../plugin.js",import.meta.url),"utf8"),sandbox);
const plugin = sandbox.module.exports.default!;
interface Fixture { claude: unknown; codex: unknown; expected: ProviderSessionObservations["api_error"]; clears?: boolean; absent?: boolean }
const fixtures: Fixture[] = JSON.parse(fs.readFileSync(new URL("../../provider-shared/access-errors.json",import.meta.url),"utf8"));
for (const fixture of fixtures) {
  const observation = plugin.parseSessionObservations(fixture.codex);
  if (fixture.absent) assert.equal(observation, null);
  else {
    assert.equal(observation?.api_error, fixture.expected);
    assert.equal(observation?.clears_api_error, fixture.clears === true);
  }
}
console.log("Access error fixtures passed");
