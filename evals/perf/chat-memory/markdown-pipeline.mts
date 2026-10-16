import { Session } from "node:inspector/promises";
import { writeFileSync } from "node:fs";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { fixtureRecord } from "./fixture.mjs";
import { messageParts, normalizedMarkdown } from "../../../packages/chat-ui/src/messageParts";
import { remarkBareDomains, remarkCodeMetadata } from "../../../packages/chat-ui/src/remarkBareDomains";
import { rehypeMarkdownLineBreaks } from "../../../packages/chat-ui/src/markdownLineBreaks";
import { rehypeMessageParts } from "../../../packages/chat-ui/src/sourceOffsets";
import { remarkCachedParse } from "../../../packages/chat-ui/src/markdownParse";

const cached = process.argv.includes("--cached");
const rows = Array.from({ length: 360 }, (_, index) => {
  const record = fixtureRecord({ session: "pure", cwd: "/pure", index, run: "markdown" });
  const source = JSON.parse(record.line).message.content[0].text
    + `\n\nSee https://example.com/${index} and docs.example.org/path. `
    + "Long inline content with **emphasis** and `code`. ".repeat(8);
  return { id: `row-${index}`, source };
});
const inspector = new Session();
inspector.connect();
await inspector.post("HeapProfiler.enable");
await inspector.post("HeapProfiler.startSampling", {
  samplingInterval: 16384,
  includeObjectsCollectedByMajorGC: true,
  includeObjectsCollectedByMinorGC: true,
});
const begin = performance.now();
let renders = 0;
for (let pass = 0; pass < 3; pass++) {
  for (const row of rows) {
    const parts = messageParts(row.id, row.source);
    const normalized = normalizedMarkdown(row.source);
    ReactMarkdown({
      children: normalized.text,
      remarkPlugins: [remarkGfm, ...(cached ? [remarkCachedParse] : []), remarkBareDomains, remarkCodeMetadata],
      rehypePlugins: [rehypeMarkdownLineBreaks, rehypeMessageParts(parts)],
    });
    renders++;
  }
}
const elapsedMs = performance.now() - begin;
const { profile } = await inspector.post("HeapProfiler.stopSampling");
inspector.disconnect();
function total(node: typeof profile.head): number {
  return node.selfSize + node.children.reduce((sum, child) => sum + total(child), 0);
}
const allocatedBytes = total(profile.head);
const self: Array<{ frame: string; bytes: number }> = [];
function frames(node: typeof profile.head) {
  const frame = node.callFrame;
  if (node.selfSize) self.push({ frame: `${frame.functionName} ${frame.url}:${frame.lineNumber + 1}:${frame.columnNumber + 1}`, bytes: node.selfSize });
  node.children.forEach(frames);
}
frames(profile.head);
const result = JSON.stringify({
  mode: cached ? "shared-parse" : "baseline", rows: rows.length, renders,
  sourceChars: rows.reduce((sum, row) => sum + row.source.length, 0),
  elapsedMs, msPerRow: elapsedMs / renders, allocatedBytes, bytesPerRow: allocatedBytes / renders,
  self: self.sort((a, b) => b.bytes - a.bytes).slice(0, 12),
  scope: "Pure production pipeline on render-cache misses; 15 sessions x 24 rows x 3 visits. Sampled allocations include collected objects, not retained heap. No highlighting or DOM.",
}, null, 2);
const out = process.argv.indexOf("--out");
if (out >= 0) writeFileSync(process.argv[out + 1], result + "\n");
console.log(result);
