import assert from "node:assert/strict";
import test from "node:test";
import { rehypeSourceOffsets, type HastNode } from "./sourceOffsets";
import { messageParts, markdownMessageParts, normalizedMarkdown } from "./messageParts";
const slices = (source: string) => messageParts("m", source).map((part) => source.slice(part.start, part.end));
test("messageParts preserves Markdown groups, definitions, tables and code examples", () => {
  assert.deepEqual(slices("  One block.\r\n"), ["  One block.\r\n"]);
  assert.deepEqual(slices("# Title\n\nOpening **paragraph**.\n\nNext."), ["# Title\n\nOpening **paragraph**.", "Next."]);
  assert.deepEqual(slices("Before.\n\n- one\n- two\n  - nested\n\nAfter."), ["Before.", "- one\n- two\n  - nested", "After."]);
  assert.deepEqual(slices('Before.\n\n```md\n# Not a heading\n\n<ds-plugin id="test" />\n```\n\nAfter.'), ['Before.', '```md\n# Not a heading\n\n<ds-plugin id="test" />\n```', 'After.']);
  assert.deepEqual(slices("Before.\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\nAfter."), ["Before.", "| A | B |\n| - | - |\n| 1 | 2 |", "After."]);
  assert.deepEqual(slices("First [link][ref].\n\nSecond.\n\n[ref]: https://example.com"), ["First [link][ref].", "Second."]);
});
test("messageParts canonical cards and pending HTML are separated before Markdown", () => {
  for (const ending of ["\n", "\r\n"]) {
    const tag = '<ds-plugin id="example" />';
    assert.deepEqual(slices(['Before.', tag, 'After.'].join(ending)), ['Before.', tag, 'After.']);
    assert.deepEqual(slices(['# Title', tag, 'After.'].join(ending)), ['# Title', tag, 'After.']);
  }
  const first = messageParts("stable", "First.\n\nSecond.")[0];
  assert.equal(first.id, "stable:0");
  assert.equal(messageParts("stable", "First.\n\nSecond.\n\nThird.")[0].id, first.id);
  assert.notEqual(messageParts("other", "First.\n\nSecond.")[0].id, first.id);
  const source = '<ds-html><input/>';
  assert.deepEqual(slices(source), [source]);
});
test("messageParts keeps original source offsets through multiline HTML normalization", () => {
  const html = "<div>first\r\n\r\nsecond</div>";
  const lead = 'Lead\n<ds-plugin id="example" />\n\n';
  const normalized = normalizedMarkdown(`${lead}${html}\n\nTail`);
  const tail = normalized.text.indexOf("Tail");
  assert.equal(normalized.start(tail), `${lead}${html}\n\n`.length);
  assert.equal(normalized.end(tail + 4), `${lead}${html}\n\nTail`.length);
  const node: HastNode = { type: "element", position: { start: { offset: tail }, end: { offset: tail + 4 } }, children: [{ type: "text", position: { start: { offset: tail }, end: { offset: tail + 4 } } }] };
  rehypeSourceOffsets(normalized)(node);
  assert.equal(node.properties?.dataSrcStart, String(`${lead}${html}\n\n`.length));
  assert.equal(node.properties?.dataSrcText, `${normalized.start(tail)}-${normalized.end(tail + 4)}`);
});
test("messageParts caches large messages and excludes mutable ends from identity", () => {
  const source = Array.from({ length: 2500 }, (_, index) => `## Section ${index}\n\nBody ${index} of a long agent reply.`).join("\n\n");
  assert.ok(source.length > 100_000);
  assert.equal(messageParts("long", source), messageParts("long", source));
  assert.equal(messageParts("tail", "partial")[0].id, messageParts("tail", "partial answer")[0].id);
});


test("messageParts separates whole-source caches from leading Markdown fragments", () => {
  const source = 'Lead\n<ds-html><input/></ds-html>\nTail';
  const whole = messageParts("shared", source);
  markdownMessageParts("shared", "Lead\n", 0);
  assert.equal(messageParts("shared", source), whole);
});
