import { useEffect, useRef } from "react";
import { listen } from "../lib/bridge";
import {
  openChimeContext,
  playArrivalChime,
  shouldSoundArrival,
  type ChimeContext,
} from "../lib/notificationSound";

/** The one arrival event, emitted by the daemon only for a level the bell admitted. */
const ARRIVAL_EVENT = "notification-created";

/**
 * Sound the chime on a bell arrival that says the agent stopped.
 *
 * The subscription is the whole trigger — nothing polls, nothing is scheduled,
 * and no other code path reaches the player. A level the user routed away from
 * the bell never produces this event, so it can never produce a sound.
 */
export function useNotificationSound(enabled: boolean): void {
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const contextRef = useRef<ChimeContext | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let dropped = false;
    void listen<{ level?: string | null }>(ARRIVAL_EVENT, (event) => {
      if (!shouldSoundArrival(enabledRef.current, event.payload?.level)) return;
      const context = contextRef.current ?? openChimeContext();
      if (!context) return;
      contextRef.current = context;
      playArrivalChime(context);
    }).then((off) => {
      if (dropped) off();
      else unlisten = off;
    });
    return () => {
      dropped = true;
      unlisten?.();
    };
  }, []);
}
