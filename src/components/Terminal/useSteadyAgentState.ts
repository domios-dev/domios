import { useEffect, useState } from "react";
import type { AgentState } from "./AgentStateIndicator";

export const WORKING_HOLD_MS = 1000;

/** Whether a working row should stay on screen while `incoming` settles. */
export function holdsWorkingRow(displayed: AgentState, incoming: AgentState): boolean {
  return displayed.state === "working" && incoming.state === "idle";
}

/** Display-only smoothing: a brief idle gap between working steps never blinks the row. */
export function useSteadyAgentState(incoming: AgentState, holdMs = WORKING_HOLD_MS): AgentState {
  const [displayed, setDisplayed] = useState(incoming);
  const holding = holdsWorkingRow(displayed, incoming);

  useEffect(() => {
    if (!holding) {
      setDisplayed(incoming);
      return;
    }
    const timer = setTimeout(() => setDisplayed(incoming), holdMs);
    return () => clearTimeout(timer);
  }, [holding, incoming, holdMs]);

  return holding ? displayed : incoming;
}
