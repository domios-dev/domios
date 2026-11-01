import { useEffect, useState } from "react";
import { enrollmentRetryMessage } from "../MeshPanel/accountMachines";
import { type AccountMeshFailure, type AccountMeshSnapshot, useAccountMeshClient } from "../MeshPanel/accountMeshClient";

function useEnrollmentSnapshot(identity: string | null): AccountMeshSnapshot | null {
  const client = useAccountMeshClient();
  const [loaded, setLoaded] = useState<{ identity: string; snapshot: AccountMeshSnapshot | null } | null>(null);
  useEffect(() => {
    if (!identity) return;
    let active = true;
    let generation = 0;
    const load = async () => {
      const current = ++generation;
      try {
        const snapshot = await client.snapshot();
        if (active && current === generation) {
          setLoaded({ identity, snapshot });
        }
      } catch {
        if (active && current === generation) setLoaded({ identity, snapshot: null });
      }
    };
    const unsubscribe = client.subscribe(() => { void load(); });
    void load();
    window.addEventListener("focus", load);
    return () => { active = false; unsubscribe(); window.removeEventListener("focus", load); };
  }, [client, identity]);
  return identity && loaded?.identity === identity ? loaded.snapshot : null;
}

export function useAccountEnrollmentFailure(identity: string | null): AccountMeshFailure | null {
  const snapshot = useEnrollmentSnapshot(identity);
  return snapshot?.enrollment === "refused" ? snapshot.last_refusal : null;
}

export function useEnrollmentNotice(identity: string | null): string | null {
  const snapshot = useEnrollmentSnapshot(identity);
  return snapshot ? enrollmentRetryMessage(snapshot, Date.now() / 1000) : null;
}
