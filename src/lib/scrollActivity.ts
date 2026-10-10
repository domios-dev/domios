const IDLE_MS = 900;

/** Marks whichever element is scrolling with `data-scrolling` until it rests, so scrollbars show only in motion. */
export function installScrollActivity(doc: Document = document): () => void {
  const timers = new Map<Element, ReturnType<typeof setTimeout>>();
  const onScroll = (event: Event) => {
    const target = event.target === doc ? doc.documentElement : event.target;
    if (!target || !("setAttribute" in target)) return;
    const element = target as Element;
    if (!element.hasAttribute("data-scrolling")) element.setAttribute("data-scrolling", "");
    const pending = timers.get(element);
    if (pending) clearTimeout(pending);
    timers.set(element, setTimeout(() => {
      element.removeAttribute("data-scrolling");
      timers.delete(element);
    }, IDLE_MS));
  };
  doc.addEventListener("scroll", onScroll, { capture: true, passive: true });
  return () => {
    doc.removeEventListener("scroll", onScroll, { capture: true });
    for (const pending of timers.values()) clearTimeout(pending);
    timers.clear();
  };
}
