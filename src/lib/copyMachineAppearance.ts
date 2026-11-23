import { copyWallpaperAsset, readMachineAppearance, readWallpaperAsset, saveMachineAppearance, type MachineAppearance } from "./chatWallpaperClient";

export interface AppearanceTarget {
  peerId?: string;
  name: string;
  available: boolean;
  unavailableReason?: string;
}

export interface AppearanceCopyReceipt {
  applied: { target: AppearanceTarget; previous: MachineAppearance }[];
  failed: { name: string; reason: string }[];
  copied: MachineAppearance;
}

function reason(error: unknown) {
  return error instanceof Error ? error.message : "Unavailable";
}

export async function copyMachineAppearance(source: AppearanceTarget, targets: AppearanceTarget[]): Promise<AppearanceCopyReceipt> {
  const copied = await readMachineAppearance(source.peerId);
  const receipt: AppearanceCopyReceipt = { copied, applied: [], failed: [] };
  const id = copied.wallpaper?.asset;
  const asset = id ? await readWallpaperAsset(source.peerId, id) : undefined;
  for (const target of targets) {
    if (!target.available) {
      receipt.failed.push({ name: target.name, reason: target.unavailableReason ?? "not applied, offline" });
      continue;
    }
    try {
      const previous = await readMachineAppearance(target.peerId);
      if (id && asset && id !== previous.wallpaper?.asset) {
        await copyWallpaperAsset(target.peerId, id, asset);
      }
      await saveMachineAppearance(target.peerId, copied, previous);
      receipt.applied.push({ target, previous });
    } catch (error) {
      receipt.failed.push({ name: target.name, reason: reason(error) });
    }
  }
  return receipt;
}

export async function undoMachineAppearance(receipt: AppearanceCopyReceipt) {
  const failed: AppearanceCopyReceipt["failed"] = [];
  for (const { target, previous } of receipt.applied) {
    try {
      await saveMachineAppearance(target.peerId, previous, receipt.copied);
    } catch (error) {
      failed.push({ name: target.name, reason: reason(error) });
    }
  }
  return failed;
}
