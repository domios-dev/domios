import { useEffect, type RefObject } from "react";

export function useHammerStrike(iconRef: RefObject<SVGSVGElement | null>, active: boolean, purpose = "build") {
  useEffect(() => {
    const icon = iconRef.current;
    if (!active || !icon) return;
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let animation: Animation | undefined;
    const updateMotion = () => {
      animation?.cancel();
      const frames = purpose === "qa" ? [
        { transform: "rotate(-5deg)" },
        { transform: "rotate(5deg)" },
        { transform: "rotate(-5deg)" },
      ] : [
        { transform: "rotate(0deg)", transformOrigin: "75% 75%", offset: 0 },
        { transform: "rotate(-18deg)", transformOrigin: "75% 75%", offset: 0.4 },
        { transform: "rotate(8deg)", transformOrigin: "75% 75%", offset: 0.55 },
        { transform: "rotate(0deg)", transformOrigin: "75% 75%", offset: 1 },
      ];
      animation = motion.matches ? undefined : icon.animate(frames, { duration: purpose === "qa" ? 1200 : 850, iterations: Infinity, easing: "ease-in-out" });
    };
    updateMotion();
    motion.addEventListener("change", updateMotion);
    return () => {
      animation?.cancel();
      motion.removeEventListener("change", updateMotion);
    };
  }, [active, iconRef, purpose]);
}
