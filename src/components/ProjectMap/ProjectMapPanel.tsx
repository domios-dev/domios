import { useCallback, useMemo, useRef, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import { useMultiplexer } from "../../store/multiplexer";
import { TopBar } from "./TopBar";
import { LeftRail } from "./LeftRail";
import { RightRail } from "./RightRail";
import { BottomStrip } from "./BottomStrip";
import { ProjectMapCanvas } from "./ProjectMapCanvas";
import { useProjectMapGraph } from "./hooks/useProjectMapGraph";
import { useFocusMode } from "./hooks/useFocusMode";
import { useNodeDetail } from "./hooks/useNodeDetail";
import type { ProjectMapNode } from "./types";

interface Props {
  api: ApiConfig;
  workspaceId: string | undefined;
}

export function ProjectMapPanel({ api, workspaceId }: Props) {
  const { state } = useMultiplexer();
  const workspace = useMemo(
    () => state.workspaces.find((w) => w.id === workspaceId),
    [state.workspaces, workspaceId],
  );
  const workspaceName = workspace?.name ?? "(unknown)";

  const { payload, stats, truncated, error, loading, refetch } = useProjectMapGraph(api, workspaceId);
  const focus = useFocusMode(null, { enableShortcuts: true });
  const { detail } = useNodeDetail(api, workspaceId, focus.focusId);

  const [visibleKinds, setVisibleKinds] = useState<Set<string>>(new Set());
  const toggleKind = useCallback((kind: string) => {
    setVisibleKinds((prev) => {
      const next = new Set(prev);
      if (next.has(kind)) next.delete(kind);
      else next.add(kind);
      return next;
    });
  }, []);

  const [bookmarks, setBookmarks] = useState<number[]>([]);
  const addBookmark = useCallback(() => {
    if (focus.focusId == null) return;
    setBookmarks((prev) => (prev.includes(focus.focusId!) ? prev : [...prev, focus.focusId!]));
  }, [focus.focusId]);
  const removeBookmark = useCallback((id: number) => {
    setBookmarks((prev) => prev.filter((b) => b !== id));
  }, []);

  const nodeMap = useMemo(() => {
    const m = new Map<number, ProjectMapNode>();
    for (const n of payload?.nodes ?? []) m.set(n.id, n);
    return m;
  }, [payload]);

  const bookmarkLabel = useCallback(
    (id: number) => nodeMap.get(id)?.display_name ?? `#${id}`,
    [nodeMap],
  );

  const fitFnRef = useRef<(() => void) | null>(null);
  const registerFit = useCallback((fn: () => void) => {
    fitFnRef.current = fn;
  }, []);

  if (!workspaceId) {
    return (
      <div className="absolute inset-0 flex items-center justify-center text-[var(--ct-muted)] text-sm bg-[var(--ct-bg)]">
        No workspace selected.
      </div>
    );
  }

  const status: "ready" | "loading" | "error" =
    error ? "error" : loading ? "loading" : "ready";

  return (
    <div className="absolute inset-0 flex flex-col bg-[var(--ct-bg)] text-[var(--ct-fg)]">
      <TopBar
        workspaceName={workspaceName}
        layout="force"
        onLayoutChange={() => {}}
        onRefresh={refetch}
        loading={loading}
      />
      <div className="flex-1 min-h-0 flex">
        <LeftRail
          visibleKinds={visibleKinds}
          toggleKind={toggleKind}
          back={focus.back}
          forward={focus.forward}
          canBack={focus.canBack}
          canForward={focus.canForward}
          bookmarks={bookmarks}
          addBookmark={addBookmark}
          removeBookmark={removeBookmark}
          jumpToBookmark={focus.setFocus}
          bookmarkLabel={bookmarkLabel}
        />
        <div className="flex-1 min-w-0 relative">
          {(payload?.nodes.length ?? 0) === 0 && !loading && !error && (
            <div className="absolute inset-0 flex items-center justify-center text-[var(--ct-muted)] text-xs">
              Empty graph. Run <code className="font-mono">codeterm map index</code> to populate.
            </div>
          )}
          {payload && payload.nodes.length > 0 && (
            <ProjectMapCanvas
              nodes={payload.nodes}
              edges={payload.edges}
              visibleKinds={visibleKinds}
              focusId={focus.focusId}
              onFocus={focus.setFocus}
              registerFit={registerFit}
            />
          )}
          {truncated && (
            <div className="absolute top-2 left-1/2 -translate-x-1/2 px-3 py-1 rounded-sm bg-[var(--ct-yellow)]/15 border border-[var(--ct-yellow)]/40 text-[10px] text-[var(--ct-yellow)]">
              Graph too large; showing partial result. Pick a starting node to focus.
            </div>
          )}
        </div>
        <RightRail detail={detail} onJump={focus.setFocus} />
      </div>
      <BottomStrip
        status={status}
        errorText={error}
        nodeCount={stats?.nodes ?? payload?.nodes.length}
        edgeCount={stats?.edges ?? payload?.edges.length}
        truncated={truncated}
      />
    </div>
  );
}
