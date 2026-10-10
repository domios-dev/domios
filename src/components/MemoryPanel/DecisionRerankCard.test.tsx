import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DecisionRerankCard } from "./DecisionRerankCard";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
let fetchCalls: number;

async function render(onOpenAgentsSettings: () => void = () => {}) {
  await act(async () => {
    root.render(<DecisionRerankCard onOpenAgentsSettings={onOpenAgentsSettings} />);
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
  it("rerank_card_has_agents_link_without_toggles", async () => {
    const onOpenAgentsSettings = vi.fn();
    await render(onOpenAgentsSettings);
    expect(host.querySelector('[role="switch"]')).toBeNull();
    const focusables = Array.from(host.querySelectorAll<HTMLElement>("button, input, a[href]"));
    expect(focusables.map((el) => el.getAttribute("data-testid"))).toEqual(["memory-rerank-agents-link"]);
    await act(async () => focusables[0].click());
    expect(onOpenAgentsSettings).toHaveBeenCalledTimes(1);
  });

  it("rerank_card_has_no_token_input", async () => {
    await render();
    expect(host.querySelector("input, form, a[href]")).toBeNull();
    expect(host.querySelector('[role="status"]')).toBeNull();
    expect(fetchCalls).toBe(0);
  });
});
