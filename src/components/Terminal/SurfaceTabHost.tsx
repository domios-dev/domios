import { SurfaceTabView } from "./SurfaceTabView";
import { useSurfaceDecl } from "../Surface/useSurfaceDecl";
import { workspaceIdOfTab } from "../../lib/surfaceTab";
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer";
import type { ApiConfig, SurfaceTabBacking, Tab } from "../../lib/types";

interface SurfaceTabHostProps {
  tab: Tab;
  backing: SurfaceTabBacking;
  workspaceId: string | null;
  api?: ApiConfig;
  isVisible: boolean;
}

/** Binds the frame to the mux and the daemon: the owner's live name, the surface declaration, focusing the owner, closing the tab. */
export function SurfaceTabHost({ tab, backing, workspaceId, api, isVisible }: SurfaceTabHostProps) {
  const { closeTab, selectTabByTab } = useMultiplexer();
  const ownerId = tab.parent_tab_id ?? null;
  const ownerName = useMuxSelector((state) => (ownerId ? state.tabs[ownerId]?.title ?? null : null));
  const ownerWorkspaceId = useMuxSelector((state) => workspaceIdOfTab(state.workspaces, ownerId));
  const tabWorkspaceId = useMuxSelector((state) => workspaceId ?? workspaceIdOfTab(state.workspaces, tab.id));
  const decl = useSurfaceDecl(api, backing.plugin_id, backing.surface_id);
  return (
    <SurfaceTabView
      tab={tab}
      backing={backing}
      ownerName={ownerName}
      onFocusOwner={ownerId && ownerWorkspaceId ? () => void selectTabByTab(ownerWorkspaceId, ownerId) : undefined}
      onClose={() => {
        if (tabWorkspaceId) closeTab(tabWorkspaceId, tab.id);
      }}
      api={api}
      isVisible={isVisible}
      decl={decl}
    />
  );
}
