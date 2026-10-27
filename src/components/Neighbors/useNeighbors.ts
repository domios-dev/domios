import { useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import { neighborClient, type NeighborClient } from "./neighborClient";
import { parseNeighbors, type NeighborCommand, type NeighborList } from "./neighborModel";
import type { ReferralClaim } from "./referralTrial";

interface LoadedNeighbors {
  identity: string | null;
  list: NeighborList | null;
  error: string | null;
  referral: ReferralClaim | null;
}

export function useNeighbors(identity: string | null, client: NeighborClient = neighborClient) {
  const [loaded, setLoaded] = useState<LoadedNeighbors>({ identity, list: null, error: null, referral: null });
  const [revision, setRevision] = useState(0);
  const [pending, setPending] = useState(false);
  const busy = useRef(false);
  const generation = useRef(0);
  const currentIdentity = useRef(identity);
  currentIdentity.current = identity;

  useEffect(() => {
    const epoch = ++generation.current;
    const controller = new AbortController();
    if (!identity) {
      setLoaded({ identity, list: null, error: null, referral: null });
      return () => controller.abort();
    }
    let latest = 0;
    let pushes = 0;
    async function load() {
      const request = ++latest;
      const push = pushes;
      let referral: ReferralClaim | null = null;
      let referralError: string | null = null;
      try {
        try { referral = await client.claimReferralTrial(controller.signal); }
        catch { referralError = "Your referral could not be checked. Refresh Neighbors to try again."; }
        if (controller.signal.aborted || generation.current !== epoch || request !== latest) return;
        const list = await client.list(controller.signal);
        if (controller.signal.aborted || generation.current !== epoch || request !== latest) return;
        setLoaded(previous => pushes === push ? { identity, list, referral, error: referralError }
          : { ...previous, referral, error: referralError });
      } catch (failure: unknown) {
        if (!controller.signal.aborted && generation.current === epoch && request === latest && pushes === push) {
          setLoaded(previous => ({ identity, list: previous.identity === identity && previous.list ? { ...previous.list, can_invite: false } : null, referral,
            error: failure instanceof Error ? failure.message : "Neighbors unavailable" }));
        }
      }
    }
    void load();
    window.addEventListener("focus", load);
    const unlisteners: (() => void)[] = [];
    const retain = (stop: () => void) => {
      if (controller.signal.aborted) stop();
      else unlisteners.push(stop);
    };
    void listen("entitlement-changed", load).then(retain);
    void listen<{ uid: string; list: unknown }>("neighbors-changed", event => {
      if (controller.signal.aborted || generation.current !== epoch || event.payload?.uid !== identity) return;
      let list: NeighborList;
      try { list = parseNeighbors(event.payload.list); }
      catch { return; }
      pushes++;
      setLoaded(previous => ({ identity, list, error: null, referral: previous.identity === identity ? previous.referral : null }));
    }).then(retain);
    return () => { controller.abort(); generation.current++; window.removeEventListener("focus", load); unlisteners.forEach(stop => stop()); };
  }, [identity, client, revision]);

  async function apply(command: Exclude<NeighborCommand, { operation: "list" }>): Promise<boolean> {
    if (busy.current || !identity) return false;
    busy.current = true;
    setPending(true);
    try {
      await client.apply(command);
      if (currentIdentity.current !== identity) return false;
      setRevision(value => value + 1);
      return true;
    } catch (failure: unknown) {
      if (currentIdentity.current === identity) setLoaded(previous => ({ ...previous,
        list: command.operation === "request" && previous.list ? { ...previous.list, can_invite: false } : previous.list,
        error: failure instanceof Error ? failure.message : "Neighbors request failed" }));
      return false;
    } finally {
      busy.current = false;
      setPending(false);
    }
  }

  return { list: loaded.identity === identity ? loaded.list : null,
    referral: loaded.identity === identity ? loaded.referral : null,
    error: loaded.identity === identity ? loaded.error : null, pending, apply,
    refresh: () => setRevision(value => value + 1) };
}
