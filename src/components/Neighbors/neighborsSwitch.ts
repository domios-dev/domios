import { useCallback, useEffect, useState } from "react";
import { localFetch } from "../../lib/webApi";

/** Neighbors is on unless the config says otherwise; a config without the field reads as on. */
export function neighborsEnabled(config: object | null | undefined): boolean {
  const neighbors = config && "neighbors" in config ? config.neighbors : undefined;
  return !(typeof neighbors === "object" && neighbors !== null && "enabled" in neighbors && neighbors.enabled === false);
}

export async function setNeighborsEnabled(enabled: boolean): Promise<boolean> {
  const response = await localFetch("/api/account/neighbors/enabled", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ enabled }),
  });
  if (!response.ok) throw new Error(await response.text());
  const value: unknown = await response.json();
  if (typeof value !== "object" || value === null || typeof (value as { enabled?: unknown }).enabled !== "boolean") throw new Error("Invalid Neighbors switch response");
  return (value as { enabled: boolean }).enabled;
}

/** The switch shows the requested state at once and settles on the next config the daemon publishes. */
export function useNeighborsSwitch(config: object | null | undefined) {
  const configured = neighborsEnabled(config);
  const [override, setOverride] = useState<boolean | null>(null);
  useEffect(() => { setOverride(null); }, [configured]);
  const enabled = override ?? configured;
  const setEnabled = useCallback(async (next: boolean) => {
    if (next === enabled) return;
    setOverride(next);
    try { setOverride(await setNeighborsEnabled(next)); }
    catch { setOverride(null); }
  }, [enabled]);
  return { enabled, setEnabled };
}
