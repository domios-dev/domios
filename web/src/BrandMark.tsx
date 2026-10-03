import { THRESHOLD } from './brand/motion';
import { useBrandMotion } from './brand/useBrandMotion';
import type { BrandMotion } from './brand/useBrandMotion';

export function BrandMark({ motion = 'static', onRest }: { motion?: BrandMotion; onRest?: () => void }) {
  const ref = useBrandMotion(motion, onRest);
  return <svg ref={ref} className="brand-mark" width="30" height="30" viewBox="0 0 64 64" aria-hidden="true" focusable="false"><g><path d={THRESHOLD} fill="currentColor" fillRule="evenodd" /></g></svg>;
}
