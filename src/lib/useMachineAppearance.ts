import { createContext, useContext, useEffect, useMemo, useState } from "react";
import type { AppConfig } from "./types";
import { daemonTransportForPeer } from "./daemonBootstrap";
import { readMachineAppearance, readWallpaperAsset, type MachineAppearance } from "./chatWallpaperClient";
import { DEFAULT_THEME } from "./themeCss";
import { relayTarget } from "./webApi";
import { discardWorkspaceSwitchOrigin, useWorkspaceBrowseTarget } from "./workspaceBrowseTarget";
import { useMeshMachineIdentity } from "./useMeshMachineIdentities";
import type { MachineIdentity } from "./machineIdentity";

export interface SelectedMachineAppearance {
  appearance: MachineAppearance;
  peerId?: string;
  identity?: MachineIdentity;
  ready: boolean;
  projectWallpaper: (wallpaper: MachineAppearance["wallpaper"]) => void;
}

export const MachineAppearanceContext = createContext<(SelectedMachineAppearance & { viewerTheme?: MachineAppearance["theme"] }) | null>(null);

export function useMachineViewerTheme() {
  const selected = useContext(MachineAppearanceContext);
  return selected?.viewerTheme ?? selected?.appearance.theme ?? DEFAULT_THEME;
}

type Target = ReturnType<typeof useWorkspaceBrowseTarget>;
type PeerAppearance = { target: NonNullable<Target>; appearance: MachineAppearance };

export function useMachineAppearance(config: AppConfig, ready: boolean): SelectedMachineAppearance {
  const target = useWorkspaceBrowseTarget();
  const relay = relayTarget()?.id;
  const peerId = target?.id ?? relay;
  const identity = useMeshMachineIdentity(peerId);
  const [peer, setPeer] = useState<PeerAppearance | null>(null);

  useEffect(() => {
    if (!target || target.id === relay) return;
    const controller = new AbortController();
    let changed = false;
    const publish = (appearance: MachineAppearance) => {
      if (!controller.signal.aborted) setPeer({ target, appearance });
    };
    const unsubscribe = daemonTransportForPeer(target.id).subscribe("/api/events/stream", (name, data) => {
      if (name !== "config-changed" || !data || typeof data !== "object" || !("theme" in data)) return;
      const event = data as AppConfig;
      changed = true;
      publish({ theme: event.theme, wallpaper: event.chat_wallpaper ?? null, tuning: event.appearance ?? null });
    }, { signal: controller.signal, onEnd: () => undefined });
    void readMachineAppearance(target.id, controller.signal).then(appearance => {
      if (!changed) publish(appearance);
    }).catch(() => { if (!controller.signal.aborted && !changed) discardWorkspaceSwitchOrigin(target); });
    return () => { controller.abort(); unsubscribe(); };
  }, [target, relay]);

  const remote = target && target.id !== relay;
  const selected = remote && peer?.target === target ? peer.appearance : null;
  return {
    appearance: selected ?? { theme: config.theme, wallpaper: config.chat_wallpaper ?? null, tuning: config.appearance ?? null },
    peerId,
    identity,
    ready: remote ? selected !== null : ready,
    projectWallpaper(wallpaper) {
      if (remote) setPeer(previous => previous?.target === target
        ? { target, appearance: { ...previous.appearance, wallpaper } }
        : previous);
    },
  };
}

export function useWallpaperAsset(peerId: string | undefined, id: string | undefined) {
  return useWallpaperAssetResult(peerId, id).asset;
}

export function useWallpaperAssetResult(peerId: string | undefined, id: string | undefined) {
  const request = useMemo(() => ({ id, peerId }), [id, peerId]);
  const [result, setResult] = useState<{ request: typeof request; asset?: WallpaperAsset }>();
  useEffect(() => {
    const { id, peerId } = request;
    if (!id) return;
    const controller = new AbortController();
    let release: (() => void) | undefined;
    void readWallpaperAsset(peerId, id, controller.signal).then(blob => {
      if (controller.signal.aborted) return;
      const url = URL.createObjectURL(blob);
      let users = 1;
      release = () => { if (--users === 0) URL.revokeObjectURL(url); };
      const drop = release;
      setResult({ request, asset: { url, size: blob.size, retain() { users++; return drop; } } });
    }).catch(() => { if (!controller.signal.aborted) setResult({ request }); });
    return () => {
      controller.abort();
      release?.();
    };
  }, [request]);
  const current = result?.request === request ? result : undefined;
  return { asset: current?.asset, ready: !id || current !== undefined };
}

export interface WallpaperAsset {
  url: string;
  size: number;
  retain: () => () => void;
}
