import { useEffect, useRef, useState } from "react";
import type { ApiConfig } from "../../../lib/types";
import { authHeaders, baseUrl } from "../api";

export interface SearchHit {
  id: number;
  kind: string;
  display_name: string;
  serialized_name: string;
  path: string | null;
  start_line: number | null;
  signature: string | null;
}

interface State {
  hits: SearchHit[];
  loading: boolean;
  error: string | null;
}

const DEBOUNCE_MS = 200;

export function useMapSearch(
  api: ApiConfig,
  workspaceId: string | undefined,
  query: string,
  opts: { kind?: string; limit?: number; semanticOnly?: boolean } = {},
): State {
  const [state, setState] = useState<State>({ hits: [], loading: false, error: null });
  const reqIdRef = useRef(0);

  useEffect(() => {
    if (!workspaceId) {
      setState({ hits: [], loading: false, error: null });
      return;
    }
    const trimmed = query.trim();
    if (trimmed.length === 0) {
      setState({ hits: [], loading: false, error: null });
      return;
    }
    const id = ++reqIdRef.current;
    setState((s) => ({ ...s, loading: true, error: null }));
    const handle = window.setTimeout(async () => {
      try {
        const params = new URLSearchParams({
          workspace: workspaceId,
          q: trimmed,
          limit: String(opts.limit ?? 30),
        });
        if (opts.kind) params.set("kind", opts.kind);
        if (opts.semanticOnly) params.set("semantic_only", "true");
        const res = await fetch(`${baseUrl(api)}/api/map/search?${params}`, {
          headers: authHeaders(api),
        });
        if (!res.ok) throw new Error(`${res.status} ${await res.text()}`);
        const data: SearchHit[] = await res.json();
        if (id !== reqIdRef.current) return;
        setState({ hits: data, loading: false, error: null });
      } catch (err) {
        if (id !== reqIdRef.current) return;
        setState({
          hits: [],
          loading: false,
          error: err instanceof Error ? err.message : "search failed",
        });
      }
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [api, workspaceId, query, opts.kind, opts.limit, opts.semanticOnly]);

  return state;
}
