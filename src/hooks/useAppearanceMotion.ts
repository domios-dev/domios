import { useEffect, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { appearanceLookKey, type AppearanceLook, type AppearanceProposal } from "../lib/appearance";
import { APPEARANCE_CHANGED, listenAppearanceCommit, type AppearanceCommit, type AppearanceTarget } from "../lib/appearanceClient";
import { animateAppearance, finishAppearanceMotion, takeAppearancePress } from "../lib/appearanceMotion";
import { effectiveWallpaper } from "../lib/chatWallpaper";
import type { SelectedMachineAppearance, WallpaperAsset } from "../lib/useMachineAppearance";
import { relayTarget } from "../lib/webApi";
import { clearWorkspaceSwitchOrigin, workspaceSwitchOrigin } from "../lib/workspaceBrowseTarget";

interface AppliedLook {
  look: AppearanceLook;
  peerId?: string;
  asset?: WallpaperAsset;
}

function isNewDoodlePattern(before: AppearanceLook, after: AppearanceLook) {
  return JSON.stringify(before.wallpaper) !== JSON.stringify(after.wallpaper)
    && effectiveWallpaper(before.theme, before.wallpaper ?? undefined).background.pattern !== "doodles"
    && effectiveWallpaper(after.theme, after.wallpaper ?? undefined).background.pattern === "doodles";
}

export function useAppearanceMotion({ ready, appearance: { theme, wallpaper: background, tuning }, peerId }: SelectedMachineAppearance,
  wallpaper?: { asset?: WallpaperAsset; ready: boolean }): AppliedLook {
  const nextLook = useMemo(() => {
    const look = { theme, wallpaper: background, tuning: tuning ?? null };
    return { look, key: appearanceLookKey(look) };
  }, [theme, background, tuning]);
  const [incoming, setIncoming] = useState(nextLook);
  if (incoming.key !== nextLook.key) setIncoming(nextLook);

  const asset = wallpaper?.asset;
  const assetReady = wallpaper?.ready ?? true;
  const [applied, setApplied] = useState<AppliedLook>({ look: incoming.look, peerId, asset });
  const current = useRef(applied);
  const canAnimate = useRef(false);
  const intent = useRef<{ peerId?: string; key?: string; commit: AppearanceCommit } | undefined>(undefined);
  const handledCommit = useRef<{ peerId: string; proposal: string; undo: boolean } | undefined>(undefined);

  useEffect(() => {
    let alive = true;
    let release: (() => void) | undefined;
    const remote = Boolean(peerId && peerId !== relayTarget()?.id);
    if (ready) void listenAppearanceCommit(commit => {
      if (!alive) return;
      const handled = handledCommit.current;
      if (remote && commit.proposal && handled && handled.peerId === peerId
        && handled.proposal === commit.proposal && handled.undo === commit.undo) {
        handledCommit.current = undefined;
        return;
      }
      const next = commit.look ? appearanceLookKey(commit.look) : undefined;
      if (!next || appearanceLookKey(current.current.look) !== next) intent.current = { peerId, key: next, commit };
    }, peerId).then(fn => { if (alive) release = fn; else fn(); }).catch(() => {});
    const onAppearanceChanged = (event: Event) => {
      if (!alive || !remote || !peerId) return;
      const detail = (event as CustomEvent<{ target?: AppearanceTarget; receipt?: AppearanceProposal }>).detail;
      const receipt = detail?.receipt;
      if (!receipt || (detail.target?.peer ?? receipt.machine?.id) !== peerId) return;
      const look = receipt.state === "undone" ? receipt.previous : receipt.look;
      if (!look?.theme) return;
      const next = appearanceLookKey(look);
      const undo = receipt.state === "undone";
      handledCommit.current = { peerId, proposal: receipt.id, undo };
      if (appearanceLookKey(current.current.look) !== next) {
        intent.current = {
          peerId,
          key: next,
          commit: { event: receipt.id, machine: receipt.machine.id, proposal: receipt.id, undo, look },
        };
      }
    };
    if (ready && remote) window.addEventListener(APPEARANCE_CHANGED, onAppearanceChanged);
    return () => {
      alive = false;
      release?.();
      if (ready && remote) window.removeEventListener(APPEARANCE_CHANGED, onAppearanceChanged);
    };
  }, [ready, peerId]);

  useEffect(() => {
    const reset = () => {
      canAnimate.current = false;
      intent.current = undefined;
      handledCommit.current = undefined;
      clearWorkspaceSwitchOrigin();
      finishAppearanceMotion();
    };
    window.addEventListener("codeterm-peer-switch-reset", reset);
    return () => {
      window.removeEventListener("codeterm-peer-switch-reset", reset);
      finishAppearanceMotion();
    };
  }, []);

  useEffect(() => {
    const before = current.current;
    const switching = before.peerId !== peerId;
    if (!ready || !assetReady) {
      if (!ready && !switching) canAnimate.current = false;
      return;
    }
    const switchOrigin = workspaceSwitchOrigin();
    const changed = appearanceLookKey(before.look) !== incoming.key;
    const next = { look: incoming.look, peerId, asset };
    const animate = changed && canAnimate.current && (!switching || switchOrigin !== undefined);
    const remembered = intent.current;
    const matched = remembered && remembered.peerId === peerId
      && (remembered.key === undefined || remembered.key === incoming.key) ? remembered : undefined;
    const apply = (sync: boolean) => {
      canAnimate.current = true;
      clearWorkspaceSwitchOrigin();
      if (changed || switching) intent.current = undefined;
      if (!changed && !switching && before.asset === asset) return;
      current.current = next;
      if (sync) flushSync(() => setApplied(next));
      else setApplied(next);
    };
    const pressed = changed && !switching ? takeAppearancePress(incoming.key, peerId) : undefined;
    if (!animate) { apply(false); return; }
    const proposal = switching ? null : matched?.commit.proposal ?? pressed?.proposal ?? null;
    const undo = !switching && (matched?.commit.undo ?? false);
    const origin = switching ? switchOrigin : pressed?.origin;
    return animateAppearance({ look: incoming.look, proposal, undo, origin,
      drawDoodles: isNewDoodlePattern(before.look, incoming.look) }, apply);
  }, [incoming, ready, assetReady, asset, peerId]);

  return applied;
}
