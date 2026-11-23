import { createContext, useEffect, useState } from "react";
import type { AppConfig } from "./types";
import { daemonTransportForPeer } from "./daemonBootstrap";
import { readMachineAppearance, readWallpaperAsset, type MachineAppearance } from "./chatWallpaperClient";
import { relayTarget } from "./webApi";
import { useWorkspaceBrowseTarget } from "./workspaceBrowseTarget";
import { useMeshMachineIdentity } from "./useMeshMachineIdentities";
import type { MachineIdentity } from "./machineIdentity";

export interface SelectedMachineAppearance {
  appearance: MachineAppearance;
  peerId?: string;
  identity?: MachineIdentity;
  ready: boolean;
  projectWallpaper: (wallpaper: MachineAppearance["wallpaper"]) => void;
}

export const MachineAppearanceContext = createContext<SelectedMachineAppearance | null>(null);

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
      publish({ theme: event.theme, wallpaper: event.chat_wallpaper ?? null });
    }, { signal: controller.signal, onEnd: () => undefined });
    void readMachineAppearance(target.id, controller.signal).then(appearance => {
      if (!changed) publish(appearance);
    }).catch(() => undefined);
    return () => { controller.abort(); unsubscribe(); };
  }, [target, relay]);

  const remote = target && target.id !== relay;
  const selected = remote && peer?.target === target ? peer.appearance : null;
  return {
    appearance: selected ?? { theme: config.theme, wallpaper: config.chat_wallpaper ?? null },
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
  const [asset, setAsset] = useState<(WallpaperAsset & { id: string; peerId?: string })>();
  useEffect(() => {
    if (!id) return;
    const controller = new AbortController();
    let release: (() => void) | undefined;
    void readWallpaperAsset(peerId, id, controller.signal).then(blob => {
      if (controller.signal.aborted) return;
      const url = URL.createObjectURL(blob);
      let users = 1;
      release = () => { if (--users === 0) URL.revokeObjectURL(url); };
      const drop = release;
      setAsset({ id, peerId, url, size: blob.size, retain() { users++; return drop; } });
    }).catch(() => undefined);
    return () => {
      controller.abort();
      release?.();
    };
  }, [peerId, id]);
  return asset?.id === id && asset?.peerId === peerId ? asset : undefined;
}

export interface WallpaperAsset {
  url: string;
  size: number;
  retain: () => () => void;
}
