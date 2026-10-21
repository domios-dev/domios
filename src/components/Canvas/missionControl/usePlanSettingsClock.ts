import { useEffect, useState } from "react";

export function usePlanSettingsClock(fixedNow?: number) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (fixedNow !== undefined) return;
    const clock = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(clock);
  }, [fixedNow]);
  return fixedNow ?? now;
}
