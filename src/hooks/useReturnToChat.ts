import { setClientTabUiMode } from "../lib/clientTabUiMode";
import { activateReturnToChat, type ReturnTo } from "../lib/returnTo";
import { useMultiplexer } from "../store/multiplexer";
import { useMuxStateStore } from "../store/multiplexer/selectorStore";
import { canvasFocusStore } from "../components/Canvas/canvasFocusStore";

export function useReturnToChat(): (origin: ReturnTo) => boolean {
  const store = useMuxStateStore();
  const { setActiveTab, setFocusedTab } = useMultiplexer();
  return (origin) => activateReturnToChat(origin, {
    workspaces: store.getState().workspaces,
    closePanels: () => { canvasFocusStore.close(); canvasFocusStore.closeWorkspace(); },
    setChatMode: (tabId) => setClientTabUiMode(tabId, "chat"),
    setActiveTab,
    setFocusedTab,
  });
}
