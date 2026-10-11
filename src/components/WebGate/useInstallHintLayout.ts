import { useLayoutEffect, useState } from "react";
import { installHintLayout } from "./installHintLayout";

export function useInstallHintLayout(touch: boolean, visible: boolean) {
  const [layout, setLayout] = useState<ReturnType<typeof installHintLayout>>({});
  useLayoutEffect(() => {
    if (!touch || !visible) {
      setLayout({});
      return;
    }
    const viewport = window.visualViewport;
    const resize = new ResizeObserver(measure);
    const observed = new Set<Element>();
    function measure() {
      const titlebar = document.querySelector('[data-testid="titlebar"]');
      const composers = [...document.querySelectorAll('[data-composer-height-surface]')];
      const elements = [titlebar, ...composers].filter((element): element is Element => element !== null);
      for (const element of observed) {
        if (!elements.includes(element)) { resize.unobserve(element); observed.delete(element); }
      }
      for (const element of elements) {
        if (!observed.has(element)) { resize.observe(element); observed.add(element); }
      }
      const composerTops = composers.map((element) => element.getBoundingClientRect()).filter((rect) => rect.width > 0 && rect.height > 0).map((rect) => rect.top);
      const next = installHintLayout({
        touch,
        viewportTop: viewport?.offsetTop ?? 0,
        viewportHeight: viewport?.height ?? window.innerHeight,
        titlebarBottom: titlebar?.getBoundingClientRect().bottom ?? 0,
        composerTop: composerTops.length ? Math.min(...composerTops) : null,
      });
      setLayout((previous) => previous.top === next.top && previous.maxHeight === next.maxHeight && previous.display === next.display ? previous : next);
    }
    const mutations = new MutationObserver(measure);
    mutations.observe(document.body, { childList: true, subtree: true });
    window.addEventListener("resize", measure);
    viewport?.addEventListener("resize", measure);
    viewport?.addEventListener("scroll", measure);
    measure();
    return () => {
      resize.disconnect();
      mutations.disconnect();
      window.removeEventListener("resize", measure);
      viewport?.removeEventListener("resize", measure);
      viewport?.removeEventListener("scroll", measure);
    };
  }, [touch, visible]);
  return layout;
}
