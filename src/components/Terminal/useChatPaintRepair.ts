import { useLayoutEffect, type RefObject } from "react";
import { chatScrollDiagnostic } from "./chatScrollDiagnostic";

export function useChatPaintRepair(scrollRef: RefObject<HTMLDivElement | null>, active: boolean, scope: string, repair: () => void) {
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || !active) return;
    let frame: number | null = null;
    const runRepair = () => {
      const gap = el.scrollHeight - el.clientHeight - el.scrollTop;
      if (gap > 0.5) chatScrollDiagnostic(scope, "before-paint-repair", { gap, scrollTop: el.scrollTop,
        scrollHeight: el.scrollHeight, clientHeight: el.clientHeight, inset: (el.firstElementChild as HTMLElement | null)?.style.paddingBottom });
      repair();
      if (gap > 0.5) chatScrollDiagnostic(scope, "after-paint-repair", { gap: el.scrollHeight - el.clientHeight - el.scrollTop,
        scrollTop: el.scrollTop, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight });
    };
    const schedule = () => {
      if (frame != null) return;
      frame = window.requestAnimationFrame(() => { frame = null; runRepair(); });
    };
    const shell = el.closest(".ct-chat-shell") ?? el;
    const mutations = new window.MutationObserver(schedule);
    mutations.observe(shell, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ["style", "class"] });
    const resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(runRepair);
    resize?.observe(el);
    window.addEventListener("resize", schedule);
    window.visualViewport?.addEventListener("resize", schedule);
    return () => {
      mutations.disconnect();
      resize?.disconnect();
      window.removeEventListener("resize", schedule);
      window.visualViewport?.removeEventListener("resize", schedule);
      if (frame != null) window.cancelAnimationFrame(frame);
    };
  }, [scrollRef, active, scope, repair]);
}
