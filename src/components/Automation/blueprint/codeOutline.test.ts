import assert from "node:assert/strict";
import { codeOutline } from "./codeOutline";

assert.deepEqual(codeOutline("def main(value): return value + 1", "python"), [{ kind: "function", name: "main", params: "value", returns: null }]);
assert.deepEqual(codeOutline("class Scorer:\n    async def score(self, a: int, b: int) -> float:\n        return a / b", "python").map((symbol) => [symbol.kind, symbol.name, symbol.params, symbol.returns]),
  [["class", "Scorer", "", null], ["function", "score", "self, a: int, b: int", "float"]]);
assert.deepEqual(codeOutline("export default function main(value: string) { return value.toUpperCase(); }", "ts"), [{ kind: "function", name: "main", params: "value: string", returns: null }]);
assert.deepEqual(codeOutline("const toUpper = (value: string): string => value.toUpperCase();", "typescript").map((symbol) => [symbol.name, symbol.params, symbol.returns]), [["toUpper", "value: string", "string"]]);
assert.deepEqual(codeOutline("echo hi", "shell"), [], "shell commands have no outline");
assert.deepEqual(codeOutline("def main(value): pass", undefined), [], "an unknown language has no outline");
console.log("Code outline: python and script symbols, nothing for other languages");
