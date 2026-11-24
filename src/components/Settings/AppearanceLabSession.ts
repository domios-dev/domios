import { useCallback, useEffect, useState } from "react";
import type { AppearanceLook } from "../../lib/appearance";
import type { OpenAppearance } from "../../lib/appearanceClient";

type TabLocation = { workspaceId: string; tabId: string };
export type AppearanceLabOrigin = TabLocation | "settings";
export type AppearanceLabProjection = { look: AppearanceLook; peer?: string; committed?: boolean };

interface Session {
  main: TabLocation;
  previousTab?: string;
  machine?: string;
  origin?: AppearanceLabOrigin;
  proposal?: OpenAppearance;
  showWallpaper?: boolean;
}

export function useAppearanceLabSession(workspaceShown: boolean, tab: string | undefined, machine: string | undefined) {
  const [state, setState] = useState<{ session: Session; entered: boolean; projection?: AppearanceLabProjection }>();
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
  const enter = useCallback((next: Session) => setState({ session: next, entered: false }), []);
  const close = useCallback(() => setState(undefined), []);
  return {
    session: open ? session : undefined,
    projection: open ? state?.projection : undefined,
    enter,
    close,
    project,
  };
}
