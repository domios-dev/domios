import { useEffect, useState } from "react";
import { keyboardViewportState } from "./keyboardViewport";
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
      // actual shell height — measuring against innerHeight would double-subtract the keyboard.
      document.documentElement.style.setProperty(
        "--vv-height",
        `${Math.round(vv.height)}px`,
      );
      const rootHeight =
        document.getElementById("root")?.clientHeight ?? window.innerHeight;
      const state = keyboardViewportState({ rootHeight, layoutHeight: window.innerHeight, visualHeight: vv.height, offsetTop: vv.offsetTop });
      setInset(state.inset);
      setKeyboardOpen(state.keyboardOpen);
      const style = document.documentElement.style;
      style.setProperty("--ct-visual-height", `${state.visualHeight}px`);
      style.setProperty("--ct-visual-top", `${state.offsetTop}px`);
      style.setProperty("--ct-visual-bottom", `${Math.max(0, window.innerHeight - state.visualHeight - state.offsetTop)}px`);
      style.setProperty("--ct-keyboard-safe-bottom", state.keyboardOpen ? "0px" : "var(--ct-safe-bottom, 0px)");
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
