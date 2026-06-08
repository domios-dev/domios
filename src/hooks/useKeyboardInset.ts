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
export interface KeyboardState {
  // Layout-viewport gap used to shrink the app shell in plain-browser mode (0 in standalone, where #root tracks --vv-height itself).
  inset: number;
  // True whenever the soft keyboard overlaps the visual viewport, in BOTH standalone and browser — drives composer bottom-padding collapse.
  keyboardOpen: boolean;
}

export function useKeyboardInset(): KeyboardState {
  const [inset, setInset] = useState(0);
  const [keyboardOpen, setKeyboardOpen] = useState(false);

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
      // Ignore sub-100px gaps: iPadOS with an external keyboard fires viewport events on input
      // focus (for cursor scroll) without showing the software keyboard, producing a small gap
      // that collapses the app shell to ~0 height and causes a gray screen.
      setInset(Math.max(0, Math.round(gap > 100 ? gap : 0)));
      // The layout viewport (innerHeight) stays full under the keyboard on iOS in BOTH standalone and
      // browser, so this detects the keyboard even when the shell-inset above reads 0 (standalone).
      const keyboardGap = window.innerHeight - vv.height - vv.offsetTop;
      setKeyboardOpen(keyboardGap > 100);
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

  return { inset, keyboardOpen };
}
