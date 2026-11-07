import assert from "node:assert/strict";
import { promptQuestionParts } from "./promptQuestion";

assert.deepEqual(promptQuestionParts("title-token"), { title: "title-token", details: "" });
assert.deepEqual(
  promptQuestionParts("title-token\nline-a\n  line-b\n\nline-c\n"),
  { title: "title-token", details: "line-a\n  line-b\n\nline-c" },
  "detail lines keep their breaks and indentation",
);
assert.deepEqual(promptQuestionParts("\r\ntitle-token\r\nline-a\r\n"), { title: "title-token", details: "line-a" });
assert.deepEqual(promptQuestionParts("  \n\n"), { title: "", details: "" });
console.log("✓ prompt questions split into a title and line-preserving details");
