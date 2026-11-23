import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { DEFAULT_THEME } from "../../lib/themeCss";
import type { AppConfig } from "../../lib/types";
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

it("documents each Lab and wallpaper control and stages one complete proposal", async () => {
  const tuning = { roundness: 1, shadow_strength: 1, font_size: 0, edges: "subtle" };
  const look = { theme: DEFAULT_THEME, wallpaper: null, tuning };
  api.readAppearance.mockResolvedValue({ machine: { id: "fixture", name: "Fixture" }, look, presets: [DEFAULT_THEME], active: null });
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
    await act(async () => root.render(<AppearanceLab config={{ theme: DEFAULT_THEME } as AppConfig} onNavigate={() => {}} />));
    collect();
    await act(async () => select("Your own wallpaper").click());
    collect();
    await act(async () => select("Botanical").click());
    collect();
    const defined = [...host.querySelectorAll("button")].find(button => button.textContent === "Defined")!;
    await act(async () => defined.click());
    const apply = [...host.querySelectorAll("button")].find(button => button.textContent === "Apply to Fixture")!;
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
    expect(node.textContent).toContain("The machine wallpaper hides this theme’s wallpaper.");
    const useTheme = [...node.querySelectorAll("button")].find(button => button.textContent === "Use theme wallpaper");
    expect(useTheme).toBeDefined();
    await act(async () => useTheme!.click());
    expect(api.changeAppearance).toHaveBeenLastCalledWith({ machine: "fixture" }, "proposal", "use-theme-wallpaper");
    expect(node.querySelector('[role="group"]')?.getAttribute("aria-label")).toBe("Appearance applied: Fixture SVG");
    expect([...node.querySelectorAll("button")].some(button => button.textContent === "Undo" && !button.disabled)).toBe(true);
  } finally {
    await act(async () => root.unmount());
    node.remove();
  }
});
