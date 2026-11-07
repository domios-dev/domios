import { useLayoutEffect, useRef } from "react";
import { toastStackTop } from "./toastPlacement";

export function useToastHostWidth(count: number) {
  const hostRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const resize = () => {
      const host = hostRef.current;
      if (host) {
        const obstacles = [...document.querySelectorAll<HTMLElement>("[data-toast-obstacle]")];
        if (obstacles.length) {
          host.style.position = "fixed";
          host.style.top = `${toastStackTop(host.getBoundingClientRect(), obstacles.map(element => element.getBoundingClientRect()))}px`;
          host.style.marginTop = "0";
        }
        host.style.maxWidth = `${Math.max(0, host.getBoundingClientRect().right - 8)}px`;
      }
    };
    resize();
    const observer = new window.ResizeObserver(resize);
    for (const element of document.querySelectorAll("[data-toast-obstacle]")) observer.observe(element);
    window.addEventListener("resize", resize);
    return () => { observer.disconnect(); window.removeEventListener("resize", resize); };
  }, [count]);
  return hostRef;
}
