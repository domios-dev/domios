import { THRESHOLD } from "./motion";
import { useBrandMotion, type BrandMotion } from "./useBrandMotion";

export function BrandMark({ motion = "static", size = 64, onRelease, beatEvery }: { motion?: BrandMotion; size?: number; onRelease?: (timestamp: number) => void; beatEvery?: number }) {
  const ref = useBrandMotion(motion, onRelease, beatEvery);
  return (
    <svg ref={ref} viewBox="-4 -4 72 72" width={size} height={size} aria-hidden="true" focusable="false" className="brand-mark">
      <g><path d={THRESHOLD} fill="currentColor" fillRule="evenodd" /></g>
    </svg>
  );
}
