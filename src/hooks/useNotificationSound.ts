import { useEffect, useRef } from "react";
import { listen } from "../lib/bridge";
import {
  chimeContext,
  establishChimeOnFirstGesture,
  openChimeContext,
  playArrivalChime,
  shouldSoundArrival,
  type ChimeContext,
  type GestureTarget,
} from "../lib/notificationSound";

/** The one arrival event, emitted by the daemon only for a level the bell admitted. */
const ARRIVAL_EVENT = "notification-created";

/** The app's own window, or nothing when there is no DOM to listen on. */
function gestureTarget(): GestureTarget | null {
  const host = globalThis as unknown as { window?: GestureTarget };
  return host.window ?? null;
}

/**
 * Sound the chime on a bell arrival that says the agent stopped.
 *
 * The subscription is the whole trigger — nothing polls, nothing is scheduled,
 * and no other code path reaches the player. A level the user routed away from
 * the bell never produces this event, so it can never produce a sound.
 *
 * The player reuses the context a user gesture established. Enabling the sound
 * is one such gesture; so is the first click or key anywhere in the app, which
 * is the only thing that can establish it when the opt-in came from stored
 * config. Nothing is opened while the sound is off.
 */
export function useNotificationSound(enabled: boolean): void {
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const contextRef = useRef<ChimeContext | null>(null);

  useEffect(() => {
    if (!enabled || chimeContext()) return;
    const target = gestureTarget();
    if (!target) return;
    return establishChimeOnFirstGesture(target);
  }, [enabled]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let dropped = false;
    void listen<{ level?: string | null }>(ARRIVAL_EVENT, (event) => {
      if (!shouldSoundArrival(enabledRef.current, event.payload?.level)) return;
      const context = contextRef.current ?? chimeContext() ?? openChimeContext();
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
