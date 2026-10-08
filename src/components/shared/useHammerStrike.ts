import { useEffect, type RefObject } from "react";

export function useHammerStrike(iconRef: RefObject<SVGSVGElement | null>, active: boolean) {
  useEffect(() => {
    const icon = iconRef.current;
    if (!active || !icon) return;
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let animation: Animation | undefined;
    const updateMotion = () => {
      animation?.cancel();
      animation = motion.matches ? undefined : icon.animate([
        { transform: "rotate(0deg)", transformOrigin: "75% 75%", offset: 0 },
        { transform: "rotate(-18deg)", transformOrigin: "75% 75%", offset: 0.4 },
        { transform: "rotate(8deg)", transformOrigin: "75% 75%", offset: 0.55 },
        { transform: "rotate(0deg)", transformOrigin: "75% 75%", offset: 1 },
      ], { duration: 850, iterations: Infinity, easing: "ease-in-out" });
    };
    updateMotion();
    motion.addEventListener("change", updateMotion);
    return () => {
      animation?.cancel();
      motion.removeEventListener("change", updateMotion);
    };
  }, [active, iconRef]);
}
