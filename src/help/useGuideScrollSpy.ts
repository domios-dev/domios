import { useEffect, type RefObject } from "react";
import { activeGuideSection } from "./guideScrollModel";
import type { HelpSectionContent, HelpSectionId } from "./types";

export function useGuideScrollSpy(
  scrollRef: RefObject<HTMLDivElement | null>,
  sections: readonly HelpSectionContent[],
  onActive: (id: HelpSectionId) => void,
) {
  useEffect(() => {
    const scrollport = scrollRef.current;
    if (!scrollport) return;
    const elements = sections.flatMap(({ id }) => {
      const element = scrollport.querySelector<HTMLElement>(`[data-help-section="${id}"]`);
      return element ? [{ id, element }] : [];
    });
    const update = () => {
      const top = scrollport.getBoundingClientRect().top + scrollport.clientTop;
      const id = activeGuideSection(elements.map(({ id, element }) => {
        const bounds = element.getBoundingClientRect();
        return { id, top: bounds.top, bottom: bounds.bottom };
      }), {
        top,
        bottom: top + scrollport.clientHeight,
        atEnd: scrollport.scrollTop > 0 &&
          scrollport.scrollTop + scrollport.clientHeight >= scrollport.scrollHeight - 1,
      });
      if (id) onActive(id);
    };
    scrollport.addEventListener("scroll", update, { passive: true });
    const resize = new window.ResizeObserver(update);
    resize.observe(scrollport);
    for (const { element } of elements) resize.observe(element);
    update();
    return () => {
      scrollport.removeEventListener("scroll", update);
      resize.disconnect();
    };
  }, [scrollRef, sections, onActive]);
}
