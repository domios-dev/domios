import { surfaceTransportTarget } from "../Surface/surfaceTransportTarget";
import { relayTarget } from "../../lib/webApi";
import { useState } from "react";
import { SurfaceTabView } from "./SurfaceTabView";
import { useSurfaceDecl } from "../Surface/useSurfaceDecl";
import { workspaceIdOfTab } from "../../lib/surfaceTab";
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer";
import type { ApiConfig, SurfaceTabBacking, Tab } from "../../lib/types";

interface SurfaceTabHostProps {
  tab: Tab;
  backing: SurfaceTabBacking;
  api?: ApiConfig;
  isVisible: boolean;
}

/** Binds the frame to the mux and the daemon: the owner's live name, the surface declaration, focusing the owner. */
export function SurfaceTabHost({ tab, backing, api, isVisible }: SurfaceTabHostProps) {
  const { selectTabByTab } = useMultiplexer();
  const ownerId = tab.parent_tab_id ?? null;
  const ownerTab = useMuxSelector((state) => (ownerId ? state.tabs[ownerId] ?? null : null));
  const ownerName = ownerTab?.title ?? null;
  const ownerWorkspaceId = useMuxSelector((state) => workspaceIdOfTab(state.workspaces, ownerId));
  const [refreshGeneration, setRefreshGeneration] = useState(0);
  const { peerId } = surfaceTransportTarget(tab, relayTarget()?.id ?? null);
  const decl = useSurfaceDecl(api, backing.plugin_id, backing.surface_id, refreshGeneration, peerId ?? null);
  return (
    <SurfaceTabView
      tab={tab}
      backing={backing}
      ownerName={ownerName}
      ownerTab={ownerTab}
      onFocusOwner={ownerId && ownerWorkspaceId ? () => void selectTabByTab(ownerWorkspaceId, ownerId) : undefined}
      api={api}
      isVisible={isVisible}
      decl={decl}
      onRetry={() => setRefreshGeneration((generation) => generation + 1)}
    />
  );
}
