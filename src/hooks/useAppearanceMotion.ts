import { useEffect, useLayoutEffect, useRef } from "react";
import type { AppearanceLook } from "../lib/appearance";
import { listenAppearanceCommit, type AppearanceCommit } from "../lib/appearanceClient";
import { animateAppearance, finishAppearanceMotion } from "../lib/appearanceMotion";
import type { SelectedMachineAppearance } from "../lib/useMachineAppearance";

function key({ theme, wallpaper, tuning }: AppearanceLook): string {
  return JSON.stringify([theme, wallpaper, tuning]);
}

export function useAppearanceMotion({ ready, appearance, peerId }: SelectedMachineAppearance): void {
  const previous = useRef<{ peerId: string | undefined; key: string } | undefined>(undefined);
  const intent = useRef<{ key: string; commit: AppearanceCommit } | undefined>(undefined);

  useEffect(() => {
    if (!ready) return;
    let alive = true;
    let release: (() => void) | undefined;
    void listenAppearanceCommit(commit => {
      const next = key(commit.look);
      if (alive && previous.current?.key !== next) intent.current = { key: next, commit };
    }, peerId).then(fn => { if (alive) release = fn; else fn(); }).catch(() => {});
    const reset = () => {
      previous.current = undefined;
      intent.current = undefined;
      finishAppearanceMotion();
    };
    window.addEventListener("codeterm-peer-switch-reset", reset);
    return () => {
      alive = false;
      release?.();
      window.removeEventListener("codeterm-peer-switch-reset", reset);
    };
  }, [ready, peerId]);

  useLayoutEffect(() => {
    if (!ready) {
      previous.current = undefined;
      intent.current = undefined;
      finishAppearanceMotion();
      return;
    }
    const look = { theme: appearance.theme, wallpaper: appearance.wallpaper, tuning: appearance.tuning ?? null };
    const next = key(look);
    const before = previous.current;
    previous.current = { peerId, key: next };
    if (!before || before.peerId !== peerId) {
      intent.current = undefined;
      finishAppearanceMotion();
      return;
    }
    if (before.key === next) return;
    const commit = intent.current?.key === next ? intent.current.commit : undefined;
    intent.current = undefined;
    animateAppearance({ look, proposal: commit?.proposal ?? null, undo: commit?.undo ?? false });
  }, [ready, peerId, appearance.theme, appearance.wallpaper, appearance.tuning]);

  useEffect(() => () => finishAppearanceMotion(), []);
}
