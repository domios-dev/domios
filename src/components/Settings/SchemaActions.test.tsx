import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createCommandRegistry, dispatchCommand } from "../../lib/transportCommands";
import type { TransportClient } from "../../lib/transportClient";
import type { AppConfig, SettingField } from "../../lib/types";

const fixture = vi.hoisted(() => ({ invoke: vi.fn(), integration: { integration_state: "absent" } as unknown }));
vi.mock("../../lib/bridge", () => ({ invoke: fixture.invoke }));
import { actionButtonVariant, actionStateFromQuery, segmentActionRuns, type ActionSetting } from "./SchemaActions";
import { SchemaRenderer } from "./SchemaRenderer";

const action = (id: string, style: ActionSetting["style"], extra: Partial<ActionSetting> = {}): ActionSetting =>
  ({ kind: "action", id, label: `label-${id}`, command: `command-${id}`, style, ...extra });
const install = action("install", "primary", { state_query: "query-install" });
const uninstall = action("uninstall", "secondary");
const restore = action("restore", "danger");
const pathLink: SettingField = { kind: "path_link", label: "Target", path: "/opaque/target", open: "open_folder" };
const badge: SettingField = { kind: "badge", query_command: "query-badge", variants: [{ id: "installed", label: "opaque badge", style: "green" }] };

describe("schema action model", () => {
  it("groups consecutive actions into one run and keeps other fields in place", () => {
    const segments = segmentActionRuns([pathLink, install, uninstall, restore, badge, uninstall]);
    expect(segments.map((segment) => segment.kind === "actions" ? segment.actions.map((a) => a.id) : segment.field.kind))
      .toEqual(["path_link", ["install", "uninstall", "restore"], "badge", ["uninstall"]]);
  });
  it("maps declared styles onto one primary, quiet secondaries and a subtle danger", () => {
    expect(actionButtonVariant("primary")).toBe("primary");
    expect(actionButtonVariant("secondary")).toBe("ghost");
    expect(actionButtonVariant("danger")).toBe("danger");
  });
  it("an installed state demotes the install action to a secondary reinstall", () => {
    const declared = { label: "opaque", style: "primary" as const };
    expect(actionStateFromQuery({ integration_state: "installed" }, declared)).toEqual({ label: "Reinstall", style: "secondary" });
    expect(actionStateFromQuery({ integration_state: "absent" }, declared)).toEqual(declared);
    expect(actionStateFromQuery({ label: "opaque state" }, { label: "x", style: "danger" })).toEqual({ label: "opaque state", style: "danger" });
  });
});

let host: HTMLDivElement;
let root: Root;
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
beforeEach(() => {
  fixture.integration = { integration_state: "absent" };
  fixture.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === "query-install") return fixture.integration;
    if (command === "query-badge") return { installed: true };
    return null;
  });
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); });

const section: SettingField = { kind: "section", title: "Opaque section", description: "opaque description", fields: [badge, pathLink, install, uninstall, restore] };
const renderSection = (hideActions = false) => act(async () => root.render(
  <SchemaRenderer schema={[section]} providerId="fixture" config={{ agents: {} } as AppConfig} onChange={() => {}} hideActions={hideActions} />,
));
const actionStyles = () => [...host.querySelectorAll<HTMLButtonElement>('[role="group"][aria-label="Actions"] button')]
  .map((button) => [button.textContent, button.dataset.actionStyle]);

describe("schema section actions", () => {
  it("renders a section's actions in one group with their declared hierarchy", async () => {
    await renderSection();
    expect(host.querySelectorAll('[role="group"][aria-label="Actions"]').length).toBe(1);
    expect(actionStyles()).toEqual([["label-install", "primary"], ["label-uninstall", "secondary"], ["label-restore", "danger"]]);
    expect(host.querySelector('[title="/opaque/target"]')).not.toBeNull();
  });
  it("once installed, no action stays primary", async () => {
    fixture.integration = { integration_state: "installed" };
    await renderSection();
    expect(actionStyles()).toEqual([["Reinstall", "secondary"], ["label-uninstall", "secondary"], ["label-restore", "danger"]]);
  });
  it("dispatches the clicked action once and hides the group when actions are hidden", async () => {
    await renderSection();
    const uninstallButton = [...host.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "label-uninstall");
    await act(async () => uninstallButton!.click());
    expect(fixture.invoke.mock.calls.filter(([command]) => command === "command-uninstall")).toEqual([["command-uninstall", undefined]]);
    await renderSection(true);
    expect(host.querySelector('[role="group"][aria-label="Actions"]')).toBeNull();
  });
});


it("wires provider rule badges and actions to audited daemon routes", async () => {
  const calls: [string, string][] = [];
  const json: TransportClient["json"] = async <T,>(method: string, path: string) => {
    calls.push([method, path]);
    return { installed: false, ok: true } as T;
  };
  const client: TransportClient = { json, localJson: json, currentToken: () => "owner", origin: () => "", upload: vi.fn(), subscribe: () => () => {} };
  const registry = createCommandRegistry(client);
  fixture.invoke.mockImplementation((command: string, args: Record<string, unknown>) => dispatchCommand(registry, command, args));
  const providerArgs = { provider_id: "fixture" };
  await act(async () => root.render(<SchemaRenderer
    schema={[
      { kind: "badge", query_command: "check_provider_rule", query_args: providerArgs, variants: [{ id: "missing", label: "Rule missing", style: "red" }] },
      action("rule", "primary", { label: "Install rule", command: "install_provider_rule", state_query: "check_provider_rule", args: providerArgs }),
      action("backup", "danger", { label: "Restore backup", command: "restore_provider_integration", args: providerArgs }),
    ]}
    providerId="fixture" config={{ agents: {} } as AppConfig} onChange={() => {}}
  />));
  expect(host.textContent).toContain("Rule missing");
  const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find(b => b.textContent === label)!;
  await act(async () => button("Install rule").click());
  await act(async () => button("Restore backup").click());
  expect(calls.filter(([method]) => method === "POST")).toEqual([
    ["POST", "/api/setup/rules/install?provider=fixture"],
    ["POST", "/api/setup/hooks/restore?provider=fixture"],
  ]);
  expect(calls.filter(([method]) => method === "GET")).toEqual(Array(3).fill(["GET", "/api/setup/rules?provider=fixture"]));
});

it("wires a generated integration action's provider_id and consent to the daemon body and refreshed state", async () => {
  const calls: [string, string, unknown][] = [];
  let installed = false;
  const json: TransportClient["json"] = async <T,>(method: string, path: string, body?: unknown) => {
    calls.push([method, path, body]);
    if (method === "POST") installed = true;
    return { integration_state: installed ? "installed" : "absent" } as T;
  };
  const client: TransportClient = { json, localJson: json, currentToken: () => "owner", origin: () => "", upload: vi.fn(), subscribe: () => () => {} };
  const registry = createCommandRegistry(client);
  fixture.invoke.mockImplementation((command: string, args: Record<string, unknown>) => dispatchCommand(registry, command, args));
  const confirmation = vi.spyOn(window, "confirm").mockReturnValue(true);
  try {
    await act(async () => root.render(<SchemaRenderer
      schema={[action("install_integration", "primary", {
        label: "Review and install", command: "install_provider_integration",
        state_query: "check_provider_integration", args: { provider_id: "fixture/opaque", consent: true },
        confirm: "opaque consent",
      })]}
      providerId="fixture/opaque" config={{ agents: {} } as AppConfig} onChange={() => {}}
    />));
    const button = [...host.querySelectorAll<HTMLButtonElement>("button")].find(b => b.textContent === "Review and install")!;
    await act(async () => button.click());
    expect(confirmation).toHaveBeenCalledOnce();
    expect(calls).toEqual([
      ["GET", "/api/setup/hooks?provider=fixture%2Fopaque", undefined],
      ["POST", "/api/setup/hooks/install?provider=fixture%2Fopaque", { consent: true, provider: "fixture/opaque" }],
      ["GET", "/api/setup/hooks?provider=fixture%2Fopaque", undefined],
    ]);
    expect(fixture.invoke.mock.calls.filter(([command]) => command === "install_provider_integration")).toEqual([
      ["install_provider_integration", { provider_id: "fixture/opaque", consent: true }],
    ]);
    expect(host.querySelector('[role="alert"]')).toBeNull();
  } finally {
    confirmation.mockRestore();
  }
});
