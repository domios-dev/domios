import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { AppConfig } from "../../lib/types";
import { DEFAULT_THEME } from "../../lib/themeCss";
import { MachineAppearanceContext } from "../../lib/useMachineAppearance";
import { ChatWallpaperPicker } from "./ChatWallpaperPicker";
import { QuickWallpaper } from "./QuickWallpaper";

const client = vi.hoisted(() => ({ saveWallpaper: vi.fn(), uploadWallpaper: vi.fn() }));
vi.mock("../../lib/chatWallpaperClient", () => ({
  ...client, deleteWallpaperAsset: vi.fn(), askGeneralForWallpaper: vi.fn(),
  WALLPAPER_FILE_TYPES: ["image/png", "image/svg+xml"],
}));

it("shows None when the selected peer has no wallpaper and the local machine has Dots", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const config = { theme: DEFAULT_THEME, chat_wallpaper: { background: { pattern: "dots" } } } as AppConfig;
  try {
    await act(async () => root.render(
      <MachineAppearanceContext.Provider value={{
        peerId: "peer", appearance: { theme: DEFAULT_THEME, wallpaper: null }, ready: true,
        projectWallpaper: vi.fn(),
      }}>
        <QuickWallpaper config={config} onOpenLab={() => {}} />
      </MachineAppearanceContext.Provider>,
    ));
    const choice = host.querySelector('[role="radiogroup"] input:checked');
    expect(choice?.getAttribute("aria-label")).toBe("None");
    expect(host.textContent).toContain("None · plain background");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("persists the selected art and colours, and keeps the current wallpaper on upload refusal", async () => {
  client.saveWallpaper.mockResolvedValue(undefined);
  client.uploadWallpaper.mockRejectedValue(new Error("Choose a valid static SVG."));
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const radio = (name: string) => {
    const input = [...host.querySelectorAll<HTMLInputElement>('input[type="radio"]')]
      .find(control => control.getAttribute("aria-label") === name ||
        [...(control.labels ?? [])].some(label => label.textContent?.trim() === name));
    expect(input).toBeDefined();
    return input!;
  };
  try {
    await act(async () => root.render(<ChatWallpaperPicker config={{ theme: DEFAULT_THEME } as AppConfig} />));
    expect(radio("None").checked).toBe(true);
    await act(async () => radio("Dots").click());
    expect(client.saveWallpaper).toHaveBeenLastCalledWith(undefined, { background: { pattern: "dots" } }, undefined);
    const dotScale = [...host.querySelectorAll<HTMLInputElement>('input[type="range"]')]
      .find(control => [...(control.labels ?? [])].some(label => label.textContent?.startsWith("Scale")));
    expect(dotScale?.min).toBe("8");
    expect(dotScale?.max).toBe("64");
    expect(dotScale?.getAttribute("aria-valuetext")).toBe("20 px");
    await act(async () => radio("Domios").click());
    expect(client.saveWallpaper).toHaveBeenLastCalledWith(undefined, { background: { pattern: "domios" } }, undefined);
    await act(async () => radio("Aurora").click());
    expect(client.saveWallpaper).toHaveBeenLastCalledWith(undefined, {
      palette: "aurora", background: { pattern: "domios" },
    }, undefined);
    const light = [...host.querySelectorAll("button")].find(button => button.textContent === "Light")!;
    await act(async () => light.click());
    expect(light.getAttribute("aria-pressed")).toBe("true");
    expect(client.saveWallpaper).toHaveBeenCalledTimes(3);
    await act(async () => radio("Your own wallpaper").click());
    const input = host.querySelector<HTMLInputElement>('input[type="file"]')!;
    const file = new File(["fixture"], "bad.svg", { type: "image/svg+xml" });
    Object.defineProperty(input, "files", { configurable: true, value: [file] });
    await act(async () => input.dispatchEvent(new Event("change", { bubbles: true })));
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("bad.svg wasn’t used");
    expect(client.saveWallpaper).toHaveBeenCalledTimes(3);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
