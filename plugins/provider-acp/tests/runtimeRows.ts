import assert from "node:assert/strict";
import type { AgentEvent, ChatMsg } from "@codeterm/plugin-sdk";

export function foldRow(rows:Map<string,ChatMsg>, event:AgentEvent):void {
  if (event.kind === "chatItem") rows.set(event.item.id,{...event.item});
  if (event.kind === "chatItemPatch") {
    const previous=rows.get(event.item.id);
    assert.ok(previous,"patch has an existing row");
    rows.set(event.item.id,{...previous,...event.item,seq:previous.seq});
  }
  if (event.kind === "chatItemAppend") {
    const previous=rows.get(event.itemId);
    assert.ok(previous,"append has an existing row");
    assert.equal(Buffer.byteLength(previous.content),event.offset,"append offset is the retained UTF-8 byte length");
    previous.content+=event.content;
  }
}
