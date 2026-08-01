// Run: npx tsx src/components/Terminal/queuedMessagePreview.test.ts

import { queuedMessagePreview } from "./queuedMessagePreview";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const copiedRichText = [
  "Result:",
  "",
  "Code",
  "text",
  "·",
  "2 lines",
  "",
  "Copy",
  "",
  "Fullscreen",
  "system.capabilities:",
  "BrowserOpenAdvertised = true",
  "Then:",
  "",
  "Code",
  "json",
  "·",
  "1 lines",
  "",
  "Copy",
  "",
  "Fullscreen",
  '{"error":{"code":-32601}}',
].join("\n");

const cleaned = queuedMessagePreview(copiedRichText);
assert(cleaned.includes("system.capabilities:"), "code source remains readable");
assert(cleaned.includes('{"error":{"code":-32601}}'), "JSON source remains readable");
assert(!cleaned.includes("Code text"), "copied code toolbar header is removed structurally");
assert(!cleaned.includes("Copy"), "copied code toolbar action is removed");
assert(!cleaned.includes("Fullscreen"), "copied fullscreen action is removed");

const markdown = queuedMessagePreview([
  "# Release note",
  "",
  "Read **the [guide](https://example.com)**.",
  "",
  "```json",
  '{"ready":true}',
  "```",
].join("\n"));
assert(markdown.includes("Release note"), "heading text remains");
assert(markdown.includes("Read the guide."), "inline Markdown becomes readable text");
assert(markdown.includes('{"ready":true}'), "fenced code keeps its source without renderer chrome");
assert(!markdown.includes("```"), "Markdown delimiters are omitted");
assert(!markdown.includes("https://example.com"), "link destination does not clutter the preview");

const ordinaryActions = queuedMessagePreview("Please Copy this sentence, then open Fullscreen mode.");
assert(ordinaryActions.includes("Copy"), "ordinary Copy prose is preserved");
assert(ordinaryActions.includes("Fullscreen"), "ordinary Fullscreen prose is preserved");

const longPreview = queuedMessagePreview("word ".repeat(300));
assert(longPreview.length <= 560, "preview has a bounded text payload");
assert(longPreview.endsWith("…"), "truncated preview communicates omitted text");

console.log("queuedMessagePreview tests: 4/4 passed");
