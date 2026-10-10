import { setClientTabUiMode } from "../lib/clientTabUiMode";
import { activateReturnToChat, type ReturnTo } from "../lib/returnTo";
import { useMultiplexer, useMuxState } from "../store/multiplexer";
import { canvasFocusStore } from "../components/Canvas/canvasFocusStore";

export function useReturnToChat(): (origin: ReturnTo) => boolean {
  const state = useMuxState();
  const { setActiveTab, setFocusedTab } = useMultiplexer();
  return (origin) => activateReturnToChat(origin, {
    workspaces: state.workspaces,
    closePanels: () => { canvasFocusStore.close(); canvasFocusStore.closeWorkspace(); },
    setChatMode: (tabId) => setClientTabUiMode(tabId, "chat"),
    setActiveTab,
    setFocusedTab,
  });
}
