import { useEffect, useState } from "react";
import { AUTOMATION_GRAPH_EVENT } from "../Automation/hostAdapter";
import { listenOrchestration } from "../../lib/transport/orchestrationRuntime";

export function useBlueprintLibraryRevision(sources: readonly string[]): number {
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let active = true;
    let stop: (() => void) | undefined;
    let binding = 0;
    const revisions = new Map<string, string>();
    const bind = () => {
      const current = ++binding;
      stop?.();
      revisions.clear();
      void listenOrchestration<{ workspace_id: string; revision: string }>(AUTOMATION_GRAPH_EVENT, ({ payload }) => {
        if (!active || current !== binding || !payload || !sources.includes(payload.workspace_id)
          || !payload.revision?.startsWith("blueprint-library:") || revisions.get(payload.workspace_id) === payload.revision) return;
        revisions.set(payload.workspace_id, payload.revision);
        setRevision((value) => value + 1);
      }).then((off) => { if (active && current === binding) stop = off; else off(); }).catch(() => undefined);
    };
    bind();
    window.addEventListener("codeterm-peer-switch-rebind", bind);
    return () => {
      active = false;
      stop?.();
      window.removeEventListener("codeterm-peer-switch-rebind", bind);
    };
  }, [sources]);
  return revision;
}
