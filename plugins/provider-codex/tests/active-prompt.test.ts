import assert from "node:assert/strict";
import fs from "node:fs";
import { parseActiveTerminalChoice, parseCodexPrompt } from "../src/prompt";
import { classifyCodexTabState } from "../src/tabState";

const fixture = (name: string) => fs.readFileSync(new URL(`./fixtures/${name}`, import.meta.url), "utf8");
const routed = fixture("tab-state/routed-numbered-list.txt");
const picker = fixture("model-picker.txt");

for (const screen of [routed, `${picker}\n${routed}`, picker.replace(/› (?=\d)/g, "  ")]) {
  assert.equal(parseCodexPrompt(screen), null);
  assert.equal(parseActiveTerminalChoice(screen), null);
  assert.notEqual(classifyCodexTabState({ tabId: "fixture", agentType: "fixture", screenText: screen }, 0)?.state,
    "clarifying_question");
}
assert.deepEqual(parseCodexPrompt(picker)?.options?.map(([, value]) => value), ["1", "2", "3", "4"]);
assert.equal(parseActiveTerminalChoice(picker)?.options.filter((option) => option.highlighted).length, 1);
assert.equal(classifyCodexTabState({ tabId: "fixture", agentType: "fixture", screenText: picker }, 0)?.state,
  "clarifying_question");
const renamed = picker.replace(/Press enter.*$/gm, "enter alpha · esc beta");
assert.equal(parseCodexPrompt(renamed)?.options?.length, 4);
console.log("13 active-prompt assertions passed");
