import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SupporterAgentCard } from "./SupporterAgentCard";
import { useDecisionToken, type SupporterControls } from "./useDecisionToken";
import type { SupporterAdapter, SupporterConnection, SupporterStatus } from "./supporterTypes";
import { statusFixture } from "../Supporter/testSupport";
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
  metadata: { display_name: `${mode} provider` },
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
    metadata: { display_name: `${adapter} provider` },
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
let failNextStatus: boolean;
let failDeleteReadback: boolean;
let nextFailure: { status: number; body: string } | null;
let unknown: boolean;
let gate: Promise<void> | null;
let release: (() => void) | undefined;
let partial: boolean;
let invalidStatus: boolean;
let invalidCatalog: boolean;
let activity: SupporterActivity[];
function stop() {
  state.adapter = null;
  state.model = null;
  state.connected = false;
  state.state = "not_connected";
}
const listeners = new Set<(event: { payload: unknown }) => void>();

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  state = snapshot();
  calls = [];
  stale = false;
  failNextStatus = false;
  failDeleteReadback = false;
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
          if (stale || failNextStatus) {
            failNextStatus = false;
            throw new Error("offline");
          }
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
        const partialError = adapter === "required"
          ? { code: "authentication_failed", message: "Authentication failed" }
          : { code: "unavailable", message: "Retry activation" };
        saved.model = String(body?.model);
        saved.token.present = Boolean(body?.token);
        saved.token.mode_ok = partial ? false : true;
        if (partial) saved.error = partialError;
        if (!partial) {
          for (const entry of state.connections) entry.active = false;
          state.adapter = adapter;
          state.model = saved.model;
          state.connected = true;
          state.state = "active";
        }
        state.connections.push(saved);
        return Response.json(
          partial
            ? {
              state: "saved_not_active",
              adapter,
              error: partialError,
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
        state.model = saved?.model ?? null;
        state.connected = true;
        state.state = "active";
      }
      if (action === "model" && saved) {
        saved.model = String(body?.model);
        if (saved.active) state.model = saved.model;
      }
      if (action === "token" && saved) {
        saved.token.present = true;
        saved.token.mode_ok = true;
        saved.error = null;
        return Response.json({ adapter, state: "saved_not_active" });
      }
      if (method === "DELETE") {
        state.connections = state.connections.filter((entry) => entry.adapter !== adapter);
        if (match && !action && failDeleteReadback) {
          failDeleteReadback = false;
          failNextStatus = true;
        }
        if (state.adapter === adapter) {
          stop();
          for (const entry of state.connections) entry.active = false;
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
        apiBase="http://127.0.0.1:7686"
        apiHeaders={{ Authorization: "Bearer fixture-auth" }}
        onOpenGuide={() => {}}
      />,
    ),
  );
}
function button(name: string, scope: ParentNode = host) {
  const found = Array.from(scope.querySelectorAll<HTMLButtonElement>("button")).find(
    (button) => button.textContent?.trim() === name || button.getAttribute("aria-label") === name,
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
  const direct = Array.from((scope ?? host).querySelectorAll<HTMLButtonElement>("button")).find(
    (candidate) => candidate.textContent?.trim() === name || candidate.getAttribute("aria-label") === name,
  );
  if (direct) {
    await act(async () => direct.click());
    return;
  }
  const adapter = (scope as HTMLElement | undefined)?.dataset.adapter;
  if (!adapter) throw new Error(`button ${name} not found`);
  const item = await menuItem(adapter, name);
  await act(async () => item.click());
}
async function menuItem(adapter: string, label: string) {
  const row = section(adapter);
  const trigger = row.querySelector<HTMLButtonElement>('button[aria-haspopup="menu"]')!;
  if (trigger.getAttribute("aria-expanded") !== "true") {
    await act(async () => trigger.dispatchEvent(new Event("pointerdown", { bubbles: true })));
  }
  const menuLabel = trigger.getAttribute("aria-label")?.replace("Actions for ", "") + " connection actions";
  const menu = Array.from(document.querySelectorAll<HTMLElement>('[role="menu"]')).find(
    (candidate) => candidate.getAttribute("aria-label") === menuLabel,
  );
  expect(menu, `${adapter} connection menu`).toBeDefined();
  const item = Array.from(menu!.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')).find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  expect(item, `menu item ${label}`).toBeDefined();
  return item!;
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
  const item = await menuItem(adapter, "Delete");
  await act(async () => item.click());
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
    await click("Refresh Spinal Cord status and models");
    expect(section("required")).toBeDefined();
    expect(host.textContent).toContain("Status stale");
    expect((await menuItem("optional", "Make active")).disabled).toBe(true);
    invalidStatus = false;
    await click("Refresh Spinal Cord status and models");
    expect((await menuItem("optional", "Make active")).disabled).toBe(false);
  });
  it("malformed_catalogue_keeps_rows_and_disables_model_selection", async () => {
    await mount();
    invalidCatalog = true;
    await click("Refresh Spinal Cord status and models");
    expect(section("required")).toBeDefined();
    expect(host.textContent).toContain("catalogue is unavailable");
    expect(host.querySelector<HTMLSelectElement>("#supporter-model-required")?.disabled).toBe(true);
    expect(button("+ Connect another adapter").disabled).toBe(true);
  });
  it("connections_render_as_one_compact_list_without_duplicate_current_header", async () => {
    await mount();
    const rows = host.querySelectorAll("[data-adapter]");
    expect((rows[0] as HTMLElement).dataset.adapter).toBe("required");
    expect(host.querySelector("[aria-label='Current Spinal Cord']")).toBeNull();
    expect(host.querySelector("[aria-label='Saved inactive connections']")).toBeNull();
    expect(host.querySelector("section[aria-label='Spinal Cord connections']")).not.toBeNull();
    expect(rows).toHaveLength(2);
    expect(host.textContent).toContain("Ready");
    expect(section("required").textContent).toContain("required One");
    expect(section("required").textContent).toContain("Token saved");
    expect(section("required").textContent).toContain("req/hr");
    expect(section("optional").textContent).toContain("Inactive");
    expect(section("optional").querySelector("details")).toBeNull();
    expect(section("optional").textContent).not.toContain("Model unavailable");
    expect(section("optional").textContent).not.toContain("Token requirements unknown");
    expect(host.querySelector('[aria-label="Actions for required provider"]')).not.toBeNull();
    expect(host.querySelector('[aria-label="Actions for optional provider"]')).not.toBeNull();
    expect(section("required").querySelectorAll("select")).toHaveLength(1);
    expect(writes()).toHaveLength(0);
  });
  it("connection_menu_has_accessible_roles_and_escape_restores_trigger_focus", async () => {
    await mount();
    const trigger = section("optional").querySelector<HTMLButtonElement>('button[aria-haspopup="menu"]')!;
    expect(trigger.getAttribute("aria-label")).toBe("Actions for optional provider");
    await act(async () => trigger.dispatchEvent(new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      cancelable: true,
    })));
    const menu = document.querySelector<HTMLElement>('[role="menu"][aria-label="optional provider connection actions"]')!;
    expect(menu).not.toBeNull();
    const labels = Array.from(menu.querySelectorAll('[role="menuitem"]')).map((item) => item.textContent?.trim());
    expect(labels).toEqual(["Make active", "Delete"]);
    const escape = new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    });
    await act(async () => menu.dispatchEvent(escape));
    expect(escape.defaultPrevented).toBe(true);
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });
  it("selected_disconnected_connection_keeps_its_name_without_claiming_readiness", async () => {
    state.connected = false;
    state.state = "not_connected";
    await mount();
    expect(section("required").textContent).toContain("required provider");
    expect(section("required").textContent).toContain("required One");
    expect(host.textContent).toContain("Disconnected");
    expect(section("required").textContent).not.toContain("Ready");
  });
  it("budget_exhaustion_is_not_presented_as_ready", async () => {
    state.state = "budget_reached";
    state.budget.reached = true;
    await mount();
    expect(host.textContent).toContain("Budget exhausted");
    expect(host.textContent).not.toContain("Ready");
  });
  it("selected_required_token_failure_is_not_ready", async () => {
    state.connections.find((entry) => entry.adapter === "required")!.token = { present: false, mode_ok: false };
    await mount();
    expect(host.textContent).toContain("Token required");
    expect(section("required").textContent).not.toContain("Ready");
  });
  it("incoherent_selection_flags_are_stale_and_do_not_choose_a_winner", async () => {
    await mount();
    state.connections.find((entry) => entry.adapter === "optional")!.active = true;
    await click("Refresh Spinal Cord status and models");
    expect(host.textContent).toContain("Status stale");
    expect(section("required").querySelector('[role="status"][aria-label]')).not.toBeNull();
    expect((await menuItem("optional", "Make active")).disabled).toBe(true);
    expect(writes()).toHaveLength(0);
  });
  it("inactive_connections_are_read_only", async () => {
    await mount();
    const inactive = section("optional");
    expect(inactive.querySelector("select")).toBeNull();
    expect(inactive.textContent).toContain("Inactive");
    expect(inactive.querySelector("input[type='password']")).toBeNull();
    expect(host.innerHTML).not.toContain(TOKEN);
    const activate = await menuItem("optional", "Make active");
    expect(activate.disabled).toBe(false);
    await act(async () => activate.click());
    expect(state.adapter).toBe("optional");
    expect(state.model).toBe("optional-one");
    expect(writes()).toEqual([
      {
        path: "/api/supporter/connections/optional/activate",
        method: "POST",
        body: null,
      },
    ]);
  });
  it("current_connection_model_change_updates_the_selected_model", async () => {
    await mount();
    await choose("#supporter-model-required", "required-two");
    expect(state.adapter).toBe("required");
    expect(state.model).toBe("required-two");
    expect(writes()).toEqual([
      {
        path: "/api/supporter/connections/required/model",
        method: "PUT",
        body: { model: "required-two" },
      },
    ]);
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
    expect((await menuItem("optional", "Retry activation")).disabled).toBe(false);
    expect(section("required").textContent).toContain("Active");
    await click("Retry activation", section("optional"));
    expect(state.adapter).toBe("optional");
  });
  it.each(["required", "optional", "none"])("connect_panel_token_field_per_token_mode: %s", async (mode) => {
    state.connections = [];
    stop();
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
    stop();
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
    stop();
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
    expect(host.querySelector("[aria-live]")?.textContent).toContain("none provider is active");
    expect(section("required").textContent).toContain("Token saved, hidden");
  });
  it("remove_confirm_escape_stops_settings_close_and_restores_focus", async () => {
    await mount();
    await confirm("optional");
    const dialog = section("optional").querySelector("[role='alertdialog']")!;
    expect(document.activeElement).toBe(button("Keep", dialog));
    let settingsClosed = 0;
    const closeSettings = (event: KeyboardEvent) => {
      if (event.key === "Escape") settingsClosed++;
    };
    document.addEventListener("keydown", closeSettings);
    const escape = new KeyboardEvent("keydown", {
      key: "Escape",
      code: "Escape",
      bubbles: true,
      cancelable: true,
    });
    await act(async () => dialog.dispatchEvent(escape));
    document.removeEventListener("keydown", closeSettings);
    expect(escape.defaultPrevented).toBe(true);
    expect(settingsClosed).toBe(0);
    expect(host.querySelector("[role='alertdialog']")).toBeNull();
    expect(document.activeElement).toBe(section("optional").querySelector('button[aria-haspopup="menu"]'));
    expect(writes()).toHaveLength(0);
  });
  it("remove_keep_cancels_without_mutation_and_restores_focus", async () => {
    await mount();
    await confirm("optional");
    const dialog = section("optional").querySelector("[role='alertdialog']")!;
    await click("Keep", dialog);
    expect(host.querySelector("[role='alertdialog']")).toBeNull();
    expect(document.activeElement).toBe(section("optional").querySelector('button[aria-haspopup="menu"]'));
    expect(writes()).toHaveLength(0);
  });
  it("accepted_delete_failed_readback_is_not_confirmed_removal", async () => {
    await mount();
    failDeleteReadback = true;
    await confirm("optional");
    const dialog = section("optional").querySelector("[role='alertdialog']")!;
    await click("Remove connection", dialog);
    expect(section("optional")).toBeDefined();
    expect(host.textContent).toContain("Status stale");
    expect(host.querySelector("[aria-live='polite']")?.textContent).toBe("");
    expect(
      writes().filter((call) => call.method === "DELETE" && call.path.endsWith("/connections/optional")),
    ).toHaveLength(1);
    expect(state.adapter).toBe("required");
    expect(state.model).toBe("required-one");
    await click("Refresh Spinal Cord status and models");
    expect(host.querySelector("[data-adapter='optional']")).toBeNull();
    expect(host.querySelector("[aria-live='polite']")?.textContent).toContain("optional provider removed");
    expect(state.adapter).toBe("required");
    expect(state.model).toBe("required-one");
    expect(document.activeElement).toBe(section("required"));
    expect(
      writes().filter((call) => call.method === "DELETE" && call.path.endsWith("/connections/optional")),
    ).toHaveLength(1);
  });
  it("inactive_removal_preserves_the_selected_connection_and_focuses_it", async () => {
    await mount();
    await confirm("optional");
    const dialog = section("optional").querySelector("[role='alertdialog']")!;
    expect(dialog.getAttribute("aria-label")).toContain("optional provider");
    await click("Remove connection", dialog);
    expect(host.querySelector("[data-adapter='optional']")).toBeNull();
    expect(state.adapter).toBe("required");
    expect(state.model).toBe("required-one");
    expect(document.activeElement).toBe(section("required"));
    expect(host.querySelector("[aria-live='polite']")?.textContent).toContain("current Spinal Cord is unchanged");
  });
  it("failed_delete_keeps_the_row_and_selection_for_retry", async () => {
    await mount();
    await confirm("optional");
    const dialog = section("optional").querySelector("[role='alertdialog']")!;
    nextFailure = {
      status: 503,
      body: JSON.stringify({ error: { code: "unavailable", message: "Removal unavailable" } }),
    };
    await click("Remove connection", dialog);
    expect(section("optional")).toBeDefined();
    expect(host.querySelector("[role='alertdialog']")).not.toBeNull();
    expect(host.textContent).toContain("Removal unavailable");
    expect(state.adapter).toBe("required");
    expect(state.model).toBe("required-one");
    expect(writes().filter((call) => call.method === "DELETE")).toHaveLength(1);
  });
  it("removing_the_last_connection_leaves_a_usable_empty_state", async () => {
    state.connections = [row("required", true)];
    await mount();
    await confirm("required");
    const dialog = section("required").querySelector("[role='alertdialog']")!;
    expect(dialog.textContent).toContain("no automatic fallback");
    await click("Remove connection", dialog);
    expect(host.querySelectorAll("[data-adapter]")).toHaveLength(0);
    expect(host.textContent).toContain("No saved connections.");
    expect(state.adapter).toBeNull();
    expect(button("+ Connect another adapter").disabled).toBe(false);
    expect(document.activeElement).toBe(button("+ Connect another adapter"));
  });
  it("pending_removal_escape_does_not_cancel_or_close_settings", async () => {
    await mount();
    await confirm("optional");
    const dialog = section("optional").querySelector("[role='alertdialog']")!;
    hold();
    await click("Remove connection", dialog);
    expect(button("Remove connection", dialog).disabled).toBe(true);
    let settingsClosed = 0;
    const closeSettings = (event: KeyboardEvent) => {
      if (event.key === "Escape") settingsClosed++;
    };
    document.addEventListener("keydown", closeSettings);
    const escape = new KeyboardEvent("keydown", {
      key: "Escape",
      code: "Escape",
      bubbles: true,
      cancelable: true,
    });
    await act(async () => dialog.dispatchEvent(escape));
    document.removeEventListener("keydown", closeSettings);
    expect(settingsClosed).toBe(0);
    expect(host.querySelector("[role='alertdialog']")).not.toBeNull();
    expect(writes().filter((call) => call.method === "DELETE")).toHaveLength(1);
    await resume();
    expect(host.querySelector("[data-adapter='optional']")).toBeNull();
  });
  it("remove_active_stops_with_no_fallback", async () => {
    await mount();
    await confirm("required");
    const dialog = section("required").querySelector("[role='alertdialog']")!;
    expect(dialog.getAttribute("aria-label")).toContain("required provider");
    expect(dialog.textContent).toContain("no automatic fallback");
    await click("Remove connection", dialog);
    expect(state.adapter).toBeNull();
    expect(state.model).toBeNull();
    expect(section("optional").textContent).toContain("Inactive");
    expect(host.querySelector("[data-adapter='required']")).toBeNull();
    expect(document.activeElement).toBe(section("optional"));
    expect(host.querySelector("[aria-live='polite']")?.textContent).toContain("Spinal Cord stopped");
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
  it("pending_guard_rejects_overlapping_activation_and_removal", async () => {
    let controls: SupporterControls;
    function Hook() {
      controls = useDecisionToken("http://127.0.0.1:7686", { Authorization: "Bearer fixture-auth" });
      return null;
    }
    await act(async () => root.render(<Hook />));
    hold();
    let activation!: ReturnType<SupporterControls["activate"]>;
    let removal!: ReturnType<SupporterControls["disconnect"]>;
    await act(async () => {
      activation = controls.activate("optional");
      removal = controls.disconnect("required");
    });
    expect(writes()).toHaveLength(1);
    expect(writes()[0].path).toBe("/api/supporter/connections/optional/activate");
    await resume();
    await act(async () => {
      await Promise.all([activation, removal]);
    });
    expect(state.adapter).toBe("optional");
    expect(writes().filter((call) => call.method === "DELETE")).toHaveLength(0);
  });
  it("stale_status_keeps_rows_disables_destructive", async () => {
    await mount();
    await confirm("optional");
    stale = true;
    await click("Refresh Spinal Cord status and models");
    expect(host.textContent).toContain("stale");
    expect(section("required")).toBeDefined();
    const dialog = section("optional").querySelector("[role='alertdialog']")!;
    expect(button("Remove connection", dialog).disabled).toBe(true);
    expect((await menuItem("optional", "Make active")).disabled).toBe(true);
    expect(section("required").querySelector('[role="status"][aria-label]')).not.toBeNull();
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
  it("missing_required_token_has_explicit_write_only_repair_without_activation", async () => {
    const missing = row("required", false);
    missing.token = { present: false, mode_ok: false };
    state.connections = [row("optional", true), missing];
    state.adapter = "optional";
    state.model = "optional-one";
    await mount();
    const inactive = section("required");
    expect((await menuItem("required", "Repair credentials")).disabled).toBe(false);
    expect(inactive.querySelector("select")).toBeNull();
    expect(inactive.querySelector("input[type='password']")).toBeNull();
    await click("Repair credentials", inactive);
    await click("Add token", inactive);
    const input = inactive.querySelector<HTMLInputElement>("input[type='password']")!;
    expect(input).not.toBeNull();
    input.value = TOKEN;
    await act(async () => input.closest("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
    expect(input.value).toBe("");
    expect(host.innerHTML).not.toContain(TOKEN);
    expect(host.textContent).not.toContain(TOKEN);
    expect(state.adapter).toBe("optional");
    expect(state.model).toBe("optional-one");
    expect(state.connections.find((entry) => entry.adapter === "required")?.active).toBe(false);
    expect(writes().filter((call) => call.path.endsWith("/connections/required/token"))).toEqual([
      {
        path: "/api/supporter/connections/required/token",
        method: "POST",
        body: { token: TOKEN },
      },
    ]);
  });
  it("failed_saved_connection_can_repair_credentials_then_activate_explicitly", async () => {
    state.connections = [row("optional", true)];
    state.adapter = "optional";
    state.model = "optional-one";
    partial = true;
    await mount();
    await panel("required");
    host.querySelector<HTMLInputElement>("#supporter-connect-token")!.value = TOKEN;
    await submit();
    const inactive = section("required");
    expect(inactive.textContent).toContain("Saved, not active");
    expect(inactive.textContent).toContain("Authentication failed");
    expect((await menuItem("required", "Repair credentials")).disabled).toBe(false);
    expect(state.adapter).toBe("optional");
    partial = false;
    await click("Repair credentials", inactive);
    await click("Replace token", inactive);
    const input = inactive.querySelector<HTMLInputElement>("input[type='password']")!;
    input.value = TOKEN;
    await act(async () => input.closest("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
    expect(input.value).toBe("");
    expect(state.adapter).toBe("optional");
    expect(state.connections.find((entry) => entry.adapter === "required")?.active).toBe(false);
    expect(host.innerHTML).not.toContain(TOKEN);
    await click("Retry activation", inactive);
    expect(state.adapter).toBe("required");
    expect(state.model).toBe("required-one");
  });
  it("money_only_for_priced_adapter", async () => {
    await mount();
    expect(section("required").querySelector("[aria-label='Spinal Cord cost']")).not.toBeNull();
    expect(section("optional").querySelector("[aria-label='Spinal Cord cost']")).toBeNull();
    await click("Make active", section("optional"));
    expect(host.querySelector("[aria-label='Spinal Cord cost']")).toBeNull();
  });
  it("unknown_price_not_zero", async () => {
    state.spend.today.estimated_usd = null;
    state.spend.month.estimated_usd = null;
    await mount();
    expect(host.textContent).toContain("price not reported");
    expect(host.textContent).not.toContain("$0.0000");
  });
  it("unpriced_shows_counts_without_cost", async () => {
    state.connections = [row("none", true)];
    state.adapter = "none";
    state.model = "none-one";
    state.pricing = null;
    await mount();
    expect(host.textContent).toContain("req/hr");
    expect(host.querySelector("[aria-label='Spinal Cord cost']")).toBeNull();
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
    state.model = "none-one";
    await mount();
    expect(host.querySelector("[aria-label='Spinal Cord spend']")).toBeNull();
  });
  it("write_surfaces_plain_text_errors", async () => {
    await mount();
    nextFailure = { status: 503, body: "Adapter endpoint refused the request" };
    await click("Make active", section("optional"));
    expect(host.textContent).toContain("Adapter endpoint refused the request");
  });
  it("priced_adapter_shows_estimated_spend", async () => {
    await mount();
    expect(section("required").querySelector('[aria-label="Spinal Cord cost"]')).not.toBeNull();
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
