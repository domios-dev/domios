import { useEffect, useState } from "react";
import { activityList } from "./meshApi";
import type { ActivityEvent } from "./types";

export function useActivityLog(active: boolean) {
  const [events, setEvents] = useState<ActivityEvent[]>([]);
  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    let lastSeen: number | undefined = undefined;
    const tick = async () => {
      const latest = await activityList({ since: lastSeen, limit: 200 }).catch(() => [] as ActivityEvent[]);
      if (cancelled) return;
      if (latest.length > 0) {
        lastSeen = latest[0].occurred_at;
        setEvents(prev => [...latest, ...prev].slice(0, 500));
      }
    };
    tick();
    const handle = setInterval(tick, 5000);
    return () => { cancelled = true; clearInterval(handle); };
  }, [active]);
  return events;
}
