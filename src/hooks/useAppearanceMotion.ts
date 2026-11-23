import { useEffect, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { appearanceLookKey, type AppearanceLook } from "../lib/appearance";
import { listenAppearanceCommit, type AppearanceCommit } from "../lib/appearanceClient";
import { animateAppearance, finishAppearanceMotion, takeAppearancePress } from "../lib/appearanceMotion";
import { effectiveWallpaper } from "../lib/chatWallpaper";
import type { SelectedMachineAppearance, WallpaperAsset } from "../lib/useMachineAppearance";
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

function palette({ chat_background: _wallpaper, name: _name, ...colors }: AppearanceLook["theme"]) {
  return JSON.stringify(colors);
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
  const intent = useRef<{ peerId?: string; key: string; commit: AppearanceCommit } | undefined>(undefined);

  useEffect(() => {
    let alive = true;
    let release: (() => void) | undefined;
    if (ready) void listenAppearanceCommit(commit => {
      const next = appearanceLookKey(commit.look);
      if (alive && appearanceLookKey(current.current.look) !== next) intent.current = { peerId, key: next, commit };
    }, peerId).then(fn => { if (alive) release = fn; else fn(); }).catch(() => {});
    return () => { alive = false; release?.(); };
  }, [ready, peerId]);

  useEffect(() => {
    const reset = () => {
      canAnimate.current = false;
      intent.current = undefined;
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
    const matched = remembered?.peerId === peerId && remembered?.key === incoming.key ? remembered : undefined;
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
      fadeContent: palette(before.look.theme) !== palette(incoming.look.theme),
      drawDoodles: isNewDoodlePattern(before.look, incoming.look) }, apply);
  }, [incoming, ready, assetReady, asset, peerId]);

  return applied;
}
