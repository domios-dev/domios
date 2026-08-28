import { useEffect, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import {
  sessionBindingSurfaceFromFact,
  silentSessionBindingSurface,
  type SessionBindingFact,
  type SessionBindingSurfaceState,
} from "./sessionBindingSurface";

export function useSessionBindingSurface(
  tabId: string,
  sessionId: string | null,
): SessionBindingSurfaceState {
  const [surface, setSurface] = useState(silentSessionBindingSurface);

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;
    const apply = (fact: SessionBindingFact) => {
      if (!cancelled) setSurface(sessionBindingSurfaceFromFact(tabId, sessionId, fact));
    };
    setSurface(silentSessionBindingSurface);

    const subscribed = listen<SessionBindingFact>(
      "session-binding-state-changed",
      (event) => apply(event.payload),
    ).then((nextUnlisten) => {
      if (cancelled) nextUnlisten();
      else unlisten = nextUnlisten;
    }).catch(() => undefined);
    void subscribed
      .then(() => invoke<SessionBindingFact>("session_binding_state", { tabId }))
      .then(apply)
      .catch(() => undefined);

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [sessionId, tabId]);

  return surface;
}
