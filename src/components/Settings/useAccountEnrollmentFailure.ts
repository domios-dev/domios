import { useEffect, useState } from "react";
import { type AccountMeshFailure, useAccountMeshClient } from "../MeshPanel/accountMeshClient";

export function useAccountEnrollmentFailure(identity: string | null): AccountMeshFailure | null {
  const client = useAccountMeshClient();
  const [loaded, setLoaded] = useState<{ identity: string; failure: AccountMeshFailure | null } | null>(null);
  useEffect(() => {
    if (!identity) return;
    let active = true;
    let generation = 0;
    const load = async () => {
      const current = ++generation;
      try {
        const snapshot = await client.snapshot();
        if (active && current === generation) {
          setLoaded({ identity, failure: snapshot.enrollment === "refused" ? snapshot.last_refusal : null });
        }
      } catch {
        if (active && current === generation) setLoaded({ identity, failure: null });
      }
    };
    const unsubscribe = client.subscribe(() => { void load(); });
    void load();
    return () => { active = false; unsubscribe(); };
  }, [client, identity]);
  return identity && loaded?.identity === identity ? loaded.failure : null;
}
