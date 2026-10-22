import type { MuxStateStore } from "../../store/multiplexer/selectorStore";
import type { Tab } from "../types";
const TAB_FIELDS: readonly (keyof Tab)[] = ["id", "title", "description", "role", "team_id", "workspace_id", "cwd", "agent_type", "tab_type", "remote_peer_id", "remote_peer_name"];
export function watchMentionState(store: Pick<MuxStateStore, "getState" | "subscribe">, changed: () => void): () => void {
  let previous = store.getState();
  return store.subscribe(() => {
    const next = store.getState();
    const ids = Object.keys(next.tabs);
    const sameTabs = ids.length === Object.keys(previous.tabs).length && ids.every((id) => previous.tabs[id]
      && TAB_FIELDS.every((field) => previous.tabs[id][field] === next.tabs[id][field]));
    const sameWorkspaces = next.workspaces.length === previous.workspaces.length && next.workspaces.every((ws, index) => {
      const before = previous.workspaces[index];
      return before.id === ws.id && before.name === ws.name && before.cwd === ws.cwd && before.tabs.length === ws.tabs.length && before.tabs.every((id, i) => ws.tabs[i] === id);
    });
    previous = next;
    if (!sameTabs || !sameWorkspaces) changed();
  });
}
