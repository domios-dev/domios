import { useRef, useState } from 'react';

export function useBrandEntrance(reducedMotion: boolean) {
  const settled = useRef(reducedMotion || document.hidden);
  const [ready, setReady] = useState(settled.current);
  const onRest = () => {
    if (settled.current) return;
    settled.current = true;
    setReady(true);
  };
  return { state: ready || reducedMotion ? 'ready' : 'pending', onRest };
}
