import { useEffect, useState } from "react";
import { IS_WEB } from "../lib/webApi";

export function shouldUseTouchOptimizedUi(isWeb: boolean, pointerCoarse: boolean, hoverNone: boolean): boolean {
  return isWeb && (pointerCoarse || hoverNone);
}

function detectTouchOptimizedUi(): boolean {
  // Touch controls follow input capabilities, not viewport width.
  if (!IS_WEB || typeof window === "undefined") return false;
  return shouldUseTouchOptimizedUi(
    IS_WEB,
    window.matchMedia("(pointer: coarse)").matches,
    window.matchMedia("(hover: none)").matches,
  );
}

export function useTouchOptimizedUi(): boolean {
  const [enabled, setEnabled] = useState(detectTouchOptimizedUi);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const queries = [
      window.matchMedia("(pointer: coarse)"),
      window.matchMedia("(hover: none)"),
    ];
    const update = () => setEnabled(detectTouchOptimizedUi());
    update();
    for (const query of queries) query.addEventListener("change", update);
    window.addEventListener("resize", update);
    return () => {
      for (const query of queries) query.removeEventListener("change", update);
      window.removeEventListener("resize", update);
    };
  }, []);

  return enabled;
}
