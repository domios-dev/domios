import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const app = readFileSync(join(process.cwd(), "src/App.tsx"), "utf8");

test("Blueprint run popups resubscribe on a peer switch, not on every mux snapshot", () => {
  assert.match(app, /useBlueprintRunPopups\(state\.workspaces, peerContext\.generation,/);
  assert.doesNotMatch(app, /useBlueprintRunPopups\([^)]*state\.generation/);
});
