import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import type { ParsedPrompt } from "@codeterm/plugin-sdk";
import type { TabStateClassification } from "../src/classify";

interface Plugin {
  parsePrompt(screen: string): ParsedPrompt | null;
  classifyTabState(snapshot: { screenText: string }): TabStateClassification;
  buildPromptResponse(index: number, screen: string): string[];
  directoryTrustPromptResponse(screen: string): unknown;
  parseDirectoryTrustPrompt(screen: string): unknown;
  launchOnboardingResponse(screen: string): unknown;
  launchOnboardingSafeChoice(screen: string): unknown;
  updatePromptSafeChoice(prompt: ParsedPrompt): unknown;
  permissionPromptResponse(tool: string, prompt: ParsedPrompt, screen: string): unknown;
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const context = {
  host: {
    manifestJson: () => "null", homeDir: () => "/tmp/home", platform: () => "linux",
    envGet: () => null, unixNowMs: () => 0,
    shell: { quoteFor: String, quote: String },
  },
  module: { exports: {} as { default: Plugin } }, exports: {},
};
new vm.Script(fs.readFileSync(path.join(root, "plugin.js"), "utf8")).runInNewContext(context);
const plugin = context.module.exports.default;
const dir = path.join(root, "tests/fixtures/external-imports");
const cases = JSON.parse(fs.readFileSync(path.join(dir, "cases.json"), "utf8")) as Array<{
  file: string; active: boolean; focus?: number;
}>;
let passed = 0;
for (const test of cases) {
  const screen = fs.readFileSync(path.join(dir, test.file), "utf8");
  const prompt = plugin.parsePrompt(screen);
  assert.equal(Boolean(prompt), test.active, test.file);
  const trustResponse = plugin.directoryTrustPromptResponse(screen);
  assert.ok(trustResponse === null || (Array.isArray(trustResponse) && trustResponse.length === 0), test.file);
  assert.equal(plugin.parseDirectoryTrustPrompt(screen), null, test.file);
  assert.equal(plugin.launchOnboardingResponse(screen), null, test.file);
  assert.equal(plugin.launchOnboardingSafeChoice(screen), null, test.file);
  if (test.active) {
    assert.ok(prompt);
    assert.equal(prompt.question, "Claude is asking whether to allow external CLAUDE.md imports");
    assert.deepEqual(Array.from(prompt.options, (option) => Array.from(option)), [
      ["No, disable external imports", "1"], ["Yes, allow external imports", "2"],
    ]);
    const verdict = plugin.classifyTabState({ screenText: screen });
    assert.equal(verdict.state, "clarifying_question");
    assert.equal(verdict.confidence, 1);
    assert.equal(verdict.action?.keys, undefined);
    assert.equal(verdict.action?.delayedResume, undefined);
    assert.equal(plugin.updatePromptSafeChoice(prompt), null);
    assert.equal(plugin.permissionPromptResponse("Bash", prompt, screen), null);
    for (let target = 0; target < 2; target++) {
      const keys = target === test.focus ? ["DQ=="] : [target > test.focus! ? "ag==" : "aw==", "DQ=="];
      assert.deepEqual(Array.from(plugin.buildPromptResponse(target, screen)), keys, test.file);
    }
  }
  passed++;
}
console.log(`# pass ${passed} external import fixture cases`);
