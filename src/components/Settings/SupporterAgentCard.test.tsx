import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SupporterAgentCard } from "./SupporterAgentCard";
import { useDecisionToken, type SupporterControls } from "./useDecisionToken";
import type { SupporterAdapter, SupporterConnection, SupporterStatus } from "./supporterTypes";
import { API, statusFixture } from "../Supporter/testSupport";
import { __setListenForTest } from "../../lib/bridge";
import type { SupporterActivity } from "../Supporter/activity";

vi.mock("../../lib/webApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/webApi")>()),
  daemonFetch: (_api: unknown, path: string, init: RequestInit) => fetch(`http://127.0.0.1:7686${path}`, init),
  daemonAuthToken: () => "fixture-auth",
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const TOKEN = "opaque-secret-do-not-render";
const price = {
  display_name: "Priced fixture",
  input_usd_per_million: 1,
  output_usd_per_million: 1,
};
const catalog: SupporterAdapter[] = ["required", "optional", "none"].map((mode) => ({
  id: mode,
  token_mode: mode as SupporterAdapter["token_mode"],
  primitives: { choice: true, noul: true, score: true },
  models: [
    { id: `${mode}-one`, display_name: `${mode} One` },
    { id: `${mode}-two`, display_name: `${mode} Two` },
  ],
  pricing: mode === "required" ? price : null,
}));
function row(adapter: string, active: boolean): SupporterConnection {
  return {
    adapter,
    model: `${adapter}-one`,
    active,
    token_mode: adapter as SupporterConnection["token_mode"],
    token: { present: adapter === "required", mode_ok: true },
    pricing: adapter === "required" ? price : null,
  };
}
function snapshot(): SupporterStatus {
  return {
    ...statusFixture(),
    adapter: "required",
    model: "required-one",
    connections: [row("optional", false), row("required", true)],
    adapters: catalog,
    pricing: price,
  };
}
interface Call {
  path: string;
  method: string;
  body: Record<string, unknown> | null;
}
let host: HTMLDivElement;
let root: Root;
let state: SupporterStatus;
let calls: Call[];
let stale: boolean;
let nextFailure: { status: number; body: string } | null;
let unknown: boolean;
let gate: Promise<void> | null;
let release: (() => void) | undefined;
let partial: boolean;
let invalidStatus: boolean;
let invalidCatalog: boolean;
let activity: SupporterActivity[];
const listeners = new Set<(event: { payload: unknown }) => void>();

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  state = snapshot();
  calls = [];
  stale = false;
  nextFailure = null;
  unknown = false;
  gate = null;
  partial = false;
  invalidStatus = false;
  invalidCatalog = false;
  activity = [];
  listeners.clear();
  __setListenForTest(async (_name, callback) => {
    const listener = callback as (event: { payload: unknown }) => void;
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const path = new URL(url).pathname;
      const method = init?.method ?? "GET";
      const body = init?.body ? (JSON.parse(String(init.body)) as Record<string, unknown>) : null;
      expect(new Headers(init?.headers).get("Authorization")).toBe("Bearer fixture-auth");
      calls.push({ path, method, body });
      if (method === "GET") {
        if (path.endsWith("/activity")) return Response.json({ entries: activity });
        if (path.endsWith("/status")) {
          if (stale) throw new Error("offline");
          return Response.json(invalidStatus ? { connections: [] } : state);
        }
        return Response.json(invalidCatalog ? { adapters: null } : { adapters: catalog });
      }
      if (gate) await gate;
      if (unknown) {
        unknown = false;
        throw new Error("unknown outcome");
      }
      if (nextFailure) {
        const failure = nextFailure;
        nextFailure = null;
        return new Response(failure.body, { status: failure.status });
      }
      if (path.endsWith("/connections")) {
        const adapter = String(body?.adapter);
        const saved = row(adapter, !partial);
        saved.model = String(body?.model);
        saved.token.present = Boolean(body?.token);
        if (!partial) {
          for (const entry of state.connections) entry.active = false;
          state.adapter = adapter;
        }
        state.connections.push(saved);
        return Response.json(
          partial
            ? {
                state: "saved_not_active",
                adapter,
                error: { code: "unavailable", message: "Retry activation" },
              }
            : { state: "active", adapter },
        );
      }
      const match = path.match(/connections\/([^/]+)(?:\/(\w+))?$/);
      const adapter = match?.[1] ?? "";
      const action = match?.[2];
      const saved = state.connections.find((entry) => entry.adapter === adapter);
      if (action === "activate") {
        for (const entry of state.connections) entry.active = entry.adapter === adapter;
        state.adapter = adapter;
      }
      if (action === "model" && saved) saved.model = String(body?.model);
      if (method === "DELETE") {
        state.connections = state.connections.filter((entry) => entry.adapter !== adapter);
        if (state.adapter === adapter) {
          state.adapter = null;
          state.connected = false;
        }
      }
      return Response.json({ adapter, state: "active" });
    }),
  );
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  __setListenForTest(null);
  expect(listeners.size).toBe(0);
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});
const writes = () => calls.filter((call) => call.method !== "GET");
async function mount() {
  await act(async () =>
    root.render(
      <SupporterAgentCard
        api={API}
        apiBase="http://127.0.0.1:7686"
        apiHeaders={{ Authorization: "Bearer fixture-auth" }}
        onOpenGuide={() => {}}
      />,
    ),
  );
}
function button(name: string, scope: ParentNode = host) {
  const found = Array.from(scope.querySelectorAll<HTMLButtonElement>("button")).find(
    (button) => button.textContent?.trim() === name,
  );
  expect(found, `button ${name}`).toBeDefined();
  return found!;
}
function section(adapter: string) {
  const value = host.querySelector<HTMLElement>(`[data-adapter="${adapter}"]`);
  expect(value).not.toBeNull();
  return value!;
}
async function click(name: string, scope?: ParentNode) {
  await act(async () => button(name, scope).click());
}
async function choose(selector: string, value: string) {
  const select = host.querySelector<HTMLSelectElement>(selector)!;
  expect(select).not.toBeNull();
  await act(async () => {
    select.value = value;
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
}
async function panel(mode = "none") {
  await click("+ Connect another adapter");
  await choose("#supporter-connect-adapter", mode);
  await choose("#supporter-connect-model", `${mode}-one`);
}
async function submit() {
  await act(async () =>
    host
      .querySelector("form[aria-label='Connect another adapter']")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
}
async function confirm(adapter: string) {
  await act(async () => section(adapter).querySelector("summary")!.click());
  await click("Remove connection", section(adapter));
}
function hold() {
  gate = new Promise<void>((resolve) => {
    release = resolve;
  });
}
async function resume() {
  await act(async () => release?.());
  gate = null;
}

describe("Supporter saved connections", () => {
  it("compact_details_embeds_heartbeat_activity_and_refreshes_on_journal_events", async () => {
    activity = [
      {
        time: 1700000000,
        team_id: null,
        kind: "heartbeat",
        label: "action_needed",
        action: "wake",
        reason: "hard_wake",
        confidence_band: "strong",
        budget_reached: false,
        fallback: false,
      },
    ];
    await mount();
    const feed = section("required").querySelector("section[aria-label='Spinal-cord activity']")!;
    expect(feed).not.toBeNull();
    const details = feed.closest("details")!;
    expect(details.querySelector("summary")?.textContent).toBe("Details");
    expect(details.open).toBe(false);
    expect(feed.querySelector("h3")).toBeNull();
    expect(feed.querySelector("li")?.getAttribute("data-kind")).toBe("heartbeat");
    expect(details.textContent).toContain("Team heartbeats");
    await act(async () => details.querySelector("summary")!.click());
    const count = calls.filter((call) => call.path.endsWith("/activity")).length;
    activity = [{ ...activity[0], label: "nothing_changed", action: "suppress", reason: "same_snapshot" }];
    await act(async () => {
      for (const listener of listeners) listener({ payload: {} });
    });
    expect(calls.filter((call) => call.path.endsWith("/activity"))).toHaveLength(count + 1);
    expect(feed.querySelector("li")?.textContent).toContain("Action: suppress");
    await click("Make active", section("optional"));
    expect(section("required").querySelector("section[aria-label='Spinal-cord activity']")).toBeNull();
    expect(section("optional").querySelector("section[aria-label='Spinal-cord activity']")).not.toBeNull();
  });
  it("initial_unknown_status_is_unavailable_without_token_claims", async () => {
    stale = true;
    await mount();
    expect(host.textContent).toContain("Status unavailable");
    expect(host.textContent).not.toContain("No token");
    expect(host.querySelectorAll("[data-adapter]")).toHaveLength(0);
    expect(button("+ Connect another adapter").disabled).toBe(true);
    expect(writes()).toHaveLength(0);
  });
  it("malformed_status_keeps_authoritative_rows_stale", async () => {
    await mount();
    invalidStatus = true;
    await click("Refresh");
    expect(section("required")).toBeDefined();
    expect(host.textContent).toContain("Status stale");
    expect(button("Make active", section("optional")).disabled).toBe(true);
  });
  it("malformed_catalogue_keeps_rows_and_disables_model_selection", async () => {
    await mount();
    invalidCatalog = true;
    await click("Refresh");
    expect(section("required")).toBeDefined();
    expect(host.textContent).toContain("catalogue is unavailable");
    expect(host.querySelector<HTMLSelectElement>("#supporter-model-required")?.disabled).toBe(true);
    expect(button("+ Connect another adapter").disabled).toBe(true);
  });
  it("rows_active_first_name_is_text", async () => {
    await mount();
    const rows = host.querySelectorAll("[data-adapter]");
    expect((rows[0] as HTMLElement).dataset.adapter).toBe("required");
    expect(section("required").querySelectorAll("select")).toHaveLength(1);
    expect(section("required").textContent).toContain("Configured connection");
    expect(writes()).toHaveLength(0);
  });
  it("inactive_model_change_does_not_activate", async () => {
    await mount();
    await choose("#supporter-model-optional", "optional-two");
    expect(writes()).toEqual([
      {
        path: "/api/supporter/connections/optional/model",
        method: "PUT",
        body: { model: "optional-two" },
      },
    ]);
    expect(section("required").textContent).toContain("Active");
    expect(state.adapter).toBe("required");
  });
  it("make_active_badge_only_after_success", async () => {
    await mount();
    hold();
    await click("Make active", section("optional"));
    expect(section("optional").textContent).not.toContain("Active");
    expect(section("required").textContent).toContain("Active");
    await resume();
    expect(section("optional").textContent).toContain("Active");
    expect(writes()[0].body).toBeNull();
  });
  it("make_active_failure_keeps_old_active", async () => {
    await mount();
    nextFailure = {
      status: 503,
      body: JSON.stringify({
        error: { code: "unavailable", message: "Server unavailable" },
      }),
    };
    await click("Make active", section("optional"));
    expect(state.adapter).toBe("required");
    expect(section("optional").textContent).toContain("Unavailable");
    expect(section("required").textContent).toContain("Active");
    await click("Retry activation", section("optional"));
    expect(state.adapter).toBe("optional");
  });
  it.each(["required", "optional", "none"])("connect_panel_token_field_per_token_mode: %s", async (mode) => {
    state.connections = [];
    state.adapter = null;
    await mount();
    await panel(mode);
    const input = host.querySelector<HTMLInputElement>("#supporter-connect-token");
    if (mode === "none") expect(input).toBeNull();
    else {
      expect(input?.type).toBe("password");
      expect(input?.required).toBe(mode === "required");
    }
  });
  it("connect_required_token_is_write_only_and_missing_token_cannot_submit", async () => {
    state.connections = [];
    state.adapter = null;
    await mount();
    await panel("required");
    await submit();
    expect(writes()).toHaveLength(0);
    host.querySelector<HTMLInputElement>("#supporter-connect-token")!.value = TOKEN;
    await submit();
    expect(writes()[0].body).toEqual({ adapter: "required", model: "required-one", token: TOKEN, activate: true });
    expect(state.adapter).toBe("required");
    expect(section("required").textContent).toContain("Token saved, hidden");
    expect(host.innerHTML).not.toContain(TOKEN);
  });
  it("connect_optional_empty_token_is_accepted", async () => {
    state.connections = [];
    state.adapter = null;
    await mount();
    await panel("optional");
    await submit();
    expect(writes()[0].body).toEqual({
      adapter: "optional",
      model: "optional-one",
      activate: true,
    });
    expect(state.adapter).toBe("optional");
  });
  it("connect_cancel_no_mutation", async () => {
    await mount();
    await panel();
    await click("Cancel");
    expect(writes()).toHaveLength(0);
    expect(host.querySelector('form[aria-label="Connect another adapter"]')).toBeNull();
  });
  it("connect_success_focuses_and_announces_new_row", async () => {
    await mount();
    await panel();
    await submit();
    expect(document.activeElement).toBe(section("none"));
    expect(host.querySelector("[aria-live]")?.textContent).toContain("none is active");
    expect(section("required").textContent).toContain("Token saved, hidden");
  });
  it("remove_confirm_keep_focused_escape_cancels_focus_restored", async () => {
    await mount();
    await confirm("optional");
    const dialog = section("optional").querySelector("[role='alertdialog']")!;
    expect(document.activeElement).toBe(button("Keep", dialog));
    await act(async () => dialog.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })));
    expect(host.querySelector("[role='alertdialog']")).toBeNull();
    expect(document.activeElement).toBe(section("optional").querySelector("summary"));
    expect(writes()).toHaveLength(0);
  });
  it("remove_active_stops_with_no_fallback", async () => {
    await mount();
    await confirm("required");
    await click("Remove connection", section("required").querySelector("[role='alertdialog']")!);
    expect(state.adapter).toBeNull();
    expect(section("optional").textContent).not.toContain("Active");
    expect(host.querySelector("[data-adapter='required']")).toBeNull();
  });
  it("unknown_outcome_refreshes_before_retry", async () => {
    await mount();
    unknown = true;
    await click("Make active", section("optional"));
    const mutation = calls.findIndex((call) => call.method === "POST");
    expect(calls[mutation + 1].path).toBe("/api/supporter/status");
    await click("Retry activation", section("optional"));
    expect(state.adapter).toBe("optional");
  });
  it("stale_status_keeps_rows_disables_destructive", async () => {
    await mount();
    await confirm("optional");
    stale = true;
    await click("Refresh");
    expect(host.textContent).toContain("stale");
    expect(section("required")).toBeDefined();
    const dialog = section("optional").querySelector("[role='alertdialog']")!;
    expect(button("Remove connection", dialog).disabled).toBe(true);
    expect(button("Make active", section("optional")).disabled).toBe(true);
    expect(host.textContent).not.toContain("No token");
  });
  it("saved_not_active_shows_retry_activation", async () => {
    await mount();
    partial = true;
    await panel();
    await submit();
    expect(section("none").textContent).toContain("Saved, not active");
    expect(state.adapter).toBe("required");
    partial = false;
    await click("Retry activation", section("none"));
    expect(state.adapter).toBe("none");
  });
  it("money_only_for_priced_adapter", async () => {
    await mount();
    expect(section("required").querySelector("[aria-label='Supporter spend']")).not.toBeNull();
    expect(section("optional").querySelector("[aria-label='Supporter spend']")).toBeNull();
    await click("Make active", section("optional"));
    expect(host.querySelector("[aria-label='Supporter spend']")).toBeNull();
    expect(host.querySelector("#supporter-balance")).toBeNull();
  });
  it("unknown_price_not_zero", async () => {
    state.spend.today.estimated_usd = null;
    state.spend.month.estimated_usd = null;
    state.spend.remaining_usd = null;
    await mount();
    expect(host.textContent).toContain("price not reported");
    expect(host.textContent).not.toContain("$0.0000");
  });
  it("unpriced_shows_counts_no_balance", async () => {
    state.connections = [row("none", true)];
    state.adapter = "none";
    state.pricing = null;
    await mount();
    expect(host.textContent).toContain("requests in the last hour");
    expect(host.textContent).toContain("out tokens");
    expect(host.querySelector("#supporter-balance")).toBeNull();
  });
  it("zero_price_local_adapter_shows_counts_without_money", async () => {
    state.connections = [
      {
        ...row("none", true),
        pricing: {
          ...price,
          input_usd_per_million: 0,
          output_usd_per_million: 0,
        },
      },
    ];
    state.adapter = "none";
    await mount();
    expect(host.querySelector("[aria-label='Supporter spend']")).toBeNull();
    expect(host.querySelector("#supporter-balance")).toBeNull();
  });
  it("write_surfaces_plain_text_errors", async () => {
    await mount();
    nextFailure = { status: 503, body: "Adapter endpoint refused the request" };
    await click("Make active", section("optional"));
    expect(host.textContent).toContain("Adapter endpoint refused the request");
  });
  it("manual_balance_write_is_scoped_to_the_active_priced_adapter", async () => {
    await mount();
    const input = section("required").querySelector<HTMLInputElement>("#supporter-balance")!;
    expect(input).not.toBeNull();
    await act(async () => input.closest("details")!.querySelector("summary")!.click());
    await act(async () =>
      input.closest("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    expect(writes()[0]).toEqual({
      path: "/api/supporter/balance",
      method: "POST",
      body: { adapter: "required", balance_usd: state.spend.balance_usd },
    });
    expect(section("optional").querySelector("#supporter-balance")).toBeNull();
    expect(state.adapter).toBe("required");
  });
  it("clear_token_is_adapter_scoped_and_returns_an_honest_success_flag", async () => {
    let controls: SupporterControls;
    function Hook() {
      controls = useDecisionToken("http://127.0.0.1:7686", { Authorization: "Bearer fixture-auth" });
      return null;
    }
    await act(async () => root.render(<Hook />));
    nextFailure = { status: 503, body: "Token storage unavailable" };
    let success: boolean | undefined;
    await act(async () => {
      success = await controls.clearToken("optional");
    });
    expect(success).toBe(false);
    await act(async () => {
      success = await controls.clearToken("optional");
    });
    expect(success).toBe(true);
    expect(
      vi
        .mocked(fetch)
        .mock.calls.some(
          ([url, init]) => String(url).endsWith("/api/supporter/token?adapter=optional") && init?.method === "DELETE",
        ),
    ).toBe(true);
    expect(state.adapter).toBe("required");
  });
  it("token_never_rendered_and_write_is_adapter_scoped", async () => {
    await mount();
    await click("Replace token", section("required"));
    const input = section("required").querySelector<HTMLInputElement>("input[type='password']")!;
    input.value = TOKEN;
    nextFailure = { status: 401, body: `Invalid token ${TOKEN}` };
    await act(async () =>
      input.closest("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    expect(input.value).toBe("");
    expect(host.innerHTML).not.toContain(TOKEN);
    expect(host.textContent).not.toContain(TOKEN);
    expect(writes()[0]).toEqual({
      path: "/api/supporter/connections/required/token",
      method: "POST",
      body: { token: TOKEN },
    });
  });
});
