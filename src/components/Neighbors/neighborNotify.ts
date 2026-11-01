import { useEffect, useRef, useState } from "react";
import type { NeighborConciergeClient, NeighborNotify } from "./neighborConciergeClient";

export const NOTIFY_CHOICES: { value: NeighborNotify; label: string }[] = [
  { value: "off", label: "Off" },
  { value: "in_app", label: "In Domios" },
  { value: "in_app_and_bridges", label: "In Domios and your bridges" },
];

/**
 * Where a friend's messages reach you: the list value, kept current by change events, set optimistically.
 * Only the newest choice's answer lands, so replies that complete out of order cannot undo it.
 */
export function useNeighborNotify(client: NeighborConciergeClient, friendship: string, listed: NeighborNotify | undefined,
  onError: (message: string) => void): [NeighborNotify, (next: NeighborNotify) => void] {
  const [notify, setNotify] = useState<NeighborNotify>(listed ?? "in_app");
  const latest = useRef(0);
  useEffect(() => { if (listed) setNotify(listed); }, [listed]);
  useEffect(() => {
    let stop: (() => void) | undefined;
    let live = true;
    void client.onNotifyChanged((changed, value) => { if (changed === friendship) setNotify(value); })
      .then(unlisten => { if (live) stop = unlisten; else unlisten(); });
    return () => { live = false; stop?.(); };
  }, [client, friendship]);
  const change = (next: NeighborNotify) => {
    const previous = notify;
    const choice = ++latest.current;
    setNotify(next);
    void client.setNotify(friendship, next).then(saved => { if (choice === latest.current) setNotify(saved); }, (failure: unknown) => {
      if (choice !== latest.current) return;
      setNotify(previous);
      onError(failure instanceof Error ? failure.message : "Notifications could not be changed.");
    });
  };
  return [notify, change];
}
