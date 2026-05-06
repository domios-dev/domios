import { useCallback, useEffect, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import { useMultiplexer } from "../../store/multiplexer";
import { TopBar } from "./TopBar";
import { BottomStrip } from "./BottomStrip";
import { NodeDetailPopup } from "./NodeDetailPopup";
import { SearchInspector } from "./SearchInspector";
import { useNodeDetail } from "./hooks/useNodeDetail";
import { useMapProgress } from "./hooks/useMapProgress";

interface Props {
  api: ApiConfig;
  workspaceId: string | undefined;
}

export function ProjectMapPanel({ api, workspaceId }: Props) {
  const { state } = useMultiplexer();
  const workspace = state.workspaces.find((w) => w.id === workspaceId);
  const workspaceName = workspace?.name ?? "(unknown)";

  const [focusId, setFocusId] = useState<number | null>(null);
  const [refreshTick, setRefreshTick] = useState(0);
  const { detail, error: detailError } = useNodeDetail(api, workspaceId, focusId);
  const progress = useMapProgress(api, workspaceId);

  const refresh = useCallback(() => setRefreshTick((t) => t + 1), []);

  useEffect(() => {
    setFocusId(null);
  }, [workspaceId]);

  if (!workspaceId) {
    return (
      <div className="absolute inset-0 flex items-center justify-center text-[var(--ct-muted)] text-sm bg-[var(--ct-bg)]">
        No workspace selected.
      </div>
    );
  }

  return (
    <div className="absolute inset-0 flex flex-col bg-[var(--ct-bg)] text-[var(--ct-fg)]">
      <TopBar workspaceName={workspaceName} onRefresh={refresh} progress={progress} />
      <div className="flex-1 min-w-0 relative">
        <SearchInspector
          key={refreshTick}
          api={api}
          workspaceId={workspaceId}
          onPick={setFocusId}
          focusId={focusId}
          progress={progress}
        />
        <NodeDetailPopup
          detail={detail}
          onClose={() => setFocusId(null)}
          onJump={setFocusId}
        />
      </div>
      <BottomStrip status={detailError ? "error" : "ready"} errorText={detailError} />
    </div>
  );
}
