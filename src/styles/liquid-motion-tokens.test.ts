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
    "animation: ct-voice-fade-in 200ms cubic-bezier(0.16, 1, 0.3, 1) both;",
    "animation: ct-voice-fade-in var(--ct-duration-normal) var(--ct-ease-liquid-settle) both;",
  ],
  [
    globalsCss,
    "animation: ct-voice-lock-pop 200ms cubic-bezier(.22, .9, .25, 1) both;",
    "animation: ct-voice-lock-pop var(--ct-duration-normal) var(--ct-ease-squash-jump) both;",
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
    "animation: tab-materialize 280ms cubic-bezier(0.16, 1, 0.3, 1) both;",
    "animation: tab-materialize 280ms var(--ct-ease-liquid-settle) both;",
  ],
] as const;

for (const [css, before, after] of substitutedDeclarations) {
  assert(css.includes(after), `missing inert substitution: ${after}`);
  const resolved = after.replaceAll(/var\((--ct-(?:duration|ease)-[\w-]+)\)/g, (_, name: string) => {
    const values = declarations.get(name);
    assert.equal(values?.length, 1, `${name} must resolve uniquely`);
    return values[0];
  });
  assert.equal(resolved, before, `substitution must resolve to its pre-change declaration: ${after}`);
}

assert.match(
  globalsCss,
  /\.ct-deferred-loader\[data-transition-state="present"\]\s*\{[^}]*animation:\s*ct-deferred-loader-reveal var\(--ct-duration-fast\) var\(--ct-ease-liquid-settle\) var\(--ct-duration-normal\) both;/s,
  "shared loader reveal must remain delayed, theme-tokenized, and cancellable by state",
);
assert.match(globalsCss, /@property --launch-progress\s*\{[^}]*syntax:\s*"<percentage>"/s, "launch progress must interpolate as a percentage");
assert.match(globalsCss, /\.chat-launch-ring::before\s*\{[^}]*conic-gradient\([^}]*var\(--launch-progress\)[^}]*mask:\s*radial-gradient/s, "launch progress must be a transparent masked ring");
assert.doesNotMatch(globalsCss, /\.chat-launch-ring::before\s*\{[^}]*background:\s*var\(--ct-bg/s, "launch glyph must not sit on a circular backing");

console.log("liquid motion token tests: passed");
