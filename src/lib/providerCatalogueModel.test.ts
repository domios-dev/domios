import assert from "node:assert/strict";
import { test } from "node:test";
import { parseProviderIcons, shouldRefetch } from "./providerCatalogueModel";

test("catalogue refresh follows server revision or build changes, not stream readiness", () => {
  assert.equal(shouldRefetch(undefined, { build: "a", revision: 1 }), true);
  assert.equal(shouldRefetch({ build: "a", revision: 1 }, { build: "a", revision: 1 }), false);
  assert.equal(shouldRefetch({ build: "a", revision: 1 }, { build: "a", revision: 2 }), true);
  assert.equal(shouldRefetch({ build: "a", revision: 2 }, { build: "b", revision: 1 }), true);
  assert.equal(shouldRefetch({ boot: "old", build: "b" }, { boot: "new", build: "b" }), true);
});

test("persisted presentation metadata rejects malformed entries", () => {
  const icon = { id: "fixture", display_name: "Fixture", icon_svg: "<svg/>" };
  assert.deepEqual(parseProviderIcons(JSON.stringify([icon, null, {}, { ...icon, icon_svg: 42 }])), [icon]);
  assert.deepEqual(parseProviderIcons(null), []);
  assert.deepEqual(parseProviderIcons("{}"), []);
  assert.throws(() => parseProviderIcons("broken"));
});
