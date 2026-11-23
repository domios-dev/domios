import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { DEFAULT_APPEARANCE, type AppearanceLook, type AppearanceProposal } from "../../lib/appearance";
import { DEFAULT_THEME } from "../../lib/themeCss";
import type { AppConfig, MeshPeersResponse } from "../../lib/types";
import { UNKNOWN_MACHINE } from "../../lib/machineIdentity";
import { rememberAccountMachineNames, rememberMachineIdentities } from "../../lib/useMeshMachineIdentities";
import { machine, pair, snapshot } from "../MeshPanel/accountMachines.testdata";
import { BackPill } from "../shared/BackPill";
import { renderAppearanceCard } from "./AppearanceCard";
import { AppearanceLab } from "./AppearanceLab";

const api = vi.hoisted(() => ({ readAppearance: vi.fn(), proposeAppearance: vi.fn(), changeAppearance: vi.fn(), readAppearanceProposal: vi.fn() }));
vi.mock("../../lib/appearanceClient", () => ({
  ...api,
  APPEARANCE_CHANGED: "test-appearance-change",
  appearanceError: String,
  checkAppearanceContrast: vi.fn(async () => []),
  listenAppearanceCommit: vi.fn(async () => () => {}),
}));
vi.mock("../../store/multiplexer/hooks", () => ({ useMuxSelector: () => undefined }));

const paired: MeshPeersResponse["paired"] = [{
  instance_id: "peer", instance_name: "Desk [dev-peer]", online: true, addresses: [],
  api_port: 7780, public_key: "", paired_at: "", last_seen: null, is_primary: false,
  access_level: "full", web_ui: null, relay_protocol_version: 1, last_active_ms: null,
}];
const rememberPeers = (peers: MeshPeersResponse["paired"]) => rememberMachineIdentities({
  paired: peers, local_instance_name: peers.length ? "Desk [dev-local]" : "",
  installations: [], local_machine_identity: UNKNOWN_MACHINE, machine_identities: {},
  local_last_active_ms: null, local_dev: true,
});
const rememberNames = () => rememberAccountMachineNames(snapshot({
  local_installation_id: "local-installation",
  registry_names: [
    machine("local-installation", "Desk [dev-local]", { admitted_at: 10 }),
    machine("peer-installation", "Desk [dev-peer]", { admitted_at: 20 }),
  ],
  pairs: [pair("peer", "Desk", { installation_id: "peer-installation" })],
}));

it("documents each Lab and wallpaper control and stages one complete proposal", async () => {
  const tuning = { roundness: 1, shadow_strength: 1, font_size: 0, edges: "subtle" };
  const look = { theme: DEFAULT_THEME, wallpaper: null, tuning };
  api.readAppearance.mockResolvedValue({ machine: { id: "fixture", name: "Desk [dev-local]" }, look, presets: [DEFAULT_THEME], active: null });
  api.proposeAppearance.mockImplementation(async (_target, proposed) => ({ id: "proposal", look: proposed }));
  api.changeAppearance.mockResolvedValue(undefined);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const select = (label: string) => [...host.querySelectorAll<HTMLInputElement>('input[type="radio"]')]
    .find(control => control.getAttribute("aria-label") === label || [...(control.labels ?? [])].some(candidate => candidate.textContent?.trim() === label))!;
  const fields = new Set<string>();
  const collect = () => host.querySelectorAll<HTMLElement>("[data-appearance-field]").forEach(node => fields.add(node.dataset.appearanceField!));
  try {
    rememberNames();
    rememberPeers(paired);
    const back = vi.fn();
    await act(async () => root.render(<AppearanceLab config={{ theme: DEFAULT_THEME } as AppConfig}
      headingId="settings-title" back={<BackPill label="General" onBack={back} />} onNavigate={() => {}} />));
    const header = host.querySelector("header")!;
    expect(header.querySelector("#settings-title")?.textContent).toBe("Appearance Lab");
    await act(async () => header.querySelector<HTMLButtonElement>('button[aria-label="Back to General"]')!.click());
    expect(back).toHaveBeenCalledOnce();
    expect([...header.querySelectorAll("button")].map(button => button.textContent?.trim())).toEqual(["Back", "Ask General"]);
    expect([...host.querySelectorAll('select[aria-label="Appearance machine"] option')].map(option => option.textContent))
      .toEqual(["This machine · Desk #1", "Desk #2"]);
    expect(host.querySelector('[role="group"][aria-label="Preview appearance"]')).toBeNull();
    expect(host.querySelector('[role="group"][aria-label="Interface text"]')).not.toBeNull();
    collect();
    await act(async () => select("Your own wallpaper").click());
    collect();
    await act(async () => select("Botanical").click());
    collect();
    const defined = [...host.querySelectorAll("button")].find(button => button.textContent === "Defined")!;
    await act(async () => defined.click());
    const apply = [...host.querySelectorAll("button")].find(button => button.textContent === "Apply")!;
    await act(async () => apply.click());

    const guide = readFileSync("prod-docs/APPEARANCE.md", "utf8");
    expect(fields.size).toBeGreaterThan(30);
    for (const field of fields) expect(guide, field).toContain(`\`${field}\``);
    expect(api.proposeAppearance).toHaveBeenCalledWith({ machine: "local" }, {
      theme: DEFAULT_THEME, wallpaper: { background: { pattern: "botanical" } },
      tuning: { ...tuning, edges: "defined" },
    }, look);
    expect(api.changeAppearance).toHaveBeenCalledWith({ machine: "local" }, "proposal", "apply");
  } finally {
    await act(async () => root.unmount());
    rememberAccountMachineNames(null);
    rememberPeers([]);
    host.remove();
  }
});

function mountLab() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const region = () => host.querySelector('section[aria-label="Changes"]')!;
  const button = (label: string) => [...region().querySelectorAll("button")]
    .find(control => (control.getAttribute("aria-label") ?? control.textContent?.trim()) === label);
  const status = () => region().querySelector('[role="status"][aria-live="polite"]')!.textContent;
  const changeRange = async (field: string, value: string) => {
    const input = host.querySelector<HTMLInputElement>(`input[data-appearance-field="${field}"]`)!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")!.set!.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    return input;
  };
  return {
    host, region, button, status, changeRange,
    render: () => act(async () => root.render(<AppearanceLab config={{ theme: DEFAULT_THEME } as AppConfig}
      back={<BackPill onBack={() => {}} />} onNavigate={() => {}} />)),
    cleanup: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

it("counts staged changes, reverts an individual control, and discards the remaining controls and wallpaper", async () => {
  const look: AppearanceLook = {
    theme: DEFAULT_THEME, wallpaper: { background: { pattern: "dots" } }, tuning: null,
  };
  api.readAppearance.mockResolvedValue({ machine: { id: "fixture", name: "Desk" }, look, presets: [DEFAULT_THEME], active: null });
  const lab = mountLab();
  const wallpaper = (label: string) => [...lab.host.querySelectorAll<HTMLInputElement>('input[type="radio"]')]
    .find(input => [...(input.labels ?? [])].some(candidate => candidate.textContent?.trim() === label))!;
  try {
    await lab.render();
    expect(lab.status()).toContain(`No changes · showing Desk’s current look, ${DEFAULT_THEME.name}`);
    expect(lab.button("Apply")?.disabled).toBe(true);
    expect([...lab.host.querySelectorAll("button")].some(button => button.textContent?.trim() === "Reset")).toBe(false);
    expect(wallpaper("Dots").checked).toBe(true);

    const roundness = await lab.changeRange("appearance.roundness", "0.4");
    const shadow = await lab.changeRange("appearance.shadow_strength", "0.6");
    await act(async () => wallpaper("Botanical").click());
    expect(lab.status()).toBe("3 changes");
    expect(lab.button("Apply")?.disabled).toBe(false);
    expect(lab.region().textContent).toContain("Roundness 1.00 → 0.40");
    await act(async () => lab.button("Revert Roundness")!.click());
    expect(lab.status()).toBe("2 changes");
    expect(roundness.valueAsNumber).toBe(1);
    expect(shadow.valueAsNumber).toBe(0.6);
    expect(wallpaper("Botanical").checked).toBe(true);

    await act(async () => lab.button("Discard changes")!.click());
    expect(lab.status()).toContain("No changes");
    expect(shadow.valueAsNumber).toBe(1);
    expect(wallpaper("Dots").checked).toBe(true);
    expect(wallpaper("Botanical").checked).toBe(false);
    expect(lab.button("Apply")?.disabled).toBe(true);
  } finally {
    await lab.cleanup();
  }
});

it("shows the applied receipt with named Undo and keeps Undo out of staged edits", async () => {
  const machine = { id: "fixture", name: "Desk" };
  const look: AppearanceLook = { theme: DEFAULT_THEME, wallpaper: null, tuning: null };
  const nextTheme = { ...DEFAULT_THEME, name: "Evening" };
  const next = { ...look, theme: nextTheme };
  const receipt: AppearanceProposal = {
    id: "fixture-proposal", machine, rationale: "", look: next, previous: look,
    state: "applied", can_undo: true, applied_at: 1735689600, contrast: [],
  };
  api.readAppearance.mockResolvedValue({ machine, look, presets: [DEFAULT_THEME, nextTheme], active: null });
  api.proposeAppearance.mockResolvedValue({ ...receipt, state: "proposed", can_undo: false });
  api.changeAppearance.mockImplementation(async (_target, _id, action) => {
    const result = action === "undo" ? { ...receipt, state: "undone", can_undo: false } : receipt;
    window.dispatchEvent(new CustomEvent("test-appearance-change", { detail: { receipt: result } }));
    return result;
  });
  const lab = mountLab();
  const undo = `Undo · back to ${DEFAULT_THEME.name}`;
  try {
    await lab.render();
    expect(lab.button(undo)).toBeUndefined();
    await act(async () => [...lab.host.querySelectorAll("button")].find(button => button.textContent === nextTheme.name)!.click());
    await act(async () => lab.button("Apply")!.click());
    expect(api.changeAppearance).toHaveBeenLastCalledWith({ machine: "local" }, receipt.id, "apply");
    expect(lab.status()).toMatch(/^Applied · \d{2}:\d{2}$/);
    expect(lab.button(undo)?.disabled).toBe(false);

    await lab.changeRange("appearance.roundness", "0.4");
    expect(lab.button(undo)).toBeUndefined();
    await act(async () => lab.button("Discard changes")!.click());
    expect(lab.button(undo)?.disabled).toBe(false);
    await act(async () => lab.button(undo)!.click());
    expect(api.changeAppearance).toHaveBeenLastCalledWith({ machine: "local" }, receipt.id, "undo");
    expect(lab.status()).toContain(`current look, ${DEFAULT_THEME.name}`);
    expect(lab.button(undo)).toBeUndefined();
    expect(lab.button("Apply")?.disabled).toBe(true);
  } finally {
    await lab.cleanup();
  }
});

it("offers the stored Undo on the selected remote machine and targets Apply there after staging", async () => {
  const remote = { id: "peer", name: "Desk [dev-peer]" };
  const look: AppearanceLook = { theme: DEFAULT_THEME, wallpaper: null, tuning: DEFAULT_APPEARANCE };
  const previous = { ...look, theme: { ...DEFAULT_THEME, name: "Earlier look" } };
  const active: AppearanceProposal = {
    id: "remote-receipt", machine: remote, look, previous, state: "applied",
    can_undo: true, applied_at: 1735689600, rationale: "", contrast: [],
  };
  api.readAppearance.mockImplementation(async target => ({
    machine: target.peer ? remote : { id: "local", name: "Desk [dev-local]" },
    look, presets: [DEFAULT_THEME], active: target.peer ? active : null,
  }));
  const lab = mountLab();
  try {
    rememberNames();
    rememberPeers(paired);
    await lab.render();
    const select = lab.host.querySelector<HTMLSelectElement>('select[aria-label="Appearance machine"]')!;
    await act(async () => {
      select.value = remote.id;
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(lab.button("Undo · back to Earlier look")?.disabled).toBe(false);
    await lab.changeRange("appearance.roundness", "0.4");
    expect(lab.button("Apply to Desk #2")?.disabled).toBe(false);
    expect(lab.button("Undo · back to Earlier look")).toBeUndefined();
  } finally {
    await lab.cleanup();
    rememberAccountMachineNames(null);
    rememberPeers([]);
  }
});

it("offers the owner the theme wallpaper when the proposal keeps a machine override", async () => {
  const look = {
    theme: { ...DEFAULT_THEME, name: "Fixture SVG", chat_background: { pattern: "svg", svg: '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><path d="M0 0L8 8"/></svg>' } },
    wallpaper: { background: { pattern: "dots" } }, tuning: null,
  };
  const proposal = {
    id: "proposal", machine: { id: "fixture", name: "Fixture" }, rationale: "Quiet ink lines",
    look, previous: look, state: "proposed", contrast: [], can_undo: false, applied_at: null,
  };
  api.readAppearanceProposal.mockResolvedValue(proposal);
  api.changeAppearance.mockResolvedValue({ ...proposal, look: { ...look, wallpaper: null }, state: "applied", can_undo: true });
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  try {
    await act(async () => root.render(renderAppearanceCard("proposal", "fixture")));
    expect(node.textContent).toContain("the machine’s wallpaper (Dots) stays on top");
    expect(node.textContent).toContain("Theme wallpaper: SVG");
    const useTheme = [...node.querySelectorAll("button")].find(button => button.textContent === "Use theme wallpaper");
    expect(useTheme).toBeDefined();
    await act(async () => useTheme!.click());
    expect(api.changeAppearance).toHaveBeenLastCalledWith({ machine: "fixture" }, "proposal", "use-theme-wallpaper");
    expect(node.querySelector('[role="group"]')?.getAttribute("aria-label")).toBe("Appearance applied: Fixture SVG");
    expect(node.querySelector('footer [role="status"]')?.textContent).toBe("✓ Applied");
    expect([...node.querySelectorAll("button")].some(button => button.textContent === "Undo" && !button.disabled)).toBe(true);
  } finally {
    await act(async () => root.unmount());
    node.remove();
  }
});

it.each([
  { id: "fixture", action: "Apply", status: "Proposal", peer: undefined },
  { id: "peer", action: "Apply to Desk #2", status: "For Desk #2", peer: "peer" },
])("uses the titlebar display name only for a remote card ($id)", async ({ id, action, status, peer }) => {
  const look = { theme: DEFAULT_THEME, wallpaper: null, tuning: null };
  const proposal = {
    id: "proposal", machine: { id, name: peer ? "Desk [dev-peer]" : "Desk [dev-local]" },
    rationale: "A quieter palette", look, previous: look, state: "proposed",
    contrast: [], can_undo: false, applied_at: null,
  };
  api.readAppearanceProposal.mockResolvedValue(proposal);
  api.changeAppearance.mockResolvedValueOnce({ ...proposal, state: "applied", can_undo: true })
    .mockResolvedValueOnce({ ...proposal, state: "undone" });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const button = (name: string) => [...host.querySelectorAll("button")].find(control => control.textContent?.trim() === name)!;
  const footerStatus = () => host.querySelector('footer [role="status"]')?.textContent;
  try {
    rememberNames();
    rememberPeers(paired);
    await act(async () => root.render(renderAppearanceCard("proposal", id)));
    expect(footerStatus()).toBe(status);
    expect(host.textContent).not.toContain("[dev-");
    expect(button("Open in Lab")).toBeDefined();
    expect(host.querySelector('[role="img"][aria-label^="Accent #"]')).not.toBeNull();
    await act(async () => button(action).click());
    expect(api.changeAppearance).toHaveBeenLastCalledWith({ machine: id, peer }, "proposal", "apply");
    expect(footerStatus()).toBe(peer ? "✓ Applied to Desk #2" : "✓ Applied");
    await act(async () => button("Undo").click());
    expect(api.changeAppearance).toHaveBeenLastCalledWith({ machine: id, peer }, "proposal", "undo");
    expect(footerStatus()).toBe(`↶ Undone · back to ${DEFAULT_THEME.name}`);
    expect(button("Apply again")).toBeDefined();
  } finally {
    await act(async () => root.unmount());
    rememberAccountMachineNames(null);
    rememberPeers([]);
    host.remove();
  }
});

it("shows an applied tweak with its changed value and Undo", async () => {
  const previous = { theme: DEFAULT_THEME, wallpaper: { background: { pattern: "dots", opacity: 0.08 } }, tuning: null };
  const look = { ...previous, wallpaper: { background: { pattern: "dots", opacity: 0.05 } } };
  const receipt = {
    id: "tweak", machine: { id: "fixture", name: "Fixture" }, rationale: "", tweak: true,
    look, previous, state: "applied", contrast: [], can_undo: true, applied_at: null,
  };
  api.readAppearanceProposal.mockResolvedValue(receipt);
  api.changeAppearance.mockResolvedValueOnce({ ...receipt, state: "undone", can_undo: false });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const buttonNames = () => [...host.querySelectorAll("button")].map(button => button.textContent?.trim());
  const status = () => host.querySelector('[role="status"]')?.textContent;
  try {
    await act(async () => root.render(renderAppearanceCard("tweak", "fixture")));
    expect(host.textContent).toContain("Wallpaper 8% → 5%");
    expect(status()).toBe("✓ Applied");
    expect(buttonNames()).toEqual(["Undo", "Open in Lab"]);
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(api.changeAppearance).toHaveBeenLastCalledWith({ machine: "fixture" }, "tweak", "undo");
    expect(status()).toBe("↶ Undone");
    expect(buttonNames()).toEqual(["Open in Lab"]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
