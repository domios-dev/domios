import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MemoryConfig } from "../../lib/types";
import { DecisionRerankCard } from "./DecisionRerankCard";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const API = "http://127.0.0.1:7686";
const AUTH = { Authorization: "Bearer bearer-fixture" };
const TOKEN = "tok-SECRET-4f9c2d";
const ADAPTER = "typesafe-jev";

interface Call {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: unknown;
}

let host: HTMLElement;
let root: Root;
let calls: Call[];
let statusBody: unknown;
let writeStatus: number;
let writeRejects: boolean;

function statusFixture(present: boolean, modeOk = true, registered = true) {
  return {
    config: { token: TOKEN },
    adapter_registered: registered,
    adapters: [{ id: ADAPTER, token: { adapter: ADAPTER, present, path: "/x/y.json", mode_ok: modeOk } }],
  };
}

function installFetch() {
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    calls.push({
      method,
      url: String(input),
      headers: { ...(init?.headers as Record<string, string> | undefined) },
      body: typeof init?.body === "string" ? JSON.parse(init.body) : undefined,
    });
    if (method !== "GET" && writeRejects) throw new Error("network down");
    const isWrite = method !== "GET";
    const body = isWrite ? { adapter: ADAPTER, present: true, token: TOKEN } : statusBody;
    return { ok: !isWrite || writeStatus < 400, status: isWrite ? writeStatus : 200, json: async () => body } as Response;
  }) as typeof fetch;
}

function mem(over: Partial<MemoryConfig> = {}): MemoryConfig {
  return {
    extraction_agent: "a", extraction_model: "m", auto_extract: true, dreaming_agent: "", dreaming_model: "",
    worker_model: "", worker_timeout_seconds: 1800, dedup_threshold: 0.15, file_sync_enabled: true,
    file_sync_interval_minutes: 60, ...over,
  };
}

async function render(m: MemoryConfig = mem(), onUpdate: (...a: unknown[]) => void = () => {}) {
  await act(async () => {
    root.render(<DecisionRerankCard mem={m} onUpdate={onUpdate as never} apiBase={API} apiHeaders={AUTH} />);
  });
}

const q = <T extends Element = HTMLElement>(id: string) => host.querySelector<T>(`[data-testid="${id}"]`);
const input = () => q<HTMLInputElement>("decision-rerank-token-input")!;

async function typeAndSubmit(value: string) {
  input().value = value;
  await act(async () => {
    q<HTMLFormElement>("decision-rerank-token-form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  });
}

beforeEach(() => {
  calls = [];
  statusBody = statusFixture(false);
  writeStatus = 200;
  writeRejects = false;
  installFetch();
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
      enabled: false, adapter: "custom", candidates: 7, max_concurrency: 2, deadline_ms: 900,
      low_confidence_threshold: 0.3, max_body_chars: 100, endpoint: "https://e", model: "m1",
    };
    await render(mem({ decision_rerank: decision }), (...a) => updates.push(a));
    await act(async () => host.querySelector<HTMLButtonElement>('[data-testid="settings-toggle"]')!.click());
    expect(updates).toHaveLength(1);
    expect(updates[0][0]).toBe("memory");
    expect(updates[0][1]).toEqual({ ...mem({ decision_rerank: decision }), decision_rerank: { ...decision, enabled: true } });
  });

  it("token_input_is_write_only", async () => {
    await render();
    expect(input().type).toBe("password");
    expect(input().value).toBe("");
    expect(input().autocomplete).toBe("off");

    await typeAndSubmit(TOKEN);
    expect(input().value).toBe("");

    writeRejects = true;
    await typeAndSubmit(TOKEN);
    expect(input().value).toBe("");

    writeRejects = false;
    writeStatus = 500;
    await typeAndSubmit(TOKEN);
    expect(input().value).toBe("");
    expect(host.innerHTML).not.toContain(TOKEN);
  });

  it("submit_posts_and_refreshes_status", async () => {
    await render();
    calls.length = 0;
    await typeAndSubmit(TOKEN);
    expect(calls).toHaveLength(2);
    expect(calls[0].method).toBe("POST");
    expect(calls[0].url).toBe(`${API}/api/mem/decision/token`);
    expect(calls[0].headers).toMatchObject({ ...AUTH, "Content-Type": "application/json" });
    expect(Object.keys(calls[0].body as object).sort()).toEqual(["adapter", "token"]);
    expect(calls[0].body).toEqual({ adapter: ADAPTER, token: TOKEN });
    expect(calls[1].method).toBe("GET");
    expect(calls[1].url).toBe(`${API}/api/mem/decision/status`);
    expect(calls[1].headers).toMatchObject(AUTH);
  });

  it("clear_deletes_and_refreshes_status", async () => {
    await render(mem({ decision_rerank: { adapter: "a b/c" } as never }));
    calls.length = 0;
    await act(async () => q<HTMLButtonElement>("decision-rerank-token-clear")!.click());
    expect(calls.map((c) => `${c.method} ${c.url}`)).toEqual([
      `DELETE ${API}/api/mem/decision/token?adapter=a%20b%2Fc`,
      `GET ${API}/api/mem/decision/status`,
    ]);
    expect(calls[0].headers).toMatchObject(AUTH);
  });

  it("status_renders_set_and_not_set", async () => {
    await render();
    expect(q("decision-rerank-token-state")!.textContent).toBe("not set");
    expect(q("decision-rerank-mode-warning")).toBeNull();

    statusBody = statusFixture(true, false, false);
    await act(async () => q<HTMLButtonElement>("decision-rerank-token-clear")!.click());
    expect(q("decision-rerank-token-state")!.textContent).toBe("set");
    expect(q("decision-rerank-mode-warning")).not.toBeNull();
    expect(q("decision-rerank-adapter-missing")).not.toBeNull();
    expect(host.innerHTML).not.toContain(TOKEN);
  });

  it("token_never_logged_or_rendered", async () => {
    const spies = (["log", "info", "warn", "error", "debug"] as const).map((k) => vi.spyOn(console, k).mockImplementation(() => {}));
    await render();
    await typeAndSubmit(TOKEN);
    writeRejects = true;
    await typeAndSubmit(TOKEN);
    for (const spy of spies) {
      expect(JSON.stringify(spy.mock.calls)).not.toContain(TOKEN);
    }
    expect(document.body.innerHTML).not.toContain(TOKEN);
    expect(host.querySelector('[value*="SECRET"]')).toBeNull();
  });

  it("help_links_present", async () => {
    await render();
    const help = q("decision-rerank-help")!;
    const hrefs = Array.from(help.querySelectorAll("a")).map((a) => a.getAttribute("href"));
    expect(hrefs).toEqual(["https://docs.typesafe.ai", "https://console.typesafe.ai/keys"]);
  });

  it("accessible_names_and_focus_order", async () => {
    await render();
    const focusables = Array.from(host.querySelectorAll<HTMLElement>("button, input, a[href]"));
    const names = focusables.map((el) => {
      const labelled = el.getAttribute("aria-label");
      const byFor = el.id ? host.querySelector(`label[for="${el.id}"]`)?.textContent : null;
      return labelled || byFor || el.textContent?.trim() || "";
    });
    expect(names.every((n) => n.length > 0)).toBe(true);
    const order = focusables.map((el) =>
      el.getAttribute("role") === "switch" ? "toggle"
        : el.tagName === "INPUT" ? "input"
        : el.tagName === "A" ? "link"
        : el.getAttribute("data-testid")?.replace("decision-rerank-token-", ""),
    );
    expect(order).toEqual(["toggle", "input", "save", "clear", "link", "link"]);
  });
});
