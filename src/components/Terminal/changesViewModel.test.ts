import assert from "node:assert/strict";
import { sessionChanges } from "@codeterm/chat-core";
import { changesDirectories, changesFilePath, changesTotals } from "./changesViewModel";

const files = sessionChanges([{ id: "edit", type: "tool_result", content: "", toolEdits: [
  { path: "z\\two.ts", old: "x", new: "y\nz" },
  { path: "a/one.ts", old: "", new: "one" },
] }]).cumulative;
assert.deepEqual(changesTotals(files), { additions: 3, deletions: 1 });
assert.deepEqual(changesDirectories(files).map((directory) => [directory.path, directory.additions, directory.deletions]), [["a", 1, 0], ["z", 2, 1]]);
assert.equal(changesFilePath(files[1], "/repo/"), "/repo/a/one.ts");
assert.equal(changesFilePath(files[1]), null);
assert.equal(changesFilePath({ ...files[1], movePath: "C:\\repo\\moved.ts" }, "/ignored"), "C:\\repo\\moved.ts");
assert.equal(changesFilePath({ ...files[1], path: "/absolute.ts" }), "/absolute.ts");
console.log("Changes view presentation passed");
