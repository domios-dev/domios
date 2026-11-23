import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { useMachineAppearance } from "./useMachineAppearance";
import { DEFAULT_THEME } from "./themeCss";
import { setWorkspaceBrowseTarget } from "./workspaceBrowseTarget";
import type { AppConfig } from "./types";
import type { MachineAppearance } from "./chatWallpaperClient";

const { read } = vi.hoisted(() => ({ read: vi.fn() }));
vi.mock("./chatWallpaperClient", () => ({ readMachineAppearance: read, readWallpaperAsset: vi.fn() }));
vi.mock("./daemonBootstrap", () => ({ daemonTransportForPeer: () => ({ subscribe: () => () => {} }) }));

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
