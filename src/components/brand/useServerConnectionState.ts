import { useEffect, useRef, useState } from "react";
import { TERMINAL_VIEWER_DISCONNECTED_EVENT, TERMINAL_VIEWER_RECONNECTED_EVENT } from "../../lib/terminalViewerEvents";
import { RECONNECTED_NOTICE_MS, RECONNECT_GRACE_MS, transitionBrandConnection } from "./brandStateModel";
import type { BrandConnectionState } from "./brandStateModel";

export function useServerConnectionState() {
  const [state, setState] = useState<BrandConnectionState>({ kind: "connected" });
  const current = useRef(state);

  useEffect(() => {
    let graceTimer: number | undefined;
    let noticeTimer: number | undefined;
    const clearGrace = () => {
      if (graceTimer !== undefined) window.clearTimeout(graceTimer);
      graceTimer = undefined;
    };
    const clearNotice = () => {
      if (noticeTimer !== undefined) window.clearTimeout(noticeTimer);
      noticeTimer = undefined;
    };
    const commit = (event: "grace-expired" | "notice-expired", now: number) => {
      const next = transitionBrandConnection(current.current, event, now);
      if (next === current.current) return;
      current.current = next;
      setState(next);
    };
    const onDisconnect = () => {
      const before = current.current;
      const next = transitionBrandConnection(before, "disconnect", Date.now());
      if (next === before) return;
      if (next.kind !== "waiting") return;
      clearNotice();
      current.current = next;
      setState(next);
      graceTimer = window.setTimeout(() => {
        graceTimer = undefined;
        commit("grace-expired", Date.now());
      }, Math.max(0, next.disconnectedAt + RECONNECT_GRACE_MS - Date.now()));
    };
    const onReconnect = () => {
      const before = current.current;
      const next = transitionBrandConnection(before, "connect", Date.now());
      if (next === before) return;
      clearGrace();
      current.current = next;
      setState(next);
      if (next.kind === "reconnected") {
        clearNotice();
        noticeTimer = window.setTimeout(() => {
          noticeTimer = undefined;
          commit("notice-expired", Date.now());
        }, RECONNECTED_NOTICE_MS);
      }
    };

    window.addEventListener(TERMINAL_VIEWER_DISCONNECTED_EVENT, onDisconnect);
    window.addEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, onReconnect);
    return () => {
      clearGrace();
      clearNotice();
      window.removeEventListener(TERMINAL_VIEWER_DISCONNECTED_EVENT, onDisconnect);
      window.removeEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, onReconnect);
    };
  }, []);

  return state;
}
