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
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
