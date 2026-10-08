import assert from "node:assert/strict";
import { rehypeSourceOffsets, type HastNode } from "./sourceOffsets";
import { messageParts, normalizedMarkdown } from "./messageParts";

const slices = (source: string) => messageParts("m", source).map((part) => source.slice(part.start, part.end));
assert.deepEqual(slices("  One block.\r\n"), ["  One block.\r\n"]);
assert.deepEqual(slices("# Title\n\nOpening **paragraph**.\n\nNext."), ["# Title\n\nOpening **paragraph**.", "Next."]);
assert.deepEqual(slices("Before.\n\n- one\n- two\n  - nested\n\nAfter."), ["Before.", "- one\n- two\n  - nested", "After."]);
assert.deepEqual(slices("Before.\n\n```md\n# Not a heading\n\n@ds-plugin test\n```\n\nAfter."), ["Before.", "```md\n# Not a heading\n\n@ds-plugin test\n```", "After."]);
assert.deepEqual(slices("Before.\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\nAfter."), ["Before.", "| A | B |\n| - | - |\n| 1 | 2 |", "After."]);
assert.deepEqual(slices("Before.\n@ds-plugin example\nAfter."), ["Before.", "@ds-plugin example", "After."]);
assert.deepEqual(slices("Before.\r\n@ds-plugin example\r\nAfter."), ["Before.", "@ds-plugin example", "After."]);
const html = "<div>first\r\n\r\nsecond</div>";
assert.deepEqual(slices(`Before.\r\n@ds-plugin example\r\n${html}\r\n\r\nAfter.`), ["Before.", "@ds-plugin example", html, "After."]);
const normalized = normalizedMarkdown(`Lead\n@ds-plugin example\n\n${html}\n\nTail`);
const tail = normalized.text.indexOf("Tail");
assert.equal(normalized.start(tail), `Lead\n@ds-plugin example\n\n${html}\n\n`.length);
assert.equal(normalized.end(tail + 4), `Lead\n@ds-plugin example\n\n${html}\n\nTail`.length);
const first = messageParts("m", "First.\n\nSecond.")[0];
assert.equal(first.id, "m:0-6");
assert.equal(messageParts("m", "First.\n\nSecond.\n\nThird.")[0].id, first.id);
assert.notEqual(messageParts("other", "First.\n\nSecond.")[0].id, first.id);
assert.deepEqual(slices("First [link][ref].\n\nSecond.\n\n[ref]: https://example.com"), ["First [link][ref].", "Second."]);
assert.deepEqual(slices("# Title\n@ds-plugin example\nAfter."), ["# Title", "@ds-plugin example", "After."]);
const node: HastNode = {
  type: "element", position: { start: { offset: tail }, end: { offset: tail + 4 } },
  children: [{ type: "text", position: { start: { offset: tail }, end: { offset: tail + 4 } } }],
};
rehypeSourceOffsets(normalized)(node);
assert.equal(node.properties?.dataSrcStart, String(`Lead\n@ds-plugin example\n\n${html}\n\n`.length));
assert.equal(node.properties?.dataSrcText, `${normalized.start(tail)}-${normalized.end(tail + 4)}`);
console.log("messageParts: 17 source and normalization assertions passed");
