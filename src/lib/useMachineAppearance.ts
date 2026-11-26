import { createContext, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { AppConfig } from "./types";
import type { AppearanceProposal } from "./appearance";
import { APPEARANCE_CHANGED, type AppearanceTarget } from "./appearanceClient";
import { peerEventRail } from "./peerEventRail";
import { readMachineAppearance, readWallpaperAsset, type MachineAppearance } from "./chatWallpaperClient";
import { DEFAULT_THEME } from "./themeCss";
import { relayTarget } from "./webApi";
import { discardWorkspaceSwitchOrigin, useWorkspaceBrowseTarget } from "./workspaceBrowseTarget";

export const PEER_APPEARANCE_TIMEOUT_MS = 6_000;

export interface SelectedMachineAppearance {
  appearance: MachineAppearance;
  peerId?: string;
  ready: boolean;
  projectWallpaper: (wallpaper: MachineAppearance["wallpaper"]) => void;
}

export const MachineAppearanceContext = createContext<(SelectedMachineAppearance & { viewerTheme?: MachineAppearance["theme"] }) | null>(null);

export function useMachineViewerTheme() {
  const selected = useContext(MachineAppearanceContext);
  return selected?.viewerTheme ?? selected?.appearance.theme ?? DEFAULT_THEME;
}

type Target = ReturnType<typeof useWorkspaceBrowseTarget>;
type PeerAppearance = { targetId: string; appearance: MachineAppearance };

export function useMachineAppearance(config: AppConfig, ready: boolean): SelectedMachineAppearance {
  const target = useWorkspaceBrowseTarget();
  const relay = relayTarget()?.id;
  const peerId = target?.id ?? relay;
  const [peer, setPeer] = useState<PeerAppearance | null>(null);
  const remoteId = target && target.id !== relay ? target.id : null;
  const targetRef = useRef<Target>(target);
  targetRef.current = target;

  useEffect(() => {
    if (!remoteId) return;
    const controller = new AbortController();
    let readController: AbortController | undefined;
    let version = 0;
    let settled = false;
    const publish = (appearance: MachineAppearance) => {
      if (controller.signal.aborted) return;
      settled = true;
      setPeer({ targetId: remoteId, appearance });
    };
    const refresh = () => {
      readController?.abort();
      const request = new AbortController();
      readController = request;
      const requestVersion = ++version;
      let timedOut = false;
      const timeout = setTimeout(() => {
        timedOut = true;
        request.abort(new DOMException("Peer appearance timed out", "TimeoutError"));
      }, PEER_APPEARANCE_TIMEOUT_MS);
      (timeout as unknown as { unref?: () => void }).unref?.();
      void readMachineAppearance(remoteId, request.signal).then(appearance => {
        if (!controller.signal.aborted && !request.signal.aborted && requestVersion === version) {
          publish(appearance);
        }
      }).catch(() => {
        if (!controller.signal.aborted && (!request.signal.aborted || timedOut) && !settled) {
          const current = targetRef.current;
          if (current?.id === remoteId) discardWorkspaceSwitchOrigin(current);
        }
      }).finally(() => {
        clearTimeout(timeout);
      });
    };
    const onAppearanceChanged = (event: Event) => {
      const detail = (event as CustomEvent<{ target?: AppearanceTarget; receipt?: AppearanceProposal }>).detail;
      const receipt = detail?.receipt;
      if (!receipt) return;
      const changedPeer = detail.target?.peer ?? receipt.machine?.id;
      if (changedPeer !== remoteId) return;
      const next = receipt.state === "undone" ? receipt.previous : receipt.look;
      if (!next?.theme) return;
      readController?.abort();
      readController = undefined;
      version++;
      publish({ theme: next.theme, wallpaper: next.wallpaper ?? null, tuning: next.tuning ?? null });
    };
    const stopRail = peerEventRail.observe(remoteId, event => {
      if (event.type === "config-changed" || event.type === "reconcile") refresh();
    });
    window.addEventListener(APPEARANCE_CHANGED, onAppearanceChanged);
    refresh();
    return () => {
      controller.abort();
      readController?.abort();
      stopRail();
      window.removeEventListener(APPEARANCE_CHANGED, onAppearanceChanged);
    };
  }, [remoteId]);

  const remote = remoteId !== null;
  const selected = remote && peer?.targetId === remoteId ? peer.appearance : null;
  return {
    appearance: selected ?? { theme: config.theme, wallpaper: config.chat_wallpaper ?? null, tuning: config.appearance ?? null },
    peerId,
    ready: remote ? selected !== null : ready,
    projectWallpaper(wallpaper) {
      if (remote) setPeer(previous => previous?.targetId === remoteId
        ? { targetId: remoteId, appearance: { ...previous.appearance, wallpaper } }
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
