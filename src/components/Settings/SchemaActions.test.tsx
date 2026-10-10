import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
