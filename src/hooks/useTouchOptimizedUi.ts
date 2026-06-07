import { useEffect, useState } from "react";

function detectTouchOptimizedUi(): boolean {
  if (typeof window === "undefined") return false;
  const nav = window.navigator;
  return (
    nav.maxTouchPoints > 0 ||
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
