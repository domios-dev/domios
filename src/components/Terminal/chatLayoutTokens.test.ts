import assert from "node:assert/strict";
import { CHAT_READABLE_WIDTH_REM, chatLayoutTokens } from "./chatLayoutTokens";

assert.equal(chatLayoutTokens().maxWidthRem, CHAT_READABLE_WIDTH_REM);

for (const width of [CHAT_READABLE_WIDTH_REM, 42]) {
  const tokens = chatLayoutTokens(width);
  assert.equal(tokens.maxWidthRem, width);
  assert.equal(Number.parseFloat(tokens.variables["--ct-chat-readable-width"]), width);
}
