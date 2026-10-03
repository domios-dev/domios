import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import plugin from "../src/plugin";

interface Block { kind: string; data: Record<string, any> }
interface Row { blocks: Block[] }
interface Parser { parseSessionDelta(chunk: string, context?: {session_key: string; from_offset: number}): {messages: Row[]} }
const context = { module: { exports: {} as {default: Parser} }, exports: {}, console };
new vm.Script(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8")).runInNewContext(context);
for (const parser of [plugin as unknown as Parser, context.module.exports.default]) {
const patch = "*** Begin Patch\n*** Update File: /repo/a.ts\n@@\n-oldA\n+newA\n@@\n-oldB\n+newB\n*** End Patch";
const call = (id = "call-a", turn = "turn-a") => ({ type: "response_item", payload: {
  type: "custom_tool_call", name: "exec", call_id: id,
  input: `text(await tools.apply_patch(${JSON.stringify(patch)}));`,
  internal_chat_message_metadata_passthrough: { turn_id: turn },
} });
const change = (id = "change-a", turn = "turn-a", path = "/repo/a.ts", status = "completed") => ({type: "event_msg", payload: {
  type: "item_completed", turn_id: turn, item: {type: "FileChange", id, status,
    changes: {[path]: {type: "update", unified_diff: "@@ -1,2 +1,2 @@\n-oldA\n+newA\n-oldB\n+newB\n"}},
    stdout: "changed fixture", stderr: status === "failed" ? "fixture failure" : "",
  },
} });
const output = (id = "call-a") => ({type: "response_item", payload: {type: "custom_tool_call_output", call_id: id, output: "Script completed"}});
const parse = (rows: unknown[], session_key?: string, from_offset = 0): Block[] => JSON.parse(JSON.stringify(parser.parseSessionDelta(rows.map(row => JSON.stringify(row)).join("\n"), session_key ? {session_key, from_offset} : undefined))).messages.flatMap((row: Row) => row.blocks);
const uses = (rows: Block[]) => rows.filter(row => row.kind === "tool_use");

const matched = parse([call(), change(), output()]);
assert.equal(uses(matched).length, 1, "one nested patch and its FileChange must display one action");
const result = matched.find(row => row.kind === "tool_result" && row.data.file_diffs);
assert.equal(result?.data.tool_use_id, "call-a");
assert.equal(result?.data.file_diffs[0].hunks.length, 1, "retain the structured merged hunk");
assert.equal(result?.data.is_error, false);

for (const [name, rows] of [
  ["other turn", [call(), change("change-a", "turn-other")]],
  ["other path", [call(), change("change-a", "turn-a", "/repo/b.ts")]],
  ["ambiguous calls", [call(), call("call-b"), change()]],
  ["already closed call", [call(), output(), change()]],
  ["activity before call", [change(), call()]],
  ["unknown turn", [call("call-a", ""), change()]],
  ["aborted turn", [call(), {type: "event_msg", payload: {type: "turn_aborted", turn_id: "turn-a"}}, change()]],
] as const) {
  const records = parse([...rows]);
  assert.equal(uses(records).length, name === "ambiguous calls" ? 3 : 2, `${name} stays visible without an exact eligible match`);
}
const different = change();
different.payload.item.changes["/repo/a.ts"].unified_diff = "@@ -1 +1 @@\n-oldA\n+different\n";
assert.equal(uses(parse([call(), different])).length, 2, "same path alone cannot hide another edit");
const repeated = parse([call(), change(), output(), call("call-b"), change("change-b"), output("call-b")]);
assert.equal(uses(repeated).length, 2, "two real calls remain two actions even when patches are identical");
assert.deepEqual(repeated.filter(row => row.data.file_diffs).map(row => row.data.tool_use_id), ["call-a", "call-b"]);
const failed = parse([call(), change("failed", "turn-a", "/repo/a.ts", "failed")]);
assert.equal(failed.find(row => row.data.file_diffs)?.data.is_error, true, "failed activity remains failed");

parse([call()], "file-pair-split", 0);
assert.equal(uses(parse([change(), output()], "file-pair-split", 100)).length, 0, "pair across increasing delta offsets");
parse([call()], "file-pair-a", 0);
assert.equal(uses(parse([change()], "file-pair-b", 100)).length, 1, "never pair across sessions");
parse([call()], "file-pair-reset", 50);
assert.equal(uses(parse([change()], "file-pair-reset", 0)).length, 1, "snapshot rewind resets pending calls");
console.log("File-change pairing: 14 scenarios passed");
}
