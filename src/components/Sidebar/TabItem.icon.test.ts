import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = readFileSync(new URL("./TabItem.tsx", import.meta.url), "utf8");
const types = readFileSync(new URL("../../lib/types.ts", import.meta.url), "utf8");
const icons = readFileSync(new URL("../Icons/index.tsx", import.meta.url), "utf8");
const styles = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");

assert.match(types, /tab_type: "Terminal" \| "Choice"/);
assert.match(source, /tab\.tab_type === "Choice"[\s\S]*?<ChoiceIcon/);
assert.doesNotMatch(source, /tab\.title\s*===|tab\.title\.includes|tab\.title\.startsWith/);
assert.match(icons, /export function ChoiceIcon[\s\S]*MousePointerClick/);
assert.match(styles, /\.sidebar-tab:focus-within \.sidebar-tab-close/);
assert.match(styles, /\.sidebar-tab:focus-within \.sidebar-leadership-badge/);
console.log("tab icon and fixed-slot contracts: passed");
