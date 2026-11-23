import { useEffect, type ReactNode } from "react";
import { SHELL_NARROW_QUERY } from "../../../../src/lib/shellBreakpoint";
import { PhoneViewport } from "../../gallery/stageTools";

// The gallery portal centres its phone stage with a width cap and top padding; the film places the shell itself.
const PHONE_SCOPE_RESET = ".film-phone > .gallery-phone-viewport { width: auto; padding-top: 0; }";

let narrow = false;
let installed = false;

function narrowAnswer(query: string): MediaQueryList {
  return {
    matches: true, media: query, onchange: null,
    addEventListener: () => undefined, removeEventListener: () => undefined,
    addListener: () => undefined, removeListener: () => undefined, dispatchEvent: () => false,
  };
}

function answerNarrow(): void {
  narrow = true;
  if (installed) return;
  installed = true;
  const native = window.matchMedia.bind(window);
  window.matchMedia = (query: string) => (narrow && query === SHELL_NARROW_QUERY ? narrowAnswer(query) : native(query));
}

/** The product at phone width inside a desktop-sized stage: SHELL_NARROW answers true while mounted and phone @media rules apply under it. */
export function PhoneShell({ children }: { children: ReactNode }) {
  // Set during render so usePhoneViewport's initial state in the children already reads the phone answer.
  answerNarrow();
  useEffect(() => {
    answerNarrow();
    return () => {
      narrow = false;
    };
  }, []);
  return <div className="film-phone">
    <style>{PHONE_SCOPE_RESET}</style>
    <PhoneViewport>{children}</PhoneViewport>
  </div>;
}
