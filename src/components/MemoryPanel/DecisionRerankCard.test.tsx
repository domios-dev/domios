import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SupporterConfig } from "../../lib/types";
import { DecisionRerankCard } from "./DecisionRerankCard";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
let fetchCalls: number;

function supporter(over: Partial<SupporterConfig> = {}): SupporterConfig {
  return {
    adapter: "fixture", endpoint: null, model: null, deadline_ms: 3000,
    memory_rerank: {
      enabled: false, docs_hybrid: false, sources: { recall: 30, graph: 3, docs: 3, sessions: 0 },
      candidates: 50, max_concurrency: 16, low_confidence_threshold: 0.15, max_body_chars: 2000,
    },
    tab_state: { enabled: false }, ...over,
  };
}

async function render(s: SupporterConfig = supporter(), onUpdate: (...a: unknown[]) => void = () => {}, onOpenAgentsSettings: () => void = () => {}) {
  await act(async () => {
    root.render(<DecisionRerankCard supporter={s} onUpdate={onUpdate as never} onOpenAgentsSettings={onOpenAgentsSettings} />);
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
    const config = supporter({
      adapter: "custom",
      memory_rerank: { enabled: false, docs_hybrid: false, sources: { recall: 7, graph: 2, docs: 0, sessions: 0 }, candidates: 20, max_concurrency: 2, low_confidence_threshold: 0.3, max_body_chars: 800 },
    });
    await render(config, (...a) => updates.push(a));
    await act(async () => host.querySelector<HTMLButtonElement>('[data-testid="settings-toggle"]')!.click());
    expect(updates).toHaveLength(1);
    expect(updates[0][0]).toBe("supporter");
    expect(updates[0][1]).toEqual({ ...config, memory_rerank: { ...config.memory_rerank, enabled: true } });
  });

  it("rerank_card_has_toggle_and_agents_link_only", async () => {
    const onOpenAgentsSettings = vi.fn();
    await render(supporter(), () => {}, onOpenAgentsSettings);
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
