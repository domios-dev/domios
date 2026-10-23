import assert from "node:assert/strict";
import { formatAttachmentMarker } from "./attachmentMarker";
import { markVoice } from "./markers";
import { reconcileSends } from "./sendOrder";
import type { NormalizedChatMessage } from "./types";

const path = String.raw`C:\Users\dev\AppData\Local\Temp\codeterm-chat\401d36d0-voice-note-1792057536516.webm`;
const sentAtMs = 1792057536561;
const composer = markVoice(path, "composer transcript");
const wrap = (body: string, id = "1dce") => `<pasted_content id="${id}">\n\n\n${body}\n</pasted_content id="${id}">\n\npasted by Domios`;
const row = (content: string, correlationId?: string, file = path, tail = ""): NormalizedChatMessage => ({
  id: "b4d001ea-c12e-42b2-9b10-8c2eaa392c45-0", type: "user", content,
  timestamp: "2026-10-15T09:45:54.081Z", correlationId,
  segments: [{ type: "attachment", kind: "voice", path: file, label: "voice", body: "provider transcript", status: null }, { type: "text", text: tail }],
});
const send = { id: "voice-send", content: composer, sentAtMs, correlationId: "voice-token" };

for (const echo of [wrap(markVoice(path, "provider transcript")), `<pasted_content id="1dce">${composer}</pasted_content>`]) {
  const result = reconcileSends([send], [row(echo)]);
  assert.equal(result.pending.length, 0);
  assert.equal(result.echoes.size, 1);
  assert(result.echoes.has(send.id));
}

const foreign = markVoice(path.replace("codeterm-chat", "another-directory"), "other transcript");
assert.equal(reconcileSends([send], [row(wrap(foreign), undefined, path.replace("codeterm-chat", "another-directory"))]).pending.length, 1);
assert.equal(reconcileSends([send], [row(wrap(markVoice(path, "provider transcript")) + "unrelated tail", undefined, path, "unrelated tail")]).pending.length, 1);

const repeated = reconcileSends([send, { ...send, id: "second-send", correlationId: "second-token" }], [row(wrap(composer))]);
assert.equal(repeated.echoes.size, 1);
assert.equal(repeated.pending.length, 1);

const pasted = { id: "paste-send", content: "composer text", sentAtMs, correlationId: "paste-token" };
const byToken = reconcileSends([pasted], [row(wrap("rendered pasted payload"), "paste-token")]);
assert.equal(byToken.pending.length, 0);
assert.equal(byToken.echoes.size, 1);
const image = formatAttachmentMarker("C:/opaque/frag.png", "image");
const pastedBody = "opaque-a\nopaque-b\nopaque-c";
const attached = (id: string, correlationId: string) => ({ id, content: `${pastedBody}\n${image}`, sentAtMs, correlationId });
const besideWrapper = (id: string, pasteId = "9f1e", closeId = pasteId): NormalizedChatMessage => ({
  id, type: "user", timestamp: "2026-10-15T09:45:54.081Z",
  content: `\n\n<pasted_content id="${pasteId}">\n${pastedBody}\n</pasted_content id="${closeId}">\n${image}`,
  segments: [
    { type: "text", text: "\n\n" },
    { type: "paste", id: pasteId, body: `\n${pastedBody}\n` },
    { type: "text", text: "\n" },
    { type: "attachment", kind: "image", path: "C:/opaque/frag.png", label: null, body: null, status: null },
  ],
});

const one = reconcileSends([attached("attached-send", "attached-token")], [besideWrapper("row-1")]);
assert.equal(one.pending.length, 0);
assert.deepEqual([...one.echoes.keys()], ["attached-send"]);

const twice = reconcileSends(
  [attached("first", "first-token"), attached("second", "second-token")],
  [besideWrapper("row-1"), besideWrapper("row-2")],
);
assert.equal(twice.pending.length, 0);
assert.equal(twice.echoes.size, 2);

const onlyOneRow = reconcileSends(
  [attached("first", "first-token"), attached("second", "second-token")],
  [besideWrapper("row-1")],
);
assert.equal(onlyOneRow.echoes.size, 1);
assert.equal(onlyOneRow.pending.length, 1);

assert.equal(reconcileSends([attached("attached-send", "attached-token")], [besideWrapper("row-1", "9f1e", "other")]).pending.length, 1);
console.log("wrapped reconciliation: 21 assertions passed");
