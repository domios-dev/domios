import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { storedRecordType, shouldParseClaudeRecord } from "../src/recordPrefix";
import { recordInput, plain } from "../../provider-shared/tests/recordHookFixture";

const cases: [string, string | undefined][] = [
  ['{"type":"progress","data":{}}', "progress"],
  ['{"parentUuid":null,"isSidechain":false,"type":"user","message":{}}', "user"],
  ['{"timestamp":"2026-01-01","type":"assistant","message":{}}', "assistant"],
  ['{"message":{"type":"progress"},"type":"user"}', undefined],
  ['{"data":[],"type":"user"}', undefined],
  ['{"parentUuid":"p","isSidechain":false,"cwd":"C:/work","sessionId":"s","version":"2.1.296","type":"attachment","attachment":{}}', "attachment"],
  ['{"note":"escaped \\"type\\":\\"progress\\" {","type":"user"}', "user"],
  ['{"type":"us\\u0065r","message":{}}', undefined],
  ['[ {"type":"progress"} ]', undefined],
  ['{"note":"no type"}', undefined],
];
for (const [line, expected] of cases) assert.equal(storedRecordType(line), expected, line);
for (const type of ["user", "assistant", "system", "ai-title", "edited_text_file", "attachment"]) {
  assert.equal(shouldParseClaudeRecord(JSON.stringify({ type }), true), true);
}
assert.equal(shouldParseClaudeRecord('{"type":"progress","data":{}}', true), false);
for (const key of ["permissionMode", "isApiErrorMessage", "toolUseResult", "message", "attachment"]) {
  assert.equal(shouldParseClaudeRecord(JSON.stringify({ type: "progress", [key]: {} }), true), true);
}
assert.equal(shouldParseClaudeRecord('{"type":"progress","permission\\u004dode":"plan"}', true), true);
assert.equal(shouldParseClaudeRecord('{"type":"system"}', false), false);

const root = path.resolve(import.meta.dirname, "..");
let parses = 0;
const context = {
  module: { exports: {} as { default: Record<string, (...args: unknown[]) => unknown> } },
  host: { platform: () => "linux", manifestJson: () => "null", envGet: () => null,
    expandHome: (value: string) => value, fs: { fileExists: () => false } },
  JSON: { parse: (text: string) => { parses++; return JSON.parse(text); }, stringify: JSON.stringify },
};
vm.runInNewContext(fs.readFileSync(path.join(root, "plugin.js"), "utf8"), context);
const plugin = context.module.exports.default;
const fixtureFiles = fs.readdirSync(path.join(root, "tests/fixtures"), { recursive: true })
  .map(String).filter(file => file.endsWith(".jsonl")).sort();
const digest = createHash("sha256");
for (const file of fixtureFiles) {
  const text = fs.readFileSync(path.join(root, "tests/fixtures", file), "utf8");
  for (const method of ["parseSessionDelta", "parseSessionMessages"]) {
    digest.update(JSON.stringify(plugin[method](text)));
  }
  const input = recordInput(text);
  const delta = plugin.parseSessionRecords(input.records, input.meta) as {messages:unknown[];new_offset:number};
  assert.deepEqual(plain(delta.messages), plain((plugin.parseSessionDelta(text) as {messages:unknown[]}).messages), file);
  assert.equal(delta.new_offset, input.meta.newOffset);
  assert.deepEqual(plain(plugin.parseSessionMessageRecords(input.records,input.meta)), plain(plugin.parseSessionMessages(text)), file);
}
assert.equal(digest.digest("hex"), "9a02c62e176daddcde0bd519645a4d9788fdcc2c4aeb32df8808f85c990450a6");
for (const method of ["parseSessionDelta", "parseSessionMessages"]) {
  parses = 0;
  const rows = plugin[method]('{"type":"progress","data":{"text":"ignored"}}\n'
    + '{"message":{"content":"kept"},"type":"user"}\n');
  assert.equal(parses, 1);
  assert.equal((method === "parseSessionDelta" ? (rows as { messages: unknown[] }).messages : rows as unknown[]).length, 1);
}
const { messages: observation } = plugin.parseSessionDelta('{"type":"progress","permissionMode":"plan"}') as { messages: Record<string, unknown>[] };
assert.equal(observation.length, 1);
assert.equal((observation[0].provider_observations as Record<string, unknown>).permission_mode, "plan");
console.log(`${cases.length} classifier cases and ${fixtureFiles.length * 2} fixture projections passed`);
