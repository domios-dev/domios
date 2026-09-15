import { useCallback, useState } from "react";
import { hostBridgeAvailable } from "../../lib/hostBridge";
import { fetchTelegramThreadState, type TelegramThreadMenuState } from "./telegramThreadActions";

export function useTelegramThreadMenu(tabId: string, enabled: boolean): {
  state: TelegramThreadMenuState | null;
  refresh: () => void;
} {
  const [state, setState] = useState<TelegramThreadMenuState | null>(null);
  const refresh = useCallback(() => {
    if (!enabled || !hostBridgeAvailable()) return;
    fetchTelegramThreadState(tabId).then(setState, () => setState(null));
  }, [enabled, tabId]);
  return { state: enabled ? state : null, refresh };
}
