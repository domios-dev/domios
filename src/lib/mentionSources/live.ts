import type { MuxStateStore } from "../../store/multiplexer/selectorStore";
import type { Tab } from "../types";
const TAB_FIELDS: readonly (keyof Tab)[] = ["id", "title", "description", "role", "team_id", "workspace_id", "cwd", "agent_type", "tab_type", "backing", "remote_peer_id", "remote_peer_name"];
export function watchMentionState(store: Pick<MuxStateStore, "getState" | "subscribe">, changed: () => void, activity = false): () => void {
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
    const sameActivity = !activity || (next.tabStatuses === previous.tabStatuses && ids.every((id) => {
      const tab = next.tabs[id], before = previous.tabs[id];
      return before && tab.agent_state === before.agent_state && tab.pending_prompt === before.pending_prompt
        && tab.boot_stage === before.boot_stage && tab.agent_booting === before.agent_booting
        && tab.agent_session_id === before.agent_session_id && tab.session_binding_nonce === before.session_binding_nonce
        && tab.reported_session_binding_nonce === before.reported_session_binding_nonce && tab.frozen === before.frozen
        && tab.agent_runtime_state === before.agent_runtime_state && tab.provider_signals === before.provider_signals;
    }));
    previous = next;
    if (!sameTabs || !sameWorkspaces || !sameActivity) changed();
  });
}
