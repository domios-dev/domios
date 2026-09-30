// Run: npx tsx src/components/Terminal/tabMenuModel.test.ts

import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { PluginTabMenuState } from "../../lib/pluginTabMenu";
import type { ProviderDescriptor, Tab } from "../../lib/types";
import {
  buildTabMenu,
  type TabMenuEntry,
  type TabMenuInput,
  type TabMenuLeafEntry,
  type TabMenuSubmenuEntry,
  type TabMenuSurface,
} from "./tabMenuModel";
import type { TelegramThreadMenuState } from "./telegramThreadActions";

const SURFACES: TabMenuSurface[] = ["sidebar", "header"];

function descriptor(id: string): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: id,
    transport: "pty",
    system_prompt_delivery: "external",
    available_models: [
      { id: "model-a", display_name: "model-a" },
      { id: "model-b", display_name: "model-b" },
    ],
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
    model_switch: "restart_resume",
    supports_permission_auto_confirm: true,
  } as ProviderDescriptor;
}

const CURRENT = descriptor("fixture-provider");
const OTHER = descriptor("other-provider");

const PLUGIN_ITEM = {
  pluginId: "ring",
  itemId: "receiver",
  label: "fixture-plugin-label",
  action: "receiverTarget" as const,
  iconSvg: '<svg viewBox="0 0 1 1"><path d="M0 0"/></svg>',
  iconColor: "#336699",
};

const BOUND: TelegramThreadMenuState = { bridgeActive: true, topics: "available", bound: true, link: null };

function tab(overrides: Partial<Tab> = {}): Tab {
  return {
    id: "tab-1",
    pty_id: "pty-1",
    tab_type: "Terminal",
    title: "Worker",
    cwd: null,
    agent_type: CURRENT.id,
    agent_session_id: "session-1",
    agent_session_model: "model-a",
    ...overrides,
  };
}

function plugins(boundTabId: string | null): PluginTabMenuState {
  return { items: [PLUGIN_ITEM], receiverTargets: boundTabId ? { ring: boundTabId } : {} };
}

function input(surface: TabMenuSurface, overrides: Partial<TabMenuInput> = {}): TabMenuInput {
  return {
    tab: tab(),
    surface,
    status: null,
    descriptor: CURRENT,
    descriptors: [CURRENT, OTHER],
    activity: { kind: "worker", planId: "plan-1", planTitle: "plan", taskCount: 1, planCount: 1 },
    orchestrators: [],
    canRename: true,
    canClose: true,
    pluginTabMenu: plugins("tab-1"),
    telegramThread: BOUND,
    ...overrides,
  };
}

interface Shape {
  kind: string;
  id: string;
  testId?: string;
  action?: string;
  entries?: Shape[];
}

function shape(entries: Array<TabMenuEntry | TabMenuLeafEntry>): Shape[] {
  return entries
    .filter((entry) => entry.kind !== "separator")
    .map((entry) => {
      if (entry.kind === "submenu") return { kind: entry.kind, id: entry.id, testId: entry.testId, entries: shape(entry.entries) };
      const action = entry.kind === "item" || entry.kind === "check" ? entry.action?.type : undefined;
      return { kind: entry.kind, id: entry.id, testId: entry.testId, action };
    });
}

function submenu(entries: TabMenuEntry[], id: string): TabMenuSubmenuEntry | undefined {
  return entries.find((entry): entry is TabMenuSubmenuEntry => entry.kind === "submenu" && entry.id === id);
}

function child(parent: TabMenuSubmenuEntry | undefined, id: string): TabMenuLeafEntry | undefined {
  return parent?.entries.find((entry) => entry.id === id);
}

describe("tab menu model", () => {
  it("lists the same actions on both surfaces except the header-only auto-confirm toggle", () => {
    const sidebar = shape(buildTabMenu(input("sidebar")));
    const header = shape(buildTabMenu(input("header")));
    const autoConfirm = header.find((entry) => entry.id === "auto-confirm");
    expect(autoConfirm?.kind).toBe("check");
    expect(autoConfirm?.testId).toBe("tab-permission-auto-confirm");
    expect(sidebar.some((entry) => entry.id === "auto-confirm")).toBe(false);
    expect(header.filter((entry) => entry.id !== "auto-confirm")).toEqual(sidebar);
    expect(sidebar.map((entry) => entry.id)).toEqual([
      "session",
      "rename",
      "make-orchestrator",
      "make-planner",
      "model",
      "provider",
      "reconnect",
      "restart",
      "plugins",
      "close",
    ]);
  });

  it("puts plugin contributions and Telegram only inside the Plugins submenu", () => {
    for (const surface of SURFACES) {
      const entries = buildTabMenu(input(surface));
      const pluginsEntry = submenu(entries, "plugins");
      expect(pluginsEntry?.testId).toBe("tab-menu-plugins");
      expect(shape(pluginsEntry?.entries ?? [])).toEqual([
        { kind: "check", id: "plugin-ring-receiver", testId: "tab-menu-plugin-ring-receiver", action: "runPlugin" },
        { kind: "item", id: "telegram-open", testId: "tab-menu-telegram-connect", action: "openTelegram" },
        { kind: "item", id: "telegram-close", testId: "tab-menu-telegram-disconnect", action: "closeTelegram" },
      ]);
      const outside = shape(entries.filter((entry) => entry.id !== "plugins")).map((entry) => entry.action);
      expect(outside.some((action) => action === "runPlugin" || action === "openTelegram" || action === "closeTelegram")).toBe(false);
    }
  });

  it("carries the plugin label, sanitized icon, color and binding through to the entry", () => {
    const bound = child(submenu(buildTabMenu(input("sidebar")), "plugins"), "plugin-ring-receiver");
    expect(bound?.kind).toBe("check");
    if (bound?.kind !== "check") return;
    expect(bound.label).toBe(PLUGIN_ITEM.label);
    expect(bound.icon).toEqual({ svg: PLUGIN_ITEM.iconSvg, color: PLUGIN_ITEM.iconColor });
    expect(bound.checked).toBe(true);
    expect(bound.action.type === "runPlugin" ? bound.action.entry.pluginId : null).toBe(PLUGIN_ITEM.pluginId);

    const elsewhere = child(submenu(buildTabMenu(input("header", { pluginTabMenu: plugins("tab-other") })), "plugins"), "plugin-ring-receiver");
    expect(elsewhere?.kind === "check" ? elsewhere.checked : null).toBe(false);
  });

  it("keeps Attach inert with a hint and offers no Detach while the bridge is off", () => {
    const off: TelegramThreadMenuState = { bridgeActive: false, topics: "unknown", bound: false, link: null };
    const pluginsEntry = submenu(buildTabMenu(input("header", { telegramThread: off })), "plugins");
    const open = child(pluginsEntry, "telegram-open");
    expect(open?.kind === "item" ? open.unavailable : null).toBe(true);
    expect(open?.kind === "item" ? (open.hint ?? "").length > 0 : false).toBe(true);
    expect(child(pluginsEntry, "telegram-close")).toBeUndefined();
  });

  it("offers an operable Attach before the Telegram state is known", () => {
    const open = child(submenu(buildTabMenu(input("sidebar", { telegramThread: null })), "plugins"), "telegram-open");
    expect(open?.kind === "item" ? open.unavailable : null).toBe(false);
    expect(open?.kind === "item" ? open.hint : "present").toBeUndefined();
  });

  it("omits the Plugins entry when it would hold nothing", () => {
    for (const surface of SURFACES) {
      const entries = buildTabMenu(input(surface, { tab: tab({ remote_peer_id: "peer-1" }) }));
      expect(submenu(entries, "plugins")).toBeUndefined();
    }
  });

  it("disables both session actions while the header reconnects", () => {
    const entries = buildTabMenu(input("header", { busy: { reconnecting: true, restarting: false } }));
    const disabled = entries.flatMap((entry) => (entry.kind === "item" && (entry.id === "reconnect" || entry.id === "restart") ? [entry.disabled] : []));
    expect(disabled).toEqual([true, true]);
  });

  it("offers neither rename nor close on a protected tab", () => {
    for (const surface of SURFACES) {
      const ids = buildTabMenu(input(surface, { tab: tab({ protected: true }) })).map((entry) => entry.id);
      expect(ids.includes("rename") || ids.includes("close")).toBe(false);
    }
  });

  it("marks the current model and provider and leaves the current provider without an action", () => {
    const entries = buildTabMenu(input("sidebar"));
    const model = child(submenu(entries, "model"), "model-model-a");
    expect(model?.kind === "item" ? model.selected : null).toBe(true);
    const current = child(submenu(entries, "provider"), `provider-${CURRENT.id}`);
    const other = child(submenu(entries, "provider"), `provider-${OTHER.id}`);
    expect(current?.kind === "item" ? current.action : "missing").toBeNull();
    expect(other?.kind === "item" ? other.action?.type : null).toBe("switchProvider");
  });
});

finish();
