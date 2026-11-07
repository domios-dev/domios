import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { validLimitsSource } from "./limitsSource";

interface SharedCase {name:string; valid:boolean; profile:{limitsSource?:unknown; secretEnv?:Record<string,string>; authEnv?:Record<string,unknown>}}
const fixture = JSON.parse(readFileSync(new URL("../../../src-tauri/crates/codeterm-plugin/src/js_runtime/parity_test/fixtures/limits-sources.json", import.meta.url), "utf8")) as {cases:SharedCase[]};

test("the SDK limits-source validator agrees with the host manifest validator on shared fixtures", () => {
  assert.ok(fixture.cases.length >= 10);
  for (const entry of fixture.cases) {
    const accepted = entry.profile.limitsSource === undefined || validLimitsSource(entry.profile.limitsSource, entry.profile);
    assert.equal(accepted, entry.valid, entry.name);
  }
});
