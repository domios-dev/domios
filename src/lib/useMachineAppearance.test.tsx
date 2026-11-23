import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { useMachineAppearance } from "./useMachineAppearance";
import { DEFAULT_THEME } from "./themeCss";
import { setWorkspaceBrowseTarget } from "./workspaceBrowseTarget";
import type { AppConfig } from "./types";
import type { MachineAppearance } from "./chatWallpaperClient";
import { useAppearanceMotion } from "../hooks/useAppearanceMotion";

const { read, subscribe, animate, listenCommit } = vi.hoisted(() => ({
  read: vi.fn(),
  subscribe: vi.fn((_path: string, _receive: (event: string, value: unknown) => void) => () => {}),
  animate: vi.fn(), listenCommit: vi.fn(async () => () => {}),
}));
vi.mock("./chatWallpaperClient", () => ({ readMachineAppearance: read, readWallpaperAsset: vi.fn() }));
vi.mock("./daemonBootstrap", () => ({ daemonTransportForPeer: () => ({ subscribe }) }));
vi.mock("./appearanceClient", () => ({ listenAppearanceCommit: listenCommit }));
vi.mock("./appearanceMotion", () => ({ animateAppearance: animate, finishAppearanceMotion: vi.fn() }));

it("keeps the newest requested machine when older responses arrive later", async () => {
  const queued = new Map<string, (look: MachineAppearance) => void>();
  read.mockImplementation((id: string) => new Promise(resolve => queued.set(id, resolve)));
  const rootNode = document.createElement("div");
  document.body.append(rootNode);
  const root = createRoot(rootNode);
  const config = { theme: DEFAULT_THEME, chat_wallpaper: { background: { pattern: "dots" } } } as AppConfig;
  function Fixture() {
    const selected = useMachineAppearance(config, true);
    return <output>{selected.ready ? selected.appearance.wallpaper?.background.pattern : "loading"}</output>;
  }
  try {
    await act(async () => root.render(<Fixture />));
    expect(rootNode.textContent).toBe("dots");
    await act(async () => setWorkspaceBrowseTarget({ id: "older", name: "Older" }));
    await act(async () => setWorkspaceBrowseTarget({ id: "newer", name: "Newer" }));
    expect(rootNode.textContent).toBe("loading");
    await act(async () => queued.get("newer")!({ theme: DEFAULT_THEME, wallpaper: { background: { pattern: "space" } } }));
    await act(async () => queued.get("older")!({ theme: DEFAULT_THEME, wallpaper: { background: { pattern: "none" } } }));
    expect(rootNode.textContent).toBe("space");
    await act(async () => setWorkspaceBrowseTarget(null));
    expect(rootNode.textContent).toBe("dots");
  } finally {
    await act(async () => root.unmount());
    setWorkspaceBrowseTarget(null);
    rootNode.remove();
  }
});

it("animates a config-driven look once without an appearance card or motion event", async () => {
  read.mockResolvedValue({ theme: DEFAULT_THEME, wallpaper: null, tuning: null });
  const rootNode = document.createElement("div");
  document.body.append(rootNode);
  const root = createRoot(rootNode);
  const config = { theme: DEFAULT_THEME } as AppConfig;
  const theme = { ...DEFAULT_THEME, name: "External theme", background: "#101a23" };
  function Fixture() {
    const selected = useMachineAppearance(config, true);
    useAppearanceMotion(selected);
    return <output>{selected.appearance.theme.name}</output>;
  }
  try {
    animate.mockClear();
    subscribe.mockClear();
    await act(async () => root.render(<Fixture />));
    await act(async () => setWorkspaceBrowseTarget({ id: "other", name: "Other" }));
    expect(animate).not.toHaveBeenCalled();
    const [, receive] = subscribe.mock.calls[subscribe.mock.calls.length - 1];
    await act(async () => receive("config-changed", { theme }));
    expect(rootNode.textContent).toBe("External theme");
    expect(animate).toHaveBeenCalledExactlyOnceWith({ look: { theme, wallpaper: null, tuning: null }, proposal: null, undo: false });
    await act(async () => receive("config-changed", { theme: { ...theme } }));
    await act(async () => setWorkspaceBrowseTarget(null));
    expect(animate).toHaveBeenCalledTimes(1);
  } finally {
    await act(async () => root.unmount());
    setWorkspaceBrowseTarget(null);
    rootNode.remove();
  }
});
