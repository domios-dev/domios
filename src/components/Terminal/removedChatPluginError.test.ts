import { strict as assert } from "node:assert";
import { test } from "node:test";
import { removedChatPluginId } from "./removedChatPluginError";

test("missing backend is identified by typed code and retains the supplied identity", () => {
  assert.equal(removedChatPluginId(JSON.stringify({ code: "chat_backend_plugin_removed", plugin_id: "fixture-backend" })), "fixture-backend");
});

test("unrelated and malformed failures are not classified as removed plugins", () => {
  for (const error of [null, "", "transport failed", "null", "[]", '{"code":"other","plugin_id":"fixture-backend"}', '{"code":"chat_backend_plugin_removed"}']) {
    assert.equal(removedChatPluginId(error), null);
  }
});
