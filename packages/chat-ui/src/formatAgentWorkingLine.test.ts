// Run: npx tsx packages/chat-ui/src/formatAgentWorkingLine.test.ts

import { AGENT_WORKING_LINE_SEP, formatAgentWorkingLine } from "./formatAgentWorkingLine";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

assert(AGENT_WORKING_LINE_SEP === " · ", "working line separator uses mid-dots");

assert(
  formatAgentWorkingLine({
    providerName: "cursor",
    model: "gpt-5.5",
    activityText: "Channeling… (1s · ↓21 tokens)",
  }) === "cursor · gpt-5.5 · Channeling… (1s · ↓21 tokens)",
  "chat working line joins provider, model, and status with mid-dots",
);

assert(
  formatAgentWorkingLine({
    providerName: "Cursor",
    model: "gpt-5.5",
    activityText: "Editing 36.86k tokens",
  }) === "Cursor · gpt-5.5 · Editing 36.86k tokens",
  "provider and model prefix the activity verb",
);

assert(
  formatAgentWorkingLine({
    providerName: "Claude Code",
    model: "opus",
    activityText: null,
  }) === null,
  "no status line — no provider/model-only string",
);

assert(
  formatAgentWorkingLine({
    providerName: "  ",
    model: null,
    activityText: "Editing 36.86k tokens",
  }) === "Editing 36.86k tokens",
  "activity survives without provider/model segments",
);

assert(formatAgentWorkingLine({ providerName: null, model: null, activityText: null }) === null, "empty parts → null");

console.log("✓ formatAgentWorkingLine: provider · model · activity for chat working row");
