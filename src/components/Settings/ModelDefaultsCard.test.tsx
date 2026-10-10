import "../Terminal/ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { AppConfig } from "../../lib/types";
import { ModelDefaultsCard } from "./ModelDefaultsCard";

test("each stored default renders once and only a still-current replacement shows its note", async () => {
  const config = {
    agents: { codex: { default_model: "gpt-6-sol" }, claude: { default_model: "opus" } },
    general_agent: { agent_provider: "claude" },
    memory: { extraction_agent: "", extraction_model: "", dreaming_agent: "", dreaming_model: "sonnet", worker_model: "haiku" },
    model_defaults: {
      replaced: {
        "agents.codex.default_model": { provider: "codex", from: "gpt-5.1-codex", to: "gpt-6-sol", reason: "r" },
        "agents.claude.default_model": { provider: "claude", from: "old", to: "sonnet", reason: "r" },
      },
    },
  } as unknown as AppConfig;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ModelDefaultsCard config={config} />));
    assert.equal(host.querySelectorAll('[data-testid="model-default-row"]').length, 4);
    const notes = host.querySelectorAll('[data-testid="model-default-replaced"]');
    assert.equal(notes.length, 1);
    assert.ok(notes[0].textContent?.includes("gpt-5.1-codex"));
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
