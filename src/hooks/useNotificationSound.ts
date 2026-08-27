import { useEffect, useRef } from "react";
import { listen } from "../lib/bridge";
import {
  chimeContext,
  chimeIsAudible,
  establishChimeOnFirstGesture,
  playArrivalChime,
  shouldSoundArrival,
  type GestureTarget,
} from "../lib/notificationSound";
import type { AgentLevelChoice } from "../lib/types";

/** The one arrival event. The daemon emits it for every record it keeps. */
const ARRIVAL_EVENT = "notification-created";

/** The app's own window, or nothing when there is no DOM to listen on. */
function gestureTarget(): GestureTarget | null {
  const host = globalThis as unknown as { window?: GestureTarget };
  return host.window ?? null;
}

/**
 * Sound the chime on an arrival the arriving level's own row says to sound.
 *
 * The subscription is the whole trigger — nothing polls, nothing is scheduled,
 * and no other code path reaches the player. Which arrivals sound is the level's
 * stored answer; the old hardcoded table is only what an unanswered row falls
 * back to.
 *
 * The player uses the context a USER GESTURE established, and nothing else.
 * Opening one here instead would produce a context the browser starts suspended
 * — silence — and cache it ahead of the running one a later gesture establishes,
 * which is permanent silence rather than a missed first chime. An arrival before
 * any gesture is therefore silent and leaves no trace; the listener is still
 * armed, so the next arrival after a gesture is heard.
 */
export function useNotificationSound(
  enabled: boolean,
  levels: readonly AgentLevelChoice[],
): void {
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const levelsRef = useRef(levels);
  levelsRef.current = levels;

  useEffect(() => {
    if (!enabled || chimeIsAudible()) return;
    const target = gestureTarget();
    if (!target) return;
    return establishChimeOnFirstGesture(target);
  }, [enabled]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let dropped = false;
    void listen<{ level?: string | null }>(ARRIVAL_EVENT, (event) => {
      if (!shouldSoundArrival(enabledRef.current, levelsRef.current, event.payload?.level)) return;
      const context = chimeContext();
      if (!context) return;
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
