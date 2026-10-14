import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import { syncChatTimeGutter } from "./chatTimeGutter";
import { chatScrollDiagnostic } from "./chatScrollDiagnostic";
import { chatFloatingBottomInset } from "./chatLayoutInsets";

export function useChatLayoutInsets(rootRef: RefObject<HTMLDivElement | null>): number {
  const previousInset = useRef(0);
  const [inset, setInset] = useState(0);
  useLayoutEffect(() => {
    const root = rootRef.current;
    const bar = root?.querySelector<HTMLElement>(".ct-chat-bottom-bar");
    if (!root || !bar) return;
    const surface = root.querySelector<HTMLElement>(".ct-chat-surface");
    let surfaceWidth = 0;
    const measureTimeGutter = () => { if (surface) syncChatTimeGutter(surface, surfaceWidth); };
    const measure = () => {
      surfaceWidth = surface?.getBoundingClientRect().width ?? 0;
      measureTimeGutter();
      const tops = Array.from(bar.querySelectorAll<HTMLElement>(".ct-ctx-pill, .ct-agent-stop"))
        .filter((element) => element.getClientRects().length && window.getComputedStyle(element).visibility !== "hidden")
        .map((element) => element.getBoundingClientRect().top);
      const next = chatFloatingBottomInset(bar.getBoundingClientRect().top, tops);
      if (next !== previousInset.current) {
        chatScrollDiagnostic(root.dataset.tabId ?? "chat", "inset", { before: previousInset.current, inset: next, barTop: bar.getBoundingClientRect().top });
        previousInset.current = next;
      }
      setInset(next);
    };
    const resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    resize?.observe(bar);
    if (surface) resize?.observe(surface);
    bar.querySelectorAll(".ct-ctx-pill, .ct-agent-stop").forEach((element) => resize?.observe(element));
    const mutations = new window.MutationObserver(() => {
      bar.querySelectorAll(".ct-ctx-pill, .ct-agent-stop").forEach((element) => resize?.observe(element));
      measure();
    });
    mutations.observe(bar, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "aria-hidden"] });
    const transcript = new window.MutationObserver(measureTimeGutter);
    if (surface) transcript.observe(surface, { childList: true, subtree: true });
    window.addEventListener("resize", measure);
    measure();
    return () => { resize?.disconnect(); mutations.disconnect(); transcript.disconnect(); window.removeEventListener("resize", measure); };
  }, [rootRef]);
  return inset;
}
