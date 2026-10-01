import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window as HappyWindow } from "happy-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiConfig } from "../../lib/types";
import type { MemNode } from "./types";
import { MemoryListTab } from "./MemoryListTab";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const api: ApiConfig = {
  enabled: true,
  host: "127.0.0.1",
  port: 7686,
  token: "fixture-token",
  cors: false,
  cors_allow_origins: [],
};

let host: HTMLElement;
let root: Root;
let urls: string[];
let originalFetch: typeof fetch;
let testWindow: InstanceType<typeof HappyWindow>;

beforeEach(() => {
  testWindow = new HappyWindow({ url: "http://localhost/" });
  for (const key of [
    "window",
    "document",
    "HTMLElement",
    "HTMLInputElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
  ] as const) {
    Object.defineProperty(globalThis, key, {
      value: (testWindow as never as Record<string, unknown>)[key],
      configurable: true,
    });
  }
  const getComputedStyle = (() => ({ getPropertyValue: () => "" })) as unknown as typeof globalThis.getComputedStyle;
  Object.defineProperty(globalThis, "getComputedStyle", { value: getComputedStyle, configurable: true });
  (testWindow as never as Record<string, unknown>).getComputedStyle = getComputedStyle;
  vi.useFakeTimers();
  originalFetch = globalThis.fetch;
  urls = [];
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    urls.push(String(input));
    return { ok: true, json: async () => ({ hits: [] }) } as Response;
  }) as typeof fetch;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  globalThis.fetch = originalFetch;
  vi.useRealTimers();
});

describe("MemoryListTab", () => {
  it("memory_list_search_disables_supporter_rerank", async () => {
    await act(async () => {
      root.render(
        <MemoryListTab
          api={api}
          nodes={new Map<string, MemNode>()}
          selectedId={null}
          onSelectItem={() => {}}
          onViewInGraph={() => {}}
          updateNode={() => {}}
          removeNode={() => {}}
        />,
      );
    });

    const search = host.querySelector<HTMLInputElement>('[data-testid="memory-list-search"]')!;
    Object.getOwnPropertyDescriptor(testWindow.HTMLInputElement.prototype, "value")?.set?.call(
      search,
      "supporter browse",
    );
    await act(async () => {
      search.dispatchEvent(new testWindow.Event("input", { bubbles: true }) as unknown as Event);
      search.dispatchEvent(new testWindow.Event("change", { bubbles: true }) as unknown as Event);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });

    expect(urls).toHaveLength(1);
    const request = new URL(urls[0]);
    expect(request.searchParams.get("q")).toBe("supporter browse");
    expect(request.searchParams.get("limit")).toBe("50");
    expect(request.searchParams.get("supporter")).toBe("false");
  });
});
