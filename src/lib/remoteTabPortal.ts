import type { Tab } from "./types";

export interface RemotePeerTabs {
  peer_id: string;
  peer_name: string;
  tabs: Tab[];
}

const STORAGE_KEY = "codeterm.remote-tab-attachments.v1";

export function loadAttachedRemoteTabs(): Record<string, Tab> {
  try {
    const value = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") as Record<string, Tab>;
    return Object.fromEntries(Object.entries(value).filter(([id, tab]) => id === tab.id && id.startsWith("mesh:")));
  } catch {
    return {};
  }
}

export function saveAttachedRemoteTabs(tabs: Record<string, Tab>): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(tabs));
  } catch {
    return;
  }
}

export function projectAttachedRemoteTab(
  source: Tab,
  peer: Pick<RemotePeerTabs, "peer_id" | "peer_name">,
  workspaceId: string,
): Tab {
  const ordinaryId = `mesh:${peer.peer_id}/${source.id}`;
  return {
    ...source,
    id: ordinaryId,
    workspace_id: workspaceId,
    pty_id: source.pty_id ? ordinaryId : null,
    remote_peer_id: peer.peer_id,
    remote_peer_name: peer.peer_name,
    remote_tab_id: source.id,
    remote_availability: "available",
    ownership: undefined,
    parent_tab_id: null,
    spawned_by_tab_id: null,
    team_id: null,
  };
}
