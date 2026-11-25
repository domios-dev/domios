import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import type { AppearanceLook } from "../../lib/appearance";
import type { OpenAppearance } from "../../lib/appearanceClient";
import { useBackDismissal } from "../../lib/backDismissal";
import { uid } from "../../lib/uid";

type TabLocation = { workspaceId: string; tabId: string };
export type AppearanceLabOrigin = TabLocation | "settings";
export type AppearanceLabProjection = { look: AppearanceLook; peer?: string; committed?: boolean };

export interface AppearanceLabSession {
  id: string;
  main: TabLocation;
  previousTab?: string;
  machine?: string;
  origin?: AppearanceLabOrigin;
  opener?: HTMLElement;
  proposal?: OpenAppearance;
  showWallpaper?: boolean;
}

export function useAppearanceLabSession(workspaceShown: boolean, tab: string | undefined, machine: string | undefined) {
  const [state, setState] = useState<{ session: AppearanceLabSession; entered: boolean; projection?: AppearanceLabProjection }>();
  const session = state?.session;
  const open = Boolean(session && workspaceShown && machine === session.machine && tab === session.main.tabId);
  if (state && !open && (state.entered || machine !== session?.machine || tab !== session?.previousTab)) {
    setState(undefined);
  } else if (state && open && !state.entered) {
    setState({ ...state, entered: true });
  }
  useEffect(() => {
    if (!session) return;
    const close = () => setState(undefined);
    const onVisibility = () => { if (document.hidden) close(); };
    window.addEventListener("pagehide", close);
    window.addEventListener("codeterm-peer-switch-reset", close);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.removeEventListener("pagehide", close);
      window.removeEventListener("codeterm-peer-switch-reset", close);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [session]);

  const project = useCallback((projection: AppearanceLabProjection | undefined) => setState(current =>
    current && current.session === session && current.projection !== projection ? { ...current, projection } : current), [session]);
  const enter = useCallback((next: Omit<AppearanceLabSession, "id">) => setState({ session: { ...next, id: uid() }, entered: false }), []);
  const close = useCallback(() => setState(undefined), []);
  return {
    session: open ? session : undefined,
    projection: open ? state?.projection : undefined,
    enter,
    close,
    project,
  };
}

export function useAppearanceLabDismissal(
  open: boolean,
  popover: "changes" | "menu" | undefined,
  changesTrigger: RefObject<HTMLButtonElement | null>,
  closeChanges: () => void,
  dismiss: () => void,
) {
  useBackDismissal(open, dismiss);
  const handlers = useRef({ popover, closeChanges, dismiss });
  handlers.current = { popover, closeChanges, dismiss };
  useEffect(() => {
    if (!open) return;
    const escape = (event: KeyboardEvent) => {
      const { popover, closeChanges, dismiss } = handlers.current;
      if ((event.code || event.key) !== "Escape" || popover === "menu" || event.defaultPrevented) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      if (popover === "changes") {
        closeChanges();
        changesTrigger.current?.focus({ preventScroll: true });
      } else dismiss();
    };
    window.addEventListener("keydown", escape, true);
    return () => window.removeEventListener("keydown", escape, true);
  }, [open, changesTrigger]);
}
