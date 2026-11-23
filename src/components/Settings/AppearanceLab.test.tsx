import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { DEFAULT_THEME } from "../../lib/themeCss";
import type { AppConfig, MeshPeersResponse } from "../../lib/types";
import { UNKNOWN_MACHINE } from "../../lib/machineIdentity";
import { rememberAccountMachineNames, rememberMachineIdentities } from "../../lib/useMeshMachineIdentities";
import { machine, pair, snapshot } from "../MeshPanel/accountMachines.testdata";
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
  api.proposeAppearance.mockResolvedValue({ id: "proposal" });
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
    await act(async () => root.render(<AppearanceLab config={{ theme: DEFAULT_THEME } as AppConfig} onNavigate={() => {}} />));
    expect([...host.querySelectorAll('select[aria-label="Appearance machine"] option')].map(option => option.textContent))
      .toEqual(["For Desk #1", "For Desk #2"]);
    expect(host.querySelector('[role="group"][aria-label="Preview appearance"]')).toBeNull();
    expect(host.querySelector('[role="group"][aria-label="Interface text"]')).not.toBeNull();
    collect();
    await act(async () => select("Your own wallpaper").click());
    collect();
    await act(async () => select("Botanical").click());
    collect();
    const defined = [...host.querySelectorAll("button")].find(button => button.textContent === "Defined")!;
    await act(async () => defined.click());
    const apply = [...host.querySelectorAll("button")].find(button => button.textContent === "Apply to Desk #1")!;
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
