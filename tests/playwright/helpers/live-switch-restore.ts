type PriorBinding = { session: string; nonce: string; pty: string };
export type BindingSnapshot = {
  agent_session_id?: string; session_binding_nonce?: string; reported_session_binding_nonce?: string; pty_id?: string;
  boot_stage?: string; structured_control_active?: boolean; lifecycle_epoch?: number;
  lifecycle?: { checkpoint?: { key?: { epoch?: number }; authority?: { session_id?: string; binding_nonce?: string };
    agent_type_write?: { origin?: string; nonce_verified?: boolean; session_id?: string } } | null;
    transitions?: Array<{ transition?: { transition?: string; session_id?: string; binding_nonce?: string } }> };
};

export function restoredBindingEvidence(snapshot: BindingSnapshot, prior: PriorBinding): string | undefined {
  const nonce = snapshot.session_binding_nonce;
  if (snapshot.boot_stage !== "agent_ready" || snapshot.agent_session_id !== prior.session || !nonce || nonce === prior.nonce ||
    snapshot.reported_session_binding_nonce !== nonce || !snapshot.pty_id || snapshot.pty_id === prior.pty) return;
  if (snapshot.lifecycle?.transitions?.some(({ transition }) => transition?.transition === "session_bound" &&
    transition.session_id === prior.session && transition.binding_nonce === nonce)) return "session_bound transition";
  const checkpoint = snapshot.lifecycle?.checkpoint;
  if (checkpoint?.key?.epoch === snapshot.lifecycle_epoch && checkpoint?.authority?.session_id === prior.session &&
    checkpoint.authority.binding_nonce === nonce && checkpoint.agent_type_write?.origin === "hook" &&
    checkpoint.agent_type_write.nonce_verified === true && checkpoint.agent_type_write.session_id === prior.session)
    return "checkpoint binding (hook-authenticated)";
  if (snapshot.structured_control_active === true) return "structured binding (reported nonce)";
}

export function isNewLaunchBound(snapshot: BindingSnapshot, prior: PriorBinding): boolean {
  return restoredBindingEvidence(snapshot, prior) !== undefined;
}

export function waitForNewLaunchBinding(read: () => Promise<BindingSnapshot>, prior: PriorBinding,
  subscribe: (wake: () => void) => () => void, timeoutMs: number): Promise<BindingSnapshot> {
  return new Promise((resolve, reject) => {
    const signal = AbortSignal.timeout(timeoutMs);
    let loading = false;
    let dirty = false;
    let finished = false;
    let lastSnapshot: BindingSnapshot | undefined;
    let unsubscribe = () => {};
    const finish = (error?: Error, snapshot?: BindingSnapshot) => {
      if (finished) return;
      finished = true;
      unsubscribe();
      signal.removeEventListener("abort", expired);
      if (error) reject(error);
      else resolve(snapshot!);
    };
    const expired = () => finish(new Error(`New own session_bound absent: ${JSON.stringify(lastSnapshot)}`));
    const inspect = async () => {
      dirty = true;
      if (loading || finished) return;
      loading = true;
      try {
        while (dirty && !finished) {
          dirty = false;
          lastSnapshot = await read();
          if (isNewLaunchBound(lastSnapshot, prior)) finish(undefined, lastSnapshot);
        }
      } catch (error) { finish(error instanceof Error ? error : new Error(String(error))); }
      finally { loading = false; }
    };
    unsubscribe = subscribe(() => { void inspect(); });
    signal.addEventListener("abort", expired, { once: true });
    void inspect();
  });
}
