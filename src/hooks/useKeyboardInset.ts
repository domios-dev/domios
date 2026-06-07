import { useEffect, useState } from "react";
import { IS_WEB } from "../lib/webApi";

/**
 * Height (px) of the on-screen keyboard overlapping the layout viewport.
 *
 * Android Chrome with `interactive-widget=resizes-content` resizes the
 * layout viewport itself, so this stays 0 there. iOS Safari (and older
 * Android) only shrink the VISUAL viewport — the layout keeps its size
 * and bottom-anchored UI ends up hidden behind the keyboard. This hook
 * measures that gap via the VisualViewport API; apply it as bottom
 * padding on the app shell so the chat composer rides above the keys.
 */
export function useKeyboardInset(): number {
  const [inset, setInset] = useState(0);

  useEffect(() => {
    if (!IS_WEB || typeof window === "undefined" || !window.visualViewport) {
      return;
    }
    const vv = window.visualViewport;
    const update = () => {
      // Standalone #root tracks --vv-height (see index.html), so measure the gap against the
      // actual shell height — measuring against innerHeight would double-subtract the keyboard.
      document.documentElement.style.setProperty(
        "--vv-height",
        `${Math.round(vv.height)}px`,
      );
      const rootHeight =
        document.getElementById("root")?.clientHeight ?? window.innerHeight;
      const gap = rootHeight - vv.height - vv.offsetTop;
      setInset(Math.max(0, Math.round(gap)));
      // iOS scrolls the page when the keyboard opens — pin it so fixed chrome (sidebar burger) stays on-screen.
      if (vv.offsetTop > 0) window.scrollTo(0, 0);
    };
    vv.addEventListener("resize", update);
    vv.addEventListener("scroll", update);
    // Standalone iOS sometimes restores innerHeight without a VV event — re-measure on window resize too.
    window.addEventListener("resize", update);
    update();
    return () => {
      vv.removeEventListener("resize", update);
      vv.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
    };
  }, []);

  return inset;
}
