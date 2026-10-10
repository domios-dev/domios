import { useLayoutEffect, useRef } from "react";

export function useToastHostWidth(count: number) {
  const hostRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const resize = () => {
      const host = hostRef.current;
      if (host) host.style.maxWidth = `${Math.max(0, host.getBoundingClientRect().right - 8)}px`;
    };
    resize();
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, [count]);
  return hostRef;
}
