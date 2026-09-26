import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import plugin from "../src/plugin";

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}${detail ? `: ${detail}` : ""}`); }
}

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}, got ${a}`);
}

function fixtureText(name: string): string {
  return fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", name), "utf8");
}

function loadHostPlugin() {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = {
    host: {
      manifestJson: () => "null",
      homeDir: () => "/tmp",
      path: { equal: () => true },
      platform: () => "linux",
      unixNowMs: () => 0,
      envGet: () => null,
      shell: { quoteFor: (value: unknown) => String(value), quote: (value: unknown) => String(value) },
      fs: {
        fileExists: () => false,
        readFile: () => null,
        readFileHead: () => null,
        readFileTail: () => null,
        readJson: () => null,
      },
      expandHome: (value: string) => value,
      writeFile: () => true,
      removeFile: () => true,
      log: () => {},
    },
    console,
    module: { exports: {} as { default?: any } },
    exports: {},
  };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

console.log("\n--- numbered prompts remain available to tab respond ---\n");
{
  const bundled = loadHostPlugin();
  const modelScreen = fixtureText("model-picker.txt");
  const parsed = bundled.parsePrompt(modelScreen);
  eq(parsed?.question, "Select Reasoning Level for gpt-5.5", "model picker remains a numbered prompt");
  eq(parsed?.options, [
    ["Low                        Fast responses with lighter reasoning", "1"],
    ["Medium (default) (current) Balances speed and reasoning depth for everyday tasks", "2"],
    ["High                       Greater reasoning depth for complex problems", "3"],
    ["Extra high                 Extra high reasoning depth for complex problems", "4"],
  ], "model picker retains its active numbered options");
  eq(bundled.buildPromptResponse(1, modelScreen), ["DQ=="], "current model responds with Enter");
  eq(bundled.buildPromptResponse(0, modelScreen), ["G1tB", "DQ=="], "tab respond moves from the active model to option one");
  eq(bundled.buildPromptResponse(3, modelScreen), ["G1tC", "G1tC", "DQ=="], "tab respond moves down to the final model");

  const lettered = [
    "• Which release strategy should we use?",
    "  A) Fast patch release now",
    "  B) Minor release with bundled fixes",
    "  C) Delay for more testing",
  ].join("\n");
  const letteredPrompt = bundled.parsePrompt(lettered);
  eq(letteredPrompt?.question, "Which release strategy should we use?", "lettered prose question extracts");
  eq(letteredPrompt?.options, [
    ["Fast patch release now", "A"],
    ["Minor release with bundled fixes", "B"],
    ["Delay for more testing", "C"],
  ], "lettered prose choices extract");
  eq(bundled.buildPromptResponse(1, lettered), ["Qg0="], "lettered response sends the selected value");

  const numbered = [
    "• Which test strategy should we use?",
    "",
    "  1. Unit tests first",
    "  2. Integration smoke tests",
    "  3. Full end-to-end suite",
  ].join("\n");
  const prose = plugin.parsePrompt?.(numbered);
  eq(prose?.question, "Which test strategy should we use?", "numbered answer prompt remains supported by parsePrompt");
  eq(bundled.buildPromptResponse(2, numbered), ["Mw0="], "numbered prose response sends option three");
}

console.log("\n--- Codex folder trust prompt classification ---\n");
{
  const screen = fixtureText("directory-trust/trust-structured-question.txt");
  const bundled = loadHostPlugin();
  for (const [label, provider] of [["source", plugin], ["bundle", bundled]] as const) {
    const generic = provider.parsePrompt?.(screen);
    const trust = provider.parseDirectoryTrustPrompt?.(screen);
    eq(generic?.options?.map(([, value]: [string, string]) => value), ["1", "2"], `${label} parsePrompt exposes numbered trust choices`);
    eq(trust?.options?.map(([, value]: [string, string]) => value), ["1", "2"], `${label} trust parser exposes numbered choices`);
    assert(Boolean(trust?.question.trim().endsWith("?")), `${label} trust question comes from the active dialog`);
  }
  eq(bundled.buildPromptResponse(0, screen), ["G1tB", "DQ=="], "tab respond moves to the affirmative trust option");

  const currentTrustScreen = fixtureText("directory-trust/trust-folder-access-0.157.1.txt");
  for (const [label, provider] of [["source", plugin], ["bundle", bundled]] as const) {
    eq(
      provider.parsePrompt?.(currentTrustScreen)?.options?.map(([, value]: [string, string]) => value),
      ["1", "2"],
      `${label} general parser exposes current trust choices in order`,
    );
    eq(
      provider.parseDirectoryTrustPrompt?.(currentTrustScreen)?.options?.map(([, value]: [string, string]) => value),
      ["1", "2"],
      `${label} trust parser exposes current choices in order`,
    );
    eq(
      provider.directoryTrustPromptResponse?.(currentTrustScreen),
      ["DQ=="],
      `${label} trust responder confirms only the focused affirmative choice`,
    );
  }

  const ordinaryApproval = fixtureText("directory-trust/approval-paragraph.txt");
  assert(Boolean(plugin.parsePrompt?.(ordinaryApproval)?.options?.length), "generic yes/no approval remains available to parsePrompt");
  assert(Boolean(bundled.parsePrompt(ordinaryApproval)?.options?.length), "bundled generic yes/no approval remains available to tab respond");
  eq(plugin.parseDirectoryTrustPrompt?.(ordinaryApproval), null, "source trust parser ignores a generic yes/no approval");
  eq(bundled.parseDirectoryTrustPrompt(ordinaryApproval), null, "bundle trust parser ignores a generic yes/no approval");

  const offScreen = screen.replace("  Press enter to continue", "  The session is ready");
  eq(plugin.parseDirectoryTrustPrompt?.(offScreen), null, "source trust parser refuses a dialog without its active footer");
  eq(bundled.parseDirectoryTrustPrompt(offScreen), null, "bundle trust parser refuses a dialog without its active footer");
  eq(bundled.directoryTrustPromptResponse(offScreen), null, "auto responder refuses a trust prompt that is no longer active");

  const negatives = [
    ["approval paragraph", "directory-trust/approval-paragraph.txt"],
    ["bypass warning", "directory-trust/bypass-warning.txt"],
    ["model picker", "directory-trust/model-picker.txt"],
    ["numbered answer list", "directory-trust/numbered-answer-list.txt"],
    ["approval that mentions trust", "directory-trust/command-approval-mentions-trust.txt"],
  ] as const;
  for (const [label, fixture] of negatives) {
    const negative = fixtureText(fixture);
    eq(plugin.parseDirectoryTrustPrompt?.(negative), null, `source trust parser rejects ${label}`);
    eq(bundled.parseDirectoryTrustPrompt(negative), null, `bundle trust parser rejects ${label}`);
    eq(bundled.directoryTrustPromptResponse(negative), null, `auto responder rejects ${label}`);
  }
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
