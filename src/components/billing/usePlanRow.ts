import { useEffect, useRef, useState, type FocusEvent, type KeyboardEvent } from "react";

export function usePlanRow() {
  const row = useRef<HTMLDivElement>(null);
  const [edges, setEdges] = useState({ previous: false, next: false });
  useEffect(() => {
    const element = row.current;
    if (!element) return;
    const update = () => setEdges({
      previous: element.scrollLeft > 1,
      next: element.scrollWidth - element.clientWidth - element.scrollLeft > 1,
    });
    const wheel = (event: WheelEvent) => {
      if (event.ctrlKey || event.shiftKey || event.deltaX || !event.deltaY) return;
      const remaining = element.scrollWidth - element.clientWidth - element.scrollLeft;
      if (event.deltaY > 0 ? remaining <= 1 : element.scrollLeft <= 1) return;
      event.preventDefault();
      element.scrollBy({ left: event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? element.clientWidth : 1) });
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    for (const card of element.children) observer.observe(card);
    element.addEventListener("scroll", update);
    element.addEventListener("wheel", wheel, { passive: false });
    return () => {
      observer.disconnect();
      element.removeEventListener("scroll", update);
      element.removeEventListener("wheel", wheel);
    };
  }, []);
  const move = (direction: number) => {
    const element = row.current;
    if (element) element.scrollBy({ left: direction * ((element.firstElementChild?.getBoundingClientRect().width ?? element.clientWidth) + 12) });
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (event.code !== "ArrowLeft" && event.code !== "ArrowRight") return;
    event.preventDefault();
    move(event.code === "ArrowRight" ? 1 : -1);
  };
  const onFocus = (event: FocusEvent<HTMLDivElement>) => {
    event.target.closest("section")?.scrollIntoView({ block: "nearest", inline: "nearest" });
  };
  return { row, edges, move, onKeyDown, onFocus };
}
