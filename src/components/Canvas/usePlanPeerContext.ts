import { useEffect, useState } from "react";
import { IS_WEB } from "../../lib/webApi";

export function usePlanPeerContext() {
  const [context, setContext] = useState({ generation: 0, ready: true });

  useEffect(() => {
    if (!IS_WEB) return;
    const reset = (event: Event) => {
      const generation = (event as CustomEvent<{ switchId: number }>).detail.switchId;
      setContext({ generation, ready: false });
    };
    const rebind = (event: Event) => {
      const generation = (event as CustomEvent<{ switchId: number }>).detail.switchId;
      setContext((current) => current.generation === generation
        ? { generation, ready: true }
        : current);
    };
    window.addEventListener("codeterm-peer-switch-reset", reset);
    window.addEventListener("codeterm-peer-switch-rebind", rebind);
    return () => {
      window.removeEventListener("codeterm-peer-switch-reset", reset);
      window.removeEventListener("codeterm-peer-switch-rebind", rebind);
    };
  }, []);

  return context;
}
