import assert from "node:assert/strict";
import { rehypeMarkdownLineBreaks } from "./markdownLineBreaks";

const sourceNewline = { type: "text", value: "\n", position: { start: { offset: 1 }, end: { offset: 2 } } };
const softBreak = { type: "text", value: "first\nsecond" };
const code = { type: "element", tagName: "code", children: [softBreak] };
const tree = {
  type: "root",
  children: [{
    type: "element",
    tagName: "p",
    children: [
      softBreak,
      { type: "element", tagName: "br" },
      { type: "text", value: "\n" },
      { type: "text", value: "third" },
      { type: "element", tagName: "br" },
      sourceNewline,
      code,
    ],
  }],
};
rehypeMarkdownLineBreaks()(tree);
assert.deepEqual(tree.children[0].children, [
  softBreak,
  { type: "element", tagName: "br" },
  { type: "text", value: "third" },
  { type: "element", tagName: "br" },
  sourceNewline,
  code,
]);
console.log("markdown hard breaks remove only generated whitespace");
