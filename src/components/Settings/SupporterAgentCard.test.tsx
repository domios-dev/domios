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
const ADAPTER = "fixture-adapter";
const MODELS = {
  adapter: ADAPTER,
  model: "fixture-model",
  adapters: [{
    id: ADAPTER,
    primitives: { choice: true, noul: true, score: true },
    models: [
      { id: "fixture-model", display_name: "Fixture Model" },
      { id: "second-model", display_name: "Second Model" },
    ],
  }],
};

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
type StatusFixture = Omit<ReturnType<typeof statusFixture>, "model"> & { model: string | null };
let statusBody: StatusFixture;
let writeStatus: number;
let writeRejects: boolean;

function statusFixture(
  state: "no_token" | "token_only" | "active" = "no_token",
  features = { memory_rerank: false, tab_state: false },
  present = state !== "no_token",
) {
  const token = { adapter: ADAPTER, present, path: "/x/y.json", mode_ok: true };
  return {
    state,
    adapter: ADAPTER,
    model: "fixture-model",
    deadline_ms: 3000,
    features,
    token,
    adapter_registered: true,
    adapters: [{ id: ADAPTER, primitives: { choice: true, noul: true, score: true }, models: MODELS.adapters[0].models, token }],
  };
}

function installFetch() {
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    const url = String(input);
    const body = typeof init?.body === "string" ? JSON.parse(init.body) as Record<string, unknown> : undefined;
    calls.push({ method, url, headers: { ...(init?.headers as Record<string, string> | undefined) }, body });
    if (method !== "GET" && writeRejects) throw new Error("network down");
    if (method === "POST" && url.endsWith("/api/supporter/features") && writeStatus < 400 && body) {
      const features = { ...statusBody.features, ...body };
      statusBody = statusFixture(
        features.memory_rerank || features.tab_state ? "active" : "token_only",
        features,
        true,
      );
    }
    if (method === "DELETE" && url.includes("/api/supporter/token") && writeStatus < 400) {
      statusBody = statusFixture();
    }
    if (method === "POST" && url.endsWith("/api/supporter/token") && writeStatus < 400) {
      statusBody = statusFixture("token_only");
    }
    if (method === "POST" && url.endsWith("/api/supporter/use") && writeStatus < 400 && body) {
      statusBody = {
        ...statusBody,
        adapter: body.adapter as string,
        model: typeof body.model === "string" ? body.model : null,
      };
    }
    const isWrite = method !== "GET";
    const responseBody = url.endsWith("/api/supporter/models")
      ? MODELS
      : method === "POST" && url.endsWith("/api/supporter/token")
        ? { adapter: ADAPTER, present: true, path: "/x/y.json", mode_ok: true }
        : isWrite && url.endsWith("/api/supporter/features")
          ? statusBody.features
          : isWrite && url.endsWith("/api/supporter/use")
            ? { adapter: statusBody.adapter, model: statusBody.model }
            : method === "DELETE" && url.includes("/api/supporter/token")
              ? { adapter: ADAPTER, removed: true }
              : statusBody;
    return {
      ok: !isWrite || writeStatus < 400,
      status: isWrite ? writeStatus : 200,
      json: async () => responseBody,
    } as Response;
  }) as typeof fetch;
}

async function render(onOpenGuide: (section: HelpSectionId) => void = () => {}) {
  await act(async () => {
    root.render(<SupporterAgentCard apiBase={API} apiHeaders={AUTH} onOpenGuide={onOpenGuide} />);
  });
  await act(async () => {});
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
  statusBody = statusFixture();
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
    const config = { supporter: { adapter: ADAPTER, model: "fixture-model" } } as unknown as AppConfig;
    await act(async () => {
      root.render(<AgentsOverview config={config} onUpdate={() => {}} onOpenProvider={() => {}} onOpenGuide={onOpenGuide} />);
    });
    await act(async () => {});
    expect(host.querySelector('[data-testid="settings-agents-overview"]')).not.toBeNull();
    expect(host.querySelectorAll('[data-testid="supporter-agent-token-form"]')).toHaveLength(1);
    expect(host.querySelectorAll('[data-testid="supporter-agent-status"]')).toHaveLength(1);
    expect(calls.filter((call) => call.method === "GET").map((call) => call.url.slice(call.url.indexOf("/api/"))).sort()).toEqual([
      "/api/supporter/models",
      "/api/supporter/status",
    ]);

    await typeAndSubmit(TOKEN);
    expect(calls.find((call) => call.method === "POST" && call.url.endsWith("/api/supporter/token"))?.body).toEqual({ adapter: ADAPTER, token: TOKEN });
    await act(async () => q<HTMLButtonElement>("supporter-agent-guide-link")!.click());
    expect(onOpenGuide).toHaveBeenCalledWith("supporter-agent");
    expect(JSON.stringify(invokes)).not.toContain(TOKEN);
    expect(invokes.every((invoke) => !/decision|token/i.test(invoke.cmd))).toBe(true);
  });

  it("supporter_no_token_state_shows_off_features_and_write_only_input", async () => {
    await render();
    expect(q("supporter-agent-overall-status")?.textContent).toBe("No token");
    expect(q("supporter-agent-token-state")?.textContent).toBe("No token");
    expect(q("supporter-agent-selection")?.textContent).toBe("fixture-adapter · fixture-model");
    expect(host.querySelector('[role="switch"][aria-label="Memory re-rank"]')?.getAttribute("aria-checked")).toBe("false");
    expect(host.querySelector('[role="switch"][aria-label="Tab-state"]')?.getAttribute("aria-checked")).toBe("false");
    expect(input().type).toBe("password");
    expect(input().value).toBe("");
    expect(input().autocomplete).toBe("off");
  });

  it("supporter_token_only_state_explicitly_says_token_set", async () => {
    statusBody = statusFixture("token_only");
    await render();
    expect(q("supporter-agent-overall-status")?.textContent).toBe("Token set, all features off");
    expect(q("supporter-agent-token-state")?.textContent).toBe("Token set");
    expect(host.querySelector('label[for="supporter-agent-token-input"]')?.textContent).toContain("Replace token");
    expect(q<HTMLButtonElement>("supporter-agent-token-save")?.textContent).toContain("Replace token");
    expect(input().value).toBe("");
  });

  it("supporter_active_state_shows_each_enabled_feature", async () => {
    statusBody = statusFixture("active", { memory_rerank: true, tab_state: true });
    await render();
    expect(q("supporter-agent-overall-status")?.textContent).toBe("Active");
    expect(host.querySelector('[role="switch"][aria-label="Memory re-rank"]')?.getAttribute("aria-checked")).toBe("true");
    expect(host.querySelector('[role="switch"][aria-label="Tab-state"]')?.getAttribute("aria-checked")).toBe("true");
    expect(q("supporter-agent-features")?.textContent).toContain("On");
  });

  it("supporter_token_input_is_write_only_and_refreshed", async () => {
    await render();
    await typeAndSubmit(TOKEN);
    expect(input().value).toBe("");
    expect(statusBody.state).toBe("token_only");

    writeRejects = true;
    await typeAndSubmit(TOKEN);
    expect(input().value).toBe("");
    writeRejects = false;
    writeStatus = 500;
    await typeAndSubmit(TOKEN);
    expect(input().value).toBe("");
    expect(host.innerHTML).not.toContain(TOKEN);
    expect(calls.filter((call) => call.method === "GET" && call.url.endsWith("/api/supporter/status")).length).toBeGreaterThan(1);
  });

  it("supporter_model_picker_lists_discovered_models", async () => {
    await render();
    const picker = q<HTMLSelectElement>("supporter-agent-model-picker")!;
    expect(Array.from(picker.options).map((option) => option.value)).toEqual(["fixture-model", "second-model"]);
    picker.value = "second-model";
    await act(async () => picker.dispatchEvent(new Event("change", { bubbles: true })));
    const use = calls.find((call) => call.method === "POST" && call.url.endsWith("/api/supporter/use"));
    expect(use?.body).toEqual({ adapter: ADAPTER, model: "second-model" });
  });

  it("supporter_feature_toggle_posts_one_feature_and_refreshes_status", async () => {
    statusBody = statusFixture("token_only");
    await render();
    await act(async () => host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Memory re-rank"]')!.click());
    expect(calls.find((call) => call.url.endsWith("/api/supporter/features"))?.body).toEqual({ memory_rerank: true });
    expect(q("supporter-agent-overall-status")?.textContent).toBe("Active");
  });

  it("supporter_status_has_accessible_names_and_token_never_renders", async () => {
    await render();
    const focusables = Array.from(host.querySelectorAll<HTMLElement>("button, input, select, a[href]"));
    const names = focusables.map((element) => element.getAttribute("aria-label") || element.textContent?.trim() || (element.id ? host.querySelector(`label[for="${element.id}"]`)?.textContent?.trim() : "") || "");
    expect(names.every((name) => name.length > 0)).toBe(true);
    expect(host.querySelector('[role="status"][aria-label="Supporter Agent status"]')).not.toBeNull();
    expect(document.body.innerHTML).not.toContain(TOKEN);
  });
});
