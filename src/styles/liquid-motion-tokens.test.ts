// Run: npx tsx src/styles/liquid-motion-tokens.test.ts

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const readCss = (name: string) =>
  readFileSync(new URL(name, import.meta.url), "utf8").replaceAll("\r\n", "\n");

const tokensCss = readCss("./tokens.css");
const agentStateCss = readCss("./agent-state.css");
const globalsCss = readCss("./globals.css");

const expectedTokens = {
  "--ct-ease-squash-jump": "cubic-bezier(.22, .9, .25, 1)",
  "--ct-ease-liquid-breathe": "cubic-bezier(.42, 0, .58, 1)",
  "--ct-duration-liquid-sweep": "560ms",
  "--ct-duration-liquid-donut": "420ms",
  "--ct-duration-recording-pulse": "1200ms",
} as const;

const declarations = new Map<string, string[]>();
for (const match of tokensCss.matchAll(/(--ct-[\w-]+):\s*([^;]+);/g)) {
  const [, name, value] = match;
  const values = declarations.get(name) ?? [];
  values.push(value.trim());
  declarations.set(name, values);
}

for (const [name, expectedValue] of Object.entries(expectedTokens)) {
  assert.deepEqual(declarations.get(name), [expectedValue], `${name} must exist exactly once`);
}

for (const [name, values] of declarations) {
  if (name.startsWith("--ct-duration-liquid-") || name.startsWith("--ct-ease-")) {
    assert.equal(values.length, 1, `${name} must not have duplicate declarations`);
  }
}

const preChangeCurves = {
  "--ct-ease-liquid-settle": "cubic-bezier(0.16, 1, 0.3, 1)",
  "--ct-ease-out-back": "cubic-bezier(0.34, 1.56, 0.64, 1)",
  "--ct-ease-spring": "cubic-bezier(0.175, 0.885, 0.32, 1.1)",
} as const;

for (const [name, literal] of Object.entries(preChangeCurves)) {
  assert.deepEqual(declarations.get(name), [literal], `${name} must preserve ${literal}`);
  assert.equal(agentStateCss.includes(literal), false, `${literal} must be tokenized in agent-state.css`);
  assert.equal(globalsCss.includes(literal), false, `${literal} must be tokenized in globals.css`);
}

const substitutedDeclarations = [
  [
    agentStateCss,
    "animation: ct-agent-question-in 220ms cubic-bezier(0.16, 1, 0.3, 1);",
    "animation: ct-agent-question-in 220ms var(--ct-ease-liquid-settle);",
  ],
  [
    globalsCss,
    "animation: ct-voice-fade-in 160ms cubic-bezier(0.16, 1, 0.3, 1);",
    "animation: ct-voice-fade-in 160ms var(--ct-ease-liquid-settle);",
  ],
  [
    globalsCss,
    "animation: ct-voice-lock-pop 200ms cubic-bezier(0.16, 1, 0.3, 1);",
    "animation: ct-voice-lock-pop 200ms var(--ct-ease-liquid-settle);",
  ],
  [
    globalsCss,
    "transition:\n    width 320ms cubic-bezier(0.34, 1.56, 0.64, 1),\n    height 320ms cubic-bezier(0.34, 1.56, 0.64, 1);",
    "transition:\n    width 320ms var(--ct-ease-out-back),\n    height 320ms var(--ct-ease-out-back);",
  ],
  [
    globalsCss,
    "transition:\n    width 320ms cubic-bezier(0.34, 1.56, 0.64, 1),\n    height 320ms cubic-bezier(0.34, 1.56, 0.64, 1),\n    transform 90ms cubic-bezier(0.22, 1, 0.36, 1);",
    "transition:\n    width 320ms var(--ct-ease-out-back),\n    height 320ms var(--ct-ease-out-back),\n    transform 90ms cubic-bezier(0.22, 1, 0.36, 1);",
  ],
  [
    globalsCss,
    "transition: transform 300ms cubic-bezier(0.16, 1, 0.3, 1);",
    "transition: transform 300ms var(--ct-ease-liquid-settle);",
  ],
  [
    globalsCss,
    "animation: sidebar-tab-appear 200ms cubic-bezier(0.175, 0.885, 0.32, 1.1) both;",
    "animation: sidebar-tab-appear 200ms var(--ct-ease-spring) both;",
  ],
  [
    globalsCss,
    "transition: opacity 250ms cubic-bezier(0.16, 1, 0.3, 1), transform 250ms cubic-bezier(0.16, 1, 0.3, 1);",
    "transition: opacity 250ms var(--ct-ease-liquid-settle), transform 250ms var(--ct-ease-liquid-settle);",
  ],
  [
    globalsCss,
    "animation: pane-materialize 280ms cubic-bezier(0.16, 1, 0.3, 1) both;",
    "animation: pane-materialize 280ms var(--ct-ease-liquid-settle) both;",
  ],
  [
    globalsCss,
    "animation: pane-focus-pulse 400ms cubic-bezier(0.16, 1, 0.3, 1) 1;",
    "animation: pane-focus-pulse 400ms var(--ct-ease-liquid-settle) 1;",
  ],
] as const;

for (const [css, before, after] of substitutedDeclarations) {
  assert(css.includes(after), `missing inert substitution: ${after}`);
  const resolved = after.replaceAll(/var\((--ct-ease-[\w-]+)\)/g, (_, name: string) => {
    const values = declarations.get(name);
    assert.equal(values?.length, 1, `${name} must resolve uniquely`);
    return values[0];
  });
  assert.equal(resolved, before, `substitution must resolve to its pre-change declaration: ${after}`);
}

console.log("liquid motion token tests: passed");
