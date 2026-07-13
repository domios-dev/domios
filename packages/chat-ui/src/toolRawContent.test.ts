import { distinctToolRawContent } from "./toolRawContent";

function assertEq(actual: unknown, expected: unknown, label: string) {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${String(expected)}, got ${String(actual)}`);
  }
}

assertEq(
  distinctToolRawContent("latest failure", "exit 2", '{\n  "command": "exit 2"\n}'),
  "latest failure",
  "distinct failed tool raw content is exposed",
);

assertEq(
  distinctToolRawContent("exit 2", "exit 2", '{\n  "command": "exit 2"\n}'),
  null,
  "primary duplicate is hidden",
);

assertEq(
  distinctToolRawContent('{\n  "command": "exit 2"\n}', "shell", '{\n  "command": "exit 2"\n}'),
  null,
  "input JSON duplicate is hidden",
);

assertEq(
  distinctToolRawContent(undefined, "shell", '{\n  "command": "exit 2"\n}'),
  null,
  "missing normalized provider content is ignored",
);

console.log("ok - tool raw content detail selection");
