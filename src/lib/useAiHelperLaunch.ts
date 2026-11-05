import { useCallback, useContext, useState } from "react";
import { createNoticePublisher } from "./notices";
import { MultiplexerActionsContext } from "../store/multiplexer/context";

/**
 * The one way a "… with the General agent" button opens its helper: it shows Opening… while the helper tab starts
 * and raises a quiet toast if it cannot. `start` spawns the helper (see `spawnAiHelper`).
 */
export function useAiHelperLaunch(failureId: string, onOpened?: () => void) {
  const actions = useContext(MultiplexerActionsContext);
  const [launching, setLaunching] = useState(false);
  const launch = useCallback((start: () => Promise<unknown>) => {
    setLaunching(true);
    start().then(() => onOpened?.(), (error: unknown) => {
      if (actions) createNoticePublisher(actions)({ kind: "toast", id: failureId,
        title: "The General agent could not be opened",
        detail: error instanceof Error ? error.message : String(error),
        severity: "Warning", tabId: null, workspaceId: null });
    }).finally(() => setLaunching(false));
  }, [actions, failureId, onOpened]);
  return { launching, launch };
}
