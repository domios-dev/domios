import { expect, it, vi } from "vitest";
import { copyMachineAppearance, undoMachineAppearance } from "./copyMachineAppearance";
import { DEFAULT_THEME } from "./themeCss";
import type { MachineAppearance } from "./chatWallpaperClient";

const client = vi.hoisted(() => ({
  readMachineAppearance: vi.fn(), saveMachineAppearance: vi.fn(), readWallpaperAsset: vi.fn(), copyWallpaperAsset: vi.fn(),
}));
vi.mock("./chatWallpaperClient", () => client);

it("copies the exact look once, skips offline peers, and fences undo against edits", async () => {
  const source = { peerId: "source", name: "Source", available: true };
  const online = { peerId: "online", name: "Online", available: true };
  const offline = { peerId: "offline", name: "Offline", available: false };
  const id = "a".repeat(64) + ".svg";
  const look: MachineAppearance = { theme: DEFAULT_THEME, wallpaper: { background: { pattern: "svg" }, asset: id } };
  const prior: MachineAppearance = { theme: { ...DEFAULT_THEME, foreground: "#abcdef" }, wallpaper: null };
  const asset = new Blob(["fixture"], { type: "image/svg+xml" });
  client.readMachineAppearance.mockResolvedValueOnce(look).mockResolvedValueOnce(prior);
  client.readWallpaperAsset.mockResolvedValue(asset);
  const receipt = await copyMachineAppearance(source, [online, offline]);
  expect(client.readMachineAppearance.mock.calls).toEqual([["source"], ["online"]]);
  expect(client.copyWallpaperAsset).toHaveBeenCalledExactlyOnceWith("online", id, asset);
  expect(client.saveMachineAppearance).toHaveBeenCalledExactlyOnceWith("online", look, prior);
  expect(receipt.failed).toEqual([{ name: "Offline", reason: "not applied, offline" }]);
  client.saveMachineAppearance.mockRejectedValueOnce(new Error("Machine appearance changed"));
  expect(await undoMachineAppearance(receipt)).toEqual([{ name: "Online", reason: "Machine appearance changed" }]);
  expect(client.saveMachineAppearance).toHaveBeenLastCalledWith("online", prior, look);
});
