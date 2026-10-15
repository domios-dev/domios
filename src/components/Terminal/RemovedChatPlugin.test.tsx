import React from "react";
import { strict as assert } from "node:assert";
import { test } from "node:test";
import { renderToStaticMarkup } from "react-dom/server";
import { RemovedChatPlugin } from "./RemovedChatPlugin";

test("removed-plugin state exposes an accessible status only for its typed failure", () => {
  const error = JSON.stringify({ code: "chat_backend_plugin_removed", plugin_id: "fixture-backend" });
  const html = renderToStaticMarkup(React.createElement(RemovedChatPlugin, { error }));
  assert.match(html, /role="status"/);
  assert.match(html, /aria-label="Plugin removed"/);
  assert.equal(renderToStaticMarkup(React.createElement(RemovedChatPlugin, { error: null })), "");
});
