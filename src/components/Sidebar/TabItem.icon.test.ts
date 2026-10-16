import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = readFileSync(new URL("./TabItem.tsx", import.meta.url), "utf8");
const types = readFileSync(new URL("../../lib/types.ts", import.meta.url), "utf8");
const icons = readFileSync(new URL("../Icons/index.tsx", import.meta.url), "utf8");

assert.match(types, /tab_type: "Terminal" \| "Choice"/);
assert.match(source, /tab\.tab_type === "Choice"[\s\S]*?<ChoiceIcon/);
assert.doesNotMatch(source, /tab\.title\s*===|tab\.title\.includes|tab\.title\.startsWith/);
assert.match(icons, /export function ChoiceIcon[\s\S]*MousePointerClick/);
console.log("tab icon contracts: passed");
