import { useEffect, useRef, useState } from "react";
import type { ApiConfig } from "../../../lib/types";
import { fetchProjectMapFocus } from "../api";
import type { ProjectMapEdge, ProjectMapNode } from "../types";

export interface NodeDetail {
  node: ProjectMapNode;
  incoming: Array<{ edge: ProjectMapEdge; other: ProjectMapNode }>;
  outgoing: Array<{ edge: ProjectMapEdge; other: ProjectMapNode }>;
}

export function useNodeDetail(
  api: ApiConfig,
  workspaceId: string | undefined,
  focusId: number | null,
) {
  const [detail, setDetail] = useState<NodeDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const tokenRef = useRef(0);

  useEffect(() => {
    if (!workspaceId || focusId == null) {
      setDetail(null);
      setError(null);
      return;
    }
    const token = ++tokenRef.current;
    const ctrl = new AbortController();
    fetchProjectMapFocus(api, workspaceId, focusId, 1, ctrl.signal)
      .then((payload) => {
        if (token !== tokenRef.current) return;
        const nodeMap = new Map<number, ProjectMapNode>();
        for (const n of payload.nodes) nodeMap.set(n.id, n);
        const center = nodeMap.get(focusId);
        if (!center) {
          setDetail(null);
          return;
        }
        const incoming: NodeDetail["incoming"] = [];
        const outgoing: NodeDetail["outgoing"] = [];
        for (const e of payload.edges) {
          if (e.target === focusId) {
            const other = nodeMap.get(e.source);
            if (other) incoming.push({ edge: e, other });
          } else if (e.source === focusId) {
            const other = nodeMap.get(e.target);
            if (other) outgoing.push({ edge: e, other });
          }
        }
        setDetail({ node: center, incoming, outgoing });
        setError(null);
      })
      .catch((err) => {
        if (ctrl.signal.aborted) return;
        if (token !== tokenRef.current) return;
        setError(err instanceof Error ? err.message : String(err));
      });
    return () => ctrl.abort();
  }, [api, workspaceId, focusId]);

  return { detail, error };
}
