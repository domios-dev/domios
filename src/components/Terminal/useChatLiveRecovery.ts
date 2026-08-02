import { useEffect } from "react";

export function useChatLiveRecovery(active: boolean, recover: () => void): void {
  useEffect(() => {
    if (!active) return;
    let hidden = document.visibilityState === "hidden";
    let blurred = !document.hasFocus();
    const onVisibilityChange = () => {
      if (document.visibilityState === "hidden") {
        hidden = true;
      } else if (hidden) {
        hidden = false;
        recover();
      }
    };
    const onBlur = () => { blurred = true; };
    const onFocus = () => {
      if (!blurred) return;
      blurred = false;
      recover();
    };
    const onPageShow = (event: PageTransitionEvent) => {
      if (event.persisted) recover();
    };
    window.addEventListener("online", recover);
    window.addEventListener("blur", onBlur);
    window.addEventListener("focus", onFocus);
    window.addEventListener("pageshow", onPageShow);
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.removeEventListener("online", recover);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("pageshow", onPageShow);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [active, recover]);
}
