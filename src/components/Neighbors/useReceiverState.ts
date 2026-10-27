import { useEffect, useState } from "react";
import { receiverState, type NeighborRuntimeClient, type ReceiverState } from "./neighborRuntimeClient";

/** Which machine receives Neighbor messages, kept current from status reads and receiver events. */
export function useReceiverState(client: NeighborRuntimeClient, active: boolean): ReceiverState | null {
  const [state, setState] = useState<ReceiverState | null>(null);
  useEffect(() => {
    if (!active) { setState(null); return; }
    const controller = new AbortController();
    let stop: (() => void) | undefined;
    void client.subscribe(event => {
      if (receiverState(event)) setState(event);
      else if ("receiver_unavailable" in event) setState(null);
    }).then(unlisten => { if (controller.signal.aborted) unlisten(); else stop = unlisten; });
    client.status(controller.signal).then(status => setState(receiverState(status) ? status : null), () => setState(null));
    return () => { controller.abort(); stop?.(); };
  }, [client, active]);
  return state;
}

export function receivingLine(state: ReceiverState): string {
  if (state.receiving_here) return "Receiving here";
  if (state.receiving_enabled) return `Receiving moved to ${state.receiver_machine_label ?? "another machine"}`;
  return state.revision === 0 ? "Receiver not chosen yet" : "Not receiving";
}
