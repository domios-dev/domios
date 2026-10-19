import { THRESHOLD } from "./motion";
import { useBrandMotion, type BrandMotion } from "./useBrandMotion";

export function BrandMark({ motion = "static", size = 64 }: { motion?: BrandMotion; size?: number }) {
  const ref = useBrandMotion(motion);
  return (
    <svg ref={ref} viewBox="-4 -4 72 72" width={size} height={size} aria-hidden="true" focusable="false" className="brand-mark">
      <g><path d={THRESHOLD} fill="currentColor" fillRule="evenodd" /></g>
    </svg>
  );
}
