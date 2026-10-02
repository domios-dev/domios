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
  adapters: [
    {
      id: ADAPTER,
      token_mode: "required" as const,
      primitives: { choice: true, noul: true, score: true },
      models: [
        { id: "fixture-model", display_name: "Fixture Model" },
        { id: "second-model", display_name: "Second Model" },
      ],
    },
  ],
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
type StatusFixture = Omit<ReturnType<typeof statusFixture>, "model"> & {
  model: string | null;
};
let statusBody: StatusFixture;
let writeStatus: number;
let writeRejects: boolean;

function statusFixture(
  state: "no_token" | "active" | "budget_reached" | "credit_exhausted" = "no_token",
  present = state !== "no_token",
  tokenMode: "required" | "optional" | "none" = "required",
) {
  const token = {
    adapter: ADAPTER,
    token_mode: tokenMode,
    present,
    path: "/x/y.json",
    mode_ok: true,
  };
  return {
    state,
    token_mode: tokenMode,
    adapter: ADAPTER,
    model: "fixture-model",
    deadline_ms: 3000,
    connected: state !== "no_token",
    calls_last_hour: 12,
    calls_last_day: 45,
    last_activity: 1700000000,
    last_verdict: {
      tab_state: { time: 1700000000, label: "ready_idle", confidence: 0.9 },
    },
    budget: {
      reached: state === "budget_reached",
      calls_per_hour: 600 as number | null,
      free_at: state === "budget_reached" ? 1700003600 : null,
      storage_ready: true,
    },
    spend: {
      today: {
        input_tokens: 1_000_000,
        output_tokens: 500,
        estimated_usd: 0.042 as number | null,
      },
      month: {
        input_tokens: 2_000_000,
        output_tokens: 1000,
        estimated_usd: 0.084 as number | null,
      },
      since_balance: {
        input_tokens: 1_000_000,
        output_tokens: 500,
        estimated_usd: 0.042 as number | null,
      },
      balance_usd: 5 as number | null,
      balance_set_at: 1700000000 as number | null,
      remaining_usd: 4.958 as number | null,
      exhausted: state === "credit_exhausted",
      next_probe_at: state === "credit_exhausted" ? 1700003600 : null,
      storage_ready: true,
    },
    token,
    adapter_registered: true,
    adapters: [
      {
        id: ADAPTER,
        token_mode: tokenMode,
        primitives: { choice: true, noul: true, score: true },
        models: MODELS.adapters[0].models,
        token,
      },
    ],
  };
}

function installFetch() {
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    const url = String(input);
    const body = typeof init?.body === "string" ? (JSON.parse(init.body) as Record<string, unknown>) : undefined;
    calls.push({
      method,
      url,
      headers: { ...(init?.headers as Record<string, string> | undefined) },
      body,
    });
    if (method !== "GET" && writeRejects) throw new Error("network down");
    if (method === "DELETE" && url.includes("/api/supporter/token") && writeStatus < 400) {
      statusBody = statusFixture();
    }
    if (method === "POST" && url.endsWith("/api/supporter/token") && writeStatus < 400) {
      statusBody = statusFixture("active");
    }
    if (method === "POST" && url.endsWith("/api/supporter/use") && writeStatus < 400 && body) {
      statusBody = {
        ...statusBody,
        adapter: body.adapter as string,
        model: typeof body.model === "string" ? body.model : null,
      };
    }
    if (method === "POST" && url.endsWith("/api/supporter/balance") && writeStatus < 400 && body) {
      statusBody = {
        ...statusBody,
        spend: {
          ...statusBody.spend,
          balance_usd: body.balance_usd as number | null,
          remaining_usd: body.balance_usd as number | null,
          exhausted: false,
        },
      };
    }
    const isWrite = method !== "GET";
    const responseBody = url.includes("/api/supporter/activity")
      ? { entries: [] }
      : url.endsWith("/api/supporter/models")
      ? MODELS
      : method === "POST" && url.endsWith("/api/supporter/token")
        ? { adapter: ADAPTER, present: true, path: "/x/y.json", mode_ok: true }
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
    root.render(<SupporterAgentCard api={{ enabled: true, host: "127.0.0.1", port: 7686, token: "test-token", cors: false }} apiBase={API} apiHeaders={AUTH} onOpenGuide={onOpenGuide} />);
  });
  await act(async () => {});
}

const q = <T extends Element = HTMLElement>(id: string) => host.querySelector<T>(`[data-testid="${id}"]`);
const input = () => q<HTMLInputElement>("supporter-agent-token-input")!;

async function typeAndSubmit(value: string) {
  input().value = value;
  await act(async () => {
    q<HTMLFormElement>("supporter-agent-token-form")!.dispatchEvent(
      new Event("submit", { bubbles: true, cancelable: true }),
    );
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
    const config = {
      supporter: { adapter: ADAPTER, model: "fixture-model" },
    } as unknown as AppConfig;
    await act(async () => {
      root.render(
        <AgentsOverview config={config} onUpdate={() => {}} onOpenProvider={() => {}} onOpenGuide={onOpenGuide} />,
      );
    });
    await act(async () => {});
    expect(host.querySelector('[data-testid="settings-agents-overview"]')).not.toBeNull();
    expect(host.querySelectorAll('[data-testid="supporter-agent-token-form"]')).toHaveLength(1);
    expect(host.querySelectorAll('[data-testid="supporter-agent-status"]')).toHaveLength(1);
    expect(
      calls
        .filter((call) => call.method === "GET")
        .map((call) => call.url.slice(call.url.indexOf("/api/")))
        .sort(),
    ).toEqual(["/api/supporter/models", "/api/supporter/status"]);

    await typeAndSubmit(TOKEN);
    expect(calls.find((call) => call.method === "POST" && call.url.endsWith("/api/supporter/token"))?.body).toEqual({
      adapter: ADAPTER,
      token: TOKEN,
    });
    await act(async () => q<HTMLButtonElement>("supporter-agent-guide-link")!.click());
    expect(onOpenGuide).toHaveBeenCalledWith("supporter-agent");
    expect(JSON.stringify(invokes)).not.toContain(TOKEN);
    expect(invokes.every((invoke) => !/decision|token/i.test(invoke.cmd))).toBe(true);
  });

  it("supporter_no_token_state_shows_connection_and_write_only_input", async () => {
    await render();
    expect(q("supporter-agent-overall-status")?.textContent).toBe("No token");
    expect(q("supporter-agent-token-state")?.textContent).toBe("No token");
    expect(q("supporter-agent-selection")?.textContent).toBe("fixture-adapter · fixture-model");
    expect(host.querySelector('[role="switch"][aria-label="Memory re-rank"]')).toBeNull();
    expect(host.querySelector('[role="switch"][aria-label="Tab-state"]')).toBeNull();
    expect(input().type).toBe("password");
    expect(input().value).toBe("");
    expect(input().autocomplete).toBe("off");
  });

  it("supporter_optional_token_mode_connects_with_write_only_optional_field", async () => {
    statusBody = statusFixture("active", false, "optional");
    await render();
    expect(q("supporter-agent-overall-status")?.textContent).toBe("Active");
    expect(q("supporter-agent-token-state")?.textContent).toBe("No token");
    expect(host.querySelector('[aria-label="API token (optional)"]')).not.toBeNull();
    expect(input().type).toBe("password");
    expect(input().value).toBe("");
    expect(input().autocomplete).toBe("off");
  });

  it("supporter_none_token_mode_hides_token_form", async () => {
    statusBody = statusFixture("active", false, "none");
    await render();
    expect(q("supporter-agent-token-state")?.textContent).toBe("No token needed");
    expect(q("supporter-agent-token-form")).toBeNull();
  });

  it("supporter_connected_state_explicitly_says_token_set", async () => {
    statusBody = statusFixture("active");
    await render();
    expect(q("supporter-agent-overall-status")?.textContent).toBe("Active");
    expect(q("supporter-agent-token-state")?.textContent).toBe("Token set");
    expect(host.querySelector('label[for="supporter-agent-token-input"]')?.textContent).toContain("Replace token");
    expect(q<HTMLButtonElement>("supporter-agent-token-save")?.textContent).toContain("Replace token");
    expect(input().value).toBe("");
  });

  it("supporter_card_shows_activity_and_budget_without_toggles", async () => {
    statusBody = statusFixture("budget_reached");
    await render();
    expect(q("supporter-agent-overall-status")?.textContent).toBe("Budget reached");
    expect(host.querySelector('[role="switch"]')).toBeNull();
    expect(q("supporter-agent-activity")?.textContent).toContain("12 in the last hour");
    expect(q("supporter-agent-activity")?.textContent).toContain("45 in the last day");
    expect(q("supporter-agent-activity")?.textContent).toContain("Available again:");
    expect(q("supporter-agent-activity")?.textContent).toContain("ready_idle");
  });

  it("supporter_status_renders_no_cap", async () => {
    statusBody = statusFixture("active");
    statusBody.budget.calls_per_hour = null;
    await render();
    expect(host.querySelector('[data-testid="supporter-agent-activity"]')?.getAttribute("data-cap")).toBe("none");
  });

  it("supporter_card_shows_usage_and_estimated_spend_and_saves_balance", async () => {
    statusBody = statusFixture("active");
    await render();
    const spend = host.querySelector('[role="status"][aria-label="Supporter spend"]');
    expect(spend?.textContent).toContain("$0.042000 estimated");
    expect(spend?.textContent).toContain("$0.084000 estimated");
    expect(spend?.textContent).toContain("Remaining: $4.9580 (estimate)");
    const balance = host.querySelector<HTMLInputElement>('[aria-label="Balance (USD)"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(balance, "10");
      balance.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () =>
      balance.closest("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    expect(calls.find((call) => call.url.endsWith("/api/supporter/balance"))?.body).toEqual({
      adapter: ADAPTER,
      balance_usd: 10,
    });
    expect(spend?.textContent).toContain("Remaining: $10.0000 (estimate)");
    expect(host.querySelector('[role="switch"]')).toBeNull();
  });

  it("supporter_card_shows_credit_fallback_and_unpriced_tokens_honestly", async () => {
    statusBody = statusFixture("credit_exhausted");
    statusBody.spend.today.estimated_usd = null;
    statusBody.spend.month.estimated_usd = null;
    statusBody.spend.remaining_usd = null;
    await render();
    expect(q("supporter-agent-overall-status")?.textContent).toBe("Balance exhausted");
    expect(host.textContent).toContain("Price not reported");
    expect(host.textContent).toContain("Supporter uses deterministic results");
    expect(host.textContent).not.toContain("Remaining:");
  });

  it("supporter_token_input_is_write_only_and_refreshed", async () => {
    await render();
    await typeAndSubmit(TOKEN);
    expect(input().value).toBe("");
    expect(statusBody.state).toBe("active");

    writeRejects = true;
    await typeAndSubmit(TOKEN);
    expect(input().value).toBe("");
    writeRejects = false;
    writeStatus = 500;
    await typeAndSubmit(TOKEN);
    expect(input().value).toBe("");
    expect(host.innerHTML).not.toContain(TOKEN);
    expect(
      calls.filter((call) => call.method === "GET" && call.url.endsWith("/api/supporter/status")).length,
    ).toBeGreaterThan(1);
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

  it("supporter_activity_refresh_reads_new_evidence", async () => {
    await render();
    statusBody = { ...statusBody, calls_last_hour: 23 };
    await act(async () =>
      Array.from(host.querySelectorAll("button"))
        .find((button) => button.textContent === "Refresh activity")!
        .click(),
    );
    expect(q("supporter-agent-activity")?.textContent).toContain("23 in the last hour");
  });

  it("supporter_status_has_accessible_names_and_token_never_renders", async () => {
    await render();
    const focusables = Array.from(host.querySelectorAll<HTMLElement>("button, input, select, a[href]"));
    const names = focusables.map(
      (element) =>
        element.getAttribute("aria-label") ||
        element.textContent?.trim() ||
        (element.id ? host.querySelector(`label[for="${element.id}"]`)?.textContent?.trim() : "") ||
        "",
    );
    expect(names.every((name) => name.length > 0)).toBe(true);
    expect(host.querySelector('[role="status"][aria-label="Supporter Agent status"]')).not.toBeNull();
    expect(document.body.innerHTML).not.toContain(TOKEN);
  });
});
