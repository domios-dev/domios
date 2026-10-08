import { useEffect, useRef, useState, type RefObject } from "react";
import { createStillHoverHint, messageHintBudget } from "./messageActionHint";

export function useMessageActionHint(row: RefObject<HTMLElement | null>, disabled: boolean) {
  const [hint, setHint] = useState(false);
  const live = useRef(disabled);
  live.current = disabled;
  const controller = useRef<ReturnType<typeof createStillHoverHint> | null>(null);
  if (!controller.current) controller.current = createStillHoverHint({
    schedule: (callback, delay) => window.setTimeout(callback, delay),
    cancel: (handle) => window.clearTimeout(handle as number),
    allowed: () => !live.current && !window.matchMedia("(pointer: coarse)").matches && !window.matchMedia("(prefers-reduced-motion: reduce)").matches && !row.current?.closest('[data-motion="paused"]'),
    claim: () => messageHintBudget.claim(),
    show: () => setHint(true),
  });
  useEffect(() => {
    if (disabled) { controller.current?.stop(); setHint(false); }
  }, [disabled]);
  useEffect(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const stopMotion = () => {
      if (media.matches || row.current?.closest('[data-motion="paused"]')) {
        controller.current?.stop();
        setHint(false);
      }
    };
    const observer = typeof MutationObserver === "undefined" ? null : new MutationObserver(stopMotion);
    for (let node: HTMLElement | null = row.current; observer && node; node = node.parentElement) {
      observer.observe(node, { attributes: true, attributeFilter: ["data-motion"] });
    }
    media.addEventListener("change", stopMotion);
    return () => { observer?.disconnect(); media.removeEventListener("change", stopMotion); controller.current?.leave(); };
  }, [row]);
  return {
    hint,
    enter: () => controller.current?.enter(),
    move: () => controller.current?.move(),
    leave: () => { controller.current?.leave(); setHint(false); },
    finish: () => setHint(false),
    opened: () => { messageHintBudget.opened(); controller.current?.stop(); setHint(false); },
  };
}
