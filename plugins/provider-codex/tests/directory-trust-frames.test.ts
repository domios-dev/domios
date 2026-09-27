/**
 * Codex directory-trust prompt response over wrapped trust frames.
 * Run: npx tsx plugins/provider-codex/tests/directory-trust-frames.test.ts
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import plugin from "../src/plugin";
import { classifyCodexTabState } from "../src/tabState";

let passed = 0;
let failed = 0;

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}: expected ${e}, got ${a}`); }
}

function visibleNumberedChoices(screen: string): [string, string][] {
  return screen
    .split(/\r*\n/)
    .map((line) => /^[ \t]*[>❯▶➤›]?[ \t]*(\d+)[.)][ \t]+(.+?)[ \t]*$/.exec(line))
    .filter((match): match is RegExpExecArray => match !== null)
    .map((match): [string, string] => [match[2], match[1]]);
}

console.log("\n--- directoryTrustPromptResponse: wrapped Codex trust frames ---\n");

{
  const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "directory-trust");
  const cases = JSON.parse(fs.readFileSync(path.join(dir, "cases.json"), "utf8")) as Array<{ file: string; expect: string }>;
  const keys: Record<string, string[] | null> = {
    enter: ["DQ=="],
    "move-up": ["G1tB"],
    "move-down": ["G1tC"],
    hold: [],
    none: null,
  };
  eq(cases.length >= 5, true, "every declared Codex directory-trust fixture is exercised");
  for (const c of cases) {
    const screen = fs.readFileSync(path.join(dir, c.file), "utf8");
    eq(plugin.directoryTrustPromptResponse!(screen), keys[c.expect], `codex directory trust ${c.file} -> ${c.expect}`);
  }

  const update = fs.readFileSync(path.join(dir, "codex-update-available.txt"), "utf8");
  const prompt = plugin.parsePrompt!(update);
  const visibleOptions = update
    .split(/\r?\n/)
    .map((line) => /^[ \t]*[>❯▶➤›]?[ \t]*\d+[.)][ \t]+(.+?)[ \t]*$/.exec(line)?.[1] ?? null)
    .filter((label): label is string => label !== null);
  const parsedOptions = prompt?.options ?? [];
  eq(parsedOptions.map(([label]) => label), visibleOptions, "update prompt retains the visible option labels and order");
  eq(parsedOptions.map(([, value]) => value), ["1", "2", "3"], "update prompt keeps screen option values in order");
  eq(plugin.buildPromptResponse!(1, update), ["G1tC", "DQ=="], "tab respond option two navigates to the next visible option");

  const transcript = fs.readFileSync(path.join(dir, "codex-update-in-transcript.txt"), "utf8");
  const transcriptState = classifyCodexTabState({
    tabId: "history-tab",
    agentType: "codex",
    screenText: transcript,
  }, 0);
  eq(
    transcriptState?.state,
    "unknown",
    "historical numbered update text is not classified as an active update modal",
  );
  eq(
    transcriptState ? transcriptState.confidence >= 0.8 : false,
    false,
    "historical update text stays below the blocking confidence threshold",
  );

  const userDir = fs.readFileSync(path.join(dir, "trust-folder-access-0.157.1.txt"), "utf8");
  const trustOptions = userDir
    .split(/\r?\n/)
    .map((line) => /^[ \t]*[>❯▶➤›]?[ \t]*\d+[.)][ \t]+(.+?)[ \t]*$/.exec(line)?.[1] ?? null)
    .filter((label): label is string => label !== null);
  const genericTrustPrompt = plugin.parsePrompt!(userDir);
  eq(genericTrustPrompt?.options?.map(([label]) => label), trustOptions,
    "provider parsePrompt exposes the active trust frame as a structured prompt");
  eq(genericTrustPrompt?.options?.map(([, value]) => value), ["1", "2"],
    "provider parsePrompt keeps both trust option values in screen order");
  const trustPrompt = plugin.parseDirectoryTrustPrompt!(userDir);
  eq(trustPrompt?.options?.map(([label]) => label), trustOptions, "folder trust prompt preserves option labels and order");
  eq(trustPrompt?.options?.map(([, value]) => value), ["1", "2"], "folder trust prompt retains both visible option values");
  eq(plugin.buildPromptResponse!(1, userDir), ["G1tC", "DQ=="], "tab respond can select the second trust option");

  for (const file of [
    "trust-frame-live-user-dir-0.157.1.txt",
    "trust-frame-full-screen-trailing-blank-rows.txt",
  ]) {
    const screen = fs.readFileSync(path.join(dir, file), "utf8");
    const choices = visibleNumberedChoices(screen);
    const parsed = plugin.parsePrompt!(screen);
    const directoryTrust = plugin.parseDirectoryTrustPrompt!(screen);
    eq(choices.length, 2, `${file} contains two visible choices`);
    eq(parsed?.options, choices, `${file} structured choices match the visible choices in order`);
    eq(directoryTrust?.options, choices, `${file} directory-trust parsing preserves the visible choices`);
    eq(parsed?.options?.map(([, value]) => value), ["1", "2"], `${file} retains both numbered values`);
  }

  const fullScreen = fs.readFileSync(path.join(dir, "trust-frame-full-screen-trailing-blank-rows.txt"), "utf8");
  eq(classifyCodexTabState({
    tabId: "full-screen-trust-tab",
    agentType: "codex",
    screenText: fullScreen,
  }, 0)?.state, "clarifying_question", "trailing terminal blank rows preserve the active choice classification");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
