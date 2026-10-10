import { useEffect, type RefObject } from "react";

const strikeDuration = 1000;
const strikeFrames: Keyframe[] = [
  { transform: "rotate(0deg)", offset: 0, easing: "ease-out" },
  { transform: "rotate(-24deg)", offset: 0.12, easing: "ease-in" },
  { transform: "rotate(30deg)", offset: 0.22, easing: "ease-out" },
  { transform: "rotate(26deg)", offset: 0.28, easing: "ease-in-out" },
  { transform: "rotate(0deg)", offset: 0.48 },
  { transform: "rotate(0deg)", offset: 1 },
];
const sparkTrajectories = [[3, -3], [1, -4], [-3, -2]] as const;

function sparkFrames(x: number, y: number): Keyframe[] {
  return [
    { opacity: 0, transform: "translate(0px, 0px)", offset: 0 },
    { opacity: 0, transform: "translate(0px, 0px)", offset: 0.219 },
    { opacity: 0.7, transform: "translate(0px, 0px)", offset: 0.22, easing: "ease-out" },
    { opacity: 0, transform: `translate(${x}px, ${y}px)`, offset: 0.47 },
    { opacity: 0, transform: `translate(${x}px, ${y}px)`, offset: 1 },
  ];
}

export function useHammerStrike(
  hammerRef: RefObject<SVGGElement | null>,
  sparksRef: RefObject<SVGGElement | null>,
  active: boolean,
) {
  useEffect(() => {
    const hammer = hammerRef.current;
    const sparks = sparksRef.current;
    if (!active || !hammer || !sparks) return;
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let animations: Animation[] = [];
    const updateMotion = () => {
      animations.forEach((animation) => animation.cancel());
      animations = [];
      if (motion.matches) return;
      const timing = { duration: strikeDuration, iterations: Infinity };
      animations.push(hammer.animate(strikeFrames, timing));
      sparks.querySelectorAll("circle").forEach((spark, index) => {
        const [x, y] = sparkTrajectories[index];
        animations.push(spark.animate(sparkFrames(x, y), timing));
      });
    };
    updateMotion();
    motion.addEventListener("change", updateMotion);
    return () => {
      animations.forEach((animation) => animation.cancel());
      motion.removeEventListener("change", updateMotion);
    };
  }, [active, hammerRef, sparksRef]);
}
