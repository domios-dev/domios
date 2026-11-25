import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import { syncChatTimeGutter } from "./chatTimeGutter";
import { chatScrollDiagnostic } from "./chatScrollDiagnostic";
import { chatFloatingBottomInset } from "./chatLayoutInsets";
import { CHAT_READABLE_WIDTH_REM } from "./chatLayoutTokens";

export function useChatLayoutInsets(rootRef: RefObject<HTMLDivElement | null>, active = true): number {
  const previousInset = useRef(0);
  const [inset, setInset] = useState(0);
  useLayoutEffect(() => {
    if (!active) return;
    const root = rootRef.current;
    const bar = root?.querySelector<HTMLElement>(".ct-chat-bottom-bar");
    if (!root || !bar) return;
    const surface = root.querySelector<HTMLElement>(".ct-chat-surface");
    const lane = bar.querySelector<HTMLElement>(".ct-composer-lane");
    const timeMeasure = document.createElement("span");
    timeMeasure.className = "ct-chat-message-time ct-chat-time-measure";
    timeMeasure.textContent = "00:00";
    timeMeasure.setAttribute("aria-hidden", "true");
    root.append(timeMeasure);
    const measureTimeGutter = () => {
      if (!surface) return;
      const bounds = surface.getBoundingClientRect();
      const rootFontSize = Number.parseFloat(window.getComputedStyle(root).fontSize);
      const columnWidth = CHAT_READABLE_WIDTH_REM * rootFontSize;
      syncChatTimeGutter(surface, bounds.width, Math.max(0, (lane ?? bar).getBoundingClientRect().left - bounds.left), timeMeasure.getBoundingClientRect().width, columnWidth);
    };
    const measure = () => {
      // Read all layout geometry before mutating styles/attributes to prevent forced reflows.
      const surfaceBounds = surface?.getBoundingClientRect();
      const laneLeft = surface ? (lane ?? bar).getBoundingClientRect().left : 0;
      const rootFontSize = surface ? Number.parseFloat(window.getComputedStyle(root).fontSize) : 0;
      const timeWidth = surface ? timeMeasure.getBoundingClientRect().width : 0;
      const barTop = bar.getBoundingClientRect().top;
      const tops = Array.from(bar.querySelectorAll<HTMLElement>(".ct-ctx-pill, .ct-agent-stop"))
        .filter((element) => element.getClientRects().length && window.getComputedStyle(element).visibility !== "hidden")
        .map((element) => element.getBoundingClientRect().top);
      if (surface && surfaceBounds) {
        syncChatTimeGutter(
          surface,
          surfaceBounds.width,
          Math.max(0, laneLeft - surfaceBounds.left),
          timeWidth,
          CHAT_READABLE_WIDTH_REM * rootFontSize,
        );
      }
      const next = chatFloatingBottomInset(barTop, tops);
      if (next !== previousInset.current) {
        chatScrollDiagnostic(root.dataset.tabId ?? "chat", "inset", { before: previousInset.current, inset: next, barTop });
        previousInset.current = next;
      }
      setInset(next);
    };
    const resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    resize?.observe(bar);
    if (surface) resize?.observe(surface);
    if (lane) resize?.observe(lane);
    resize?.observe(timeMeasure);
    bar.querySelectorAll(".ct-ctx-pill, .ct-agent-stop").forEach((element) => resize?.observe(element));
    const mutations = new window.MutationObserver(() => {
      bar.querySelectorAll(".ct-ctx-pill, .ct-agent-stop").forEach((element) => resize?.observe(element));
      measure();
    });
    mutations.observe(bar, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "aria-hidden"] });
    let transcriptFrame: number | null = null;
    const scheduleTimeGutter = () => {
      if (typeof window.requestAnimationFrame !== "function") {
        measureTimeGutter();
        return;
      }
      if (transcriptFrame !== null) return;
      transcriptFrame = window.requestAnimationFrame(() => {
        transcriptFrame = null;
        measureTimeGutter();
      });
    };
    const transcript = new window.MutationObserver(scheduleTimeGutter);
    if (surface) transcript.observe(surface, { childList: true, subtree: true });
    if (!resize) window.addEventListener("resize", measure);
    measure();
    return () => {
      resize?.disconnect();
      mutations.disconnect();
      transcript.disconnect();
      if (transcriptFrame !== null) window.cancelAnimationFrame(transcriptFrame);
      if (!resize) window.removeEventListener("resize", measure);
      timeMeasure.remove();
    };
  }, [rootRef, active]);
  return inset;
}
