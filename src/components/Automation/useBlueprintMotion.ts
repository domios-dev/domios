import { useEffect, useState, type RefObject } from "react";

export function useBlueprintMotion(owner?: RefObject<HTMLElement | null>): boolean {
  const read = () => typeof window !== "undefined" && (
    window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true
    || (owner?.current ?? document.body)?.closest('[data-motion="paused"]') != null
  );
  const [staticMotion, setStaticMotion] = useState(read);
  useEffect(() => {
    const query = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    const sync = () => setStaticMotion(
      query?.matches === true || (owner?.current ?? document.body)?.closest('[data-motion="paused"]') != null,
    );
    const observer = typeof MutationObserver === "undefined" ? null : new MutationObserver(sync);
    let ancestor: HTMLElement | null = owner?.current ?? document.body;
    while (observer && ancestor) {
      observer.observe(ancestor, { attributes: true, attributeFilter: ["data-motion"] });
      ancestor = ancestor.parentElement;
    }
    sync();
    query?.addEventListener("change", sync);
    return () => {
      query?.removeEventListener("change", sync);
      observer?.disconnect();
    };
  }, [owner]);
  return !staticMotion;
}
