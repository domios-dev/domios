import { useEffect, useState } from "react";
import { IS_WEB } from "../lib/webApi";

function detectTouchOptimizedUi(): boolean {
  // The desktop Tauri app is never touch-optimized — phones/tablets reach
  // us through the web mirror. maxTouchPoints is deliberately NOT used:
  // desktop Windows machines with touch digitizers report it and would
  // flip the whole desktop UI into mobile mode.
  if (!IS_WEB || typeof window === "undefined") return false;
  return (
    window.matchMedia("(pointer: coarse)").matches ||
    window.matchMedia("(hover: none)").matches ||
    window.matchMedia("(max-width: 800px)").matches
  );
}

export function useTouchOptimizedUi(): boolean {
  const [enabled, setEnabled] = useState(detectTouchOptimizedUi);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const queries = [
      window.matchMedia("(pointer: coarse)"),
      window.matchMedia("(hover: none)"),
      window.matchMedia("(max-width: 800px)"),
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
