import assert from "node:assert/strict";
import { renderToStaticMarkup } from "react-dom/server";
import { ChatWindowRow } from "./ChatWindowRow";

const markup = renderToStaticMarkup(
  <ChatWindowRow message={{ id: "empty-thinking", type: "thinking", content: "" }}>
    <span>invisible child</span>
  </ChatWindowRow>,
);
assert.equal(markup, "");

const visibleMarkup = renderToStaticMarkup(
  <ChatWindowRow message={{ id: "visible-thinking", type: "thinking", content: "Reasoning" }}>
    <span>visible child</span>
  </ChatWindowRow>,
);
assert.match(visibleMarkup, /data-chat-window-row="visible-thinking"/);
assert.match(visibleMarkup, /visible child/);
