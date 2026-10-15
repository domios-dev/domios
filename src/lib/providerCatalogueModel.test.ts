import assert from "node:assert/strict";
import { test } from "node:test";
import { parseProviderIcons, shouldRefetch } from "./providerCatalogueModel";

test("catalogue decisions cover attachment, unchanged versions, reconnects and failed fetches", () => {
  assert.equal(shouldRefetch(undefined, 0, false, null), true);
  assert.equal(shouldRefetch(1, 2, false, null), true);
  assert.equal(shouldRefetch(2, 2, false, null), false);
  assert.equal(shouldRefetch(2, 2, true, null), true);
  assert.equal(shouldRefetch(2, 2, true, "offline"), true);
  assert.equal(shouldRefetch(2, 2, false, "offline"), false);
});

test("persisted presentation metadata rejects malformed entries", () => {
  const icon = { id: "fixture", display_name: "Fixture", icon_svg: "<svg/>" };
  assert.deepEqual(parseProviderIcons(JSON.stringify([icon, null, {}, { ...icon, icon_svg: 42 }])), [icon]);
  assert.deepEqual(parseProviderIcons(null), []);
  assert.deepEqual(parseProviderIcons("{}"), []);
  assert.throws(() => parseProviderIcons("broken"));
});
