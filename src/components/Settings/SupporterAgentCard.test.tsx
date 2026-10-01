import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HelpSectionId } from "../../help/types";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import type { AppConfig } from "../../lib/types";
import { AgentsOverview } from "./AgentsTab";
import { SupporterAgentCard } from "./SupporterAgentCard";

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
let invokes: { cmd: string; args: unknown }[];
let statusBody: unknown;
let writeStatus: number;
let writeRejects: boolean;

function statusFixture(present: boolean, modeOk: boolean | null = true, registered = true) {
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

async function render(adapter = ADAPTER, onOpenGuide: (section: HelpSectionId) => void = () => {}) {
  await act(async () => {
    root.render(<SupporterAgentCard adapter={adapter} apiBase={API} apiHeaders={AUTH} onOpenGuide={onOpenGuide} />);
  });
}

const q = <T extends Element = HTMLElement>(id: string) => host.querySelector<T>(`[data-testid="${id}"]`);
const input = () => q<HTMLInputElement>("supporter-agent-token-input")!;

async function typeAndSubmit(value: string) {
  input().value = value;
  await act(async () => {
    q<HTMLFormElement>("supporter-agent-token-form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  });
}

beforeEach(() => {
  calls = [];
  invokes = [];
  statusBody = statusFixture(false);
  writeStatus = 200;
  writeRejects = false;
  installFetch();
  __setInvokeForTest(async <T,>(cmd: string, args?: unknown) => {
    invokes.push({ cmd, args });
    return (cmd === "list_agent_providers" ? [] : null) as T;
  });
  __setListenForTest(async () => () => {});
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  __setInvokeForTest(null);
  __setListenForTest(null);
  vi.restoreAllMocks();
});

describe("SupporterAgentCard", () => {
  it("supporter_card_renders_once_in_agents_overview", async () => {
    const onOpenGuide = vi.fn();
    const config = { memory: { decision_rerank: { adapter: "custom-adapter" } } } as unknown as AppConfig;
    await act(async () => {
      root.render(<AgentsOverview config={config} onUpdate={() => {}} onOpenProvider={() => {}} onOpenGuide={onOpenGuide} />);
    });
    expect(host.querySelector('[data-testid="settings-agents-overview"]')).not.toBeNull();
    expect(host.querySelectorAll('[data-testid="supporter-agent-token-form"]')).toHaveLength(1);
    expect(host.querySelectorAll('[data-testid="supporter-agent-status"]')).toHaveLength(1);
    expect(calls.map((c) => `${c.method} ${c.url.slice(c.url.indexOf("/api/"))}`)).toEqual(["GET /api/mem/decision/status"]);

    await typeAndSubmit(TOKEN);
    expect((calls[1].body as { adapter: string }).adapter).toBe("custom-adapter");
    await act(async () => q<HTMLButtonElement>("supporter-agent-guide-link")!.click());
    expect(onOpenGuide).toHaveBeenCalledWith("supporter-agent");
    expect(JSON.stringify(invokes)).not.toContain(TOKEN);
    expect(invokes.every((i) => !/decision|token/i.test(i.cmd))).toBe(true);
  });

  it("supporter_token_input_is_write_only", async () => {
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
    expect(calls.filter((c) => c.method === "GET").every((c) => c.url === `${API}/api/mem/decision/status`)).toBe(true);
  });

  it("supporter_submit_posts_and_refreshes_status", async () => {
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
    expect(invokes).toEqual([]);
  });

  it("supporter_submit_ignores_blank_token", async () => {
    await render();
    calls.length = 0;
    await typeAndSubmit("   ");
    expect(calls).toEqual([]);
  });

  it("supporter_clear_deletes_and_refreshes", async () => {
    await render("a b/c");
    calls.length = 0;
    await act(async () => q<HTMLButtonElement>("supporter-agent-token-clear")!.click());
    expect(calls.map((c) => `${c.method} ${c.url}`)).toEqual([
      `DELETE ${API}/api/mem/decision/token?adapter=a%20b%2Fc`,
      `GET ${API}/api/mem/decision/status`,
    ]);
    expect(calls[0].headers).toMatchObject(AUTH);
    expect(invokes).toEqual([]);
  });

  it("supporter_status_renders_set_and_not_set", async () => {
    await render();
    expect(q("supporter-agent-token-state")!.textContent).toBe("not set");
    expect(q("supporter-agent-mode-warning")).toBeNull();

    statusBody = statusFixture(true, false, false);
    await act(async () => q<HTMLButtonElement>("supporter-agent-token-clear")!.click());
    expect(q("supporter-agent-token-state")!.textContent).toBe("set");
    expect(q("supporter-agent-mode-warning")).not.toBeNull();
    expect(q("supporter-agent-adapter-missing")).not.toBeNull();
    expect(host.innerHTML).not.toContain(TOKEN);
  });

  it.each([
    ["absent file, mode unknown", false, null, false],
    ["present, mode unknown (non-Unix)", true, null, false],
    ["present, mode not owner-only", true, false, true],
  ] as const)("supporter_mode_warning: %s", async (_name, present, modeOk, warns) => {
    statusBody = statusFixture(present, modeOk);
    await render();
    expect(q("supporter-agent-token-state")!.textContent).toBe(present ? "set" : "not set");
    expect(q("supporter-agent-mode-warning") !== null).toBe(warns);
  });

  it("supporter_token_never_logged_or_rendered", async () => {
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
    expect(JSON.stringify(invokes)).not.toContain(TOKEN);
  });

  it("supporter_links_open_guide_section_and_site", async () => {
    const onOpenGuide = vi.fn();
    await render(ADAPTER, onOpenGuide);
    const help = q("supporter-agent-help")!;
    expect(Array.from(help.querySelectorAll("a")).map((a) => a.getAttribute("href"))).toEqual(["https://docs.typesafe.ai"]);
    expect(help.querySelector("a")!.getAttribute("target")).toBe("_blank");
    await act(async () => q<HTMLButtonElement>("supporter-agent-guide-link")!.click());
    expect(onOpenGuide).toHaveBeenCalledTimes(1);
    expect(onOpenGuide).toHaveBeenCalledWith("supporter-agent");
  });

  it("supporter_accessible_names_and_focus_order", async () => {
    await render();
    const focusables = Array.from(host.querySelectorAll<HTMLElement>("button, input, a[href]"));
    const names = focusables.map((el) => {
      const labelled = el.getAttribute("aria-label");
      const byFor = el.id ? host.querySelector(`label[for="${el.id}"]`)?.textContent : null;
      return labelled || byFor || el.textContent?.trim() || "";
    });
    expect(names.every((n) => n.length > 0)).toBe(true);
    const order = focusables.map((el) =>
      el.tagName === "INPUT" ? "input"
        : el.tagName === "A" ? "site-link"
        : el.getAttribute("data-testid")?.replace("supporter-agent-", ""),
    );
    expect(order).toEqual(["input", "token-save", "token-clear", "guide-link", "site-link"]);
    expect(host.querySelector('[role="status"]')).not.toBeNull();
  });
});
