import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MemoryConfig } from "../../lib/types";
import { DecisionRerankCard } from "./DecisionRerankCard";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
let fetchCalls: number;

function mem(over: Partial<MemoryConfig> = {}): MemoryConfig {
  return {
    extraction_agent: "a", extraction_model: "m", auto_extract: true, dreaming_agent: "", dreaming_model: "",
    worker_model: "", worker_timeout_seconds: 1800, dedup_threshold: 0.15, file_sync_enabled: true,
    file_sync_interval_minutes: 60, ...over,
  };
}

async function render(m: MemoryConfig = mem(), onUpdate: (...a: unknown[]) => void = () => {}, onOpenAgentsSettings: () => void = () => {}) {
  await act(async () => {
    root.render(<DecisionRerankCard mem={m} onUpdate={onUpdate as never} onOpenAgentsSettings={onOpenAgentsSettings} />);
  });
}

beforeEach(() => {
  fetchCalls = 0;
  globalThis.fetch = (async () => {
    fetchCalls += 1;
    throw new Error("the re-rank card must not fetch");
  }) as typeof fetch;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

describe("DecisionRerankCard", () => {
  it("toggle_writes_only_enabled", async () => {
    const updates: unknown[][] = [];
    const decision = {
      enabled: false, adapter: "custom", sources: { recall: 7, graph: 2, docs: 0 }, max_concurrency: 2, deadline_ms: 900,
      low_confidence_threshold: 0.3, max_body_chars: 100, endpoint: "https://e", model: "m1",
    };
    await render(mem({ decision_rerank: decision }), (...a) => updates.push(a));
    await act(async () => host.querySelector<HTMLButtonElement>('[data-testid="settings-toggle"]')!.click());
    expect(updates).toHaveLength(1);
    expect(updates[0][0]).toBe("memory");
    expect(updates[0][1]).toEqual({ ...mem({ decision_rerank: decision }), decision_rerank: { ...decision, enabled: true } });
  });

  it("rerank_card_has_toggle_and_agents_link_only", async () => {
    const onOpenAgentsSettings = vi.fn();
    await render(mem(), () => {}, onOpenAgentsSettings);
    const focusables = Array.from(host.querySelectorAll<HTMLElement>("button, input, a[href]"));
    expect(focusables.map((el) => el.getAttribute("data-testid"))).toEqual(["settings-toggle", "memory-rerank-agents-link"]);
    expect(focusables.every((el) => (el.getAttribute("aria-label") || el.textContent?.trim() || "").length > 0)).toBe(true);
    await act(async () => focusables[1].click());
    expect(onOpenAgentsSettings).toHaveBeenCalledTimes(1);
  });

  it("rerank_card_has_no_token_input", async () => {
    await render();
    expect(host.querySelector("input, form, a[href]")).toBeNull();
    expect(host.querySelector('[role="status"]')).toBeNull();
    expect(fetchCalls).toBe(0);
  });
});
