import assert from "node:assert/strict";
import { test } from "node:test";
import { formatAttachmentMarker } from "@codeterm/chat-core";
import { chatMessageAccessibleName } from "./chatMessageAccessibleName";

test("tab markers use live display names in the article name", () => {
  assert.equal(chatMessageAccessibleName("user", 'ping @Hugo and raw <domios type="tab" id="hugo" /> ok', id => id === "hugo" ? "Hugo" : undefined),
    "user message: ping @Hugo and raw @Hugo ok");
  assert.equal(chatMessageAccessibleName("user", '<domios type="tab" id="missing" />'), "user message: @Unknown tab");
});

test("a message name shows attachment labels without raw markers or generated numbers", () => {
  const marker = formatAttachmentMarker("C:\tmp\shot.png", "image", "image1");
  const name = chatMessageAccessibleName("user", `look ${marker} please`);
  assert.equal(name, "user message: look image please");
  assert.ok(!name.includes("<domios"));
});

test("a marker cut off by the scan window never leaks into the name", () => {
  const marker = formatAttachmentMarker("C:\tmp\shot.png", "image", "image1");
  const name = chatMessageAccessibleName("user", `${"x".repeat(990)}${marker}`);
  assert.ok(!name.includes("<domios"));
});
