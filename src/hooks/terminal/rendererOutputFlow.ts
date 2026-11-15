import type { NativeOutputFlowUpdate } from "../../lib/nativeBridge";

export function rendererFlowEligible(tab: { agent_booting?: boolean; agent_type?: string | null; boot_stage?: string | null } | undefined): boolean {
  if (!tab || tab.agent_booting) return false;
  if (tab.agent_type) return tab.boot_stage === "agent_ready";
  return tab.boot_stage == null || ["shell_ready", "pty_reattached", "agent_ready"].includes(tab.boot_stage);
}

export const OUTPUT_ACK_BYTES = 32 * 1024;

export function createRendererOutputFlow(
  tabId: string,
  ptyId: string,
  attachment: string,
  send: (update: NativeOutputFlowUpdate) => Promise<boolean>,
) {
  let enabled = true;
  let attached = false;
  let sending = false;
  let release = false;
  let revision = 0;
  let acknowledged: number | undefined;
  let attempted: number | undefined;
  let latest: number | undefined;
  let pending: number | undefined;
  const detach = (): NativeOutputFlowUpdate => ({ action: "detach", pty_id: ptyId, attachment });

  const drain = async () => {
    if (sending) return;
    sending = true;
    let releasing = false;
    try {
      while (release || pending !== undefined) {
        if (release) {
          releasing = true;
          release = false;
          attached = false;
          acknowledged = undefined;
          await send(detach());
          releasing = false;
        } else {
          const watermark = attached ? acknowledged : attempted;
          if (watermark !== undefined && pending! - watermark < OUTPUT_ACK_BYTES) break;
          const offset = pending!;
          attempted = offset;
          pending = undefined;
          const current = revision;
          const accepted = await send({ action: "rendered", tab_id: tabId, pty_id: ptyId, attachment, offset, attach: true });
          if (current === revision) {
            attached = accepted;
            acknowledged = accepted ? offset : undefined;
          }
        }
      }
    } catch {
      attached = false;
      acknowledged = undefined;
      pending = undefined;
      release = true;
      if (!releasing) {
        try { await send(detach()); release = false; } catch { release = true; }
      }
    } finally {
      sending = false;
    }
  };

  const suspend = () => {
    revision++;
    attempted = undefined;
    enabled = false;
    attached = false;
    acknowledged = undefined;
    pending = undefined;
    release = true;
    void drain();
  };
  return {
    rendered(offset: number) {
      if (!Number.isSafeInteger(offset) || offset < 0) return;
      latest = Math.max(latest ?? 0, offset);
      if (!enabled) { if (release) void drain(); return; }
      pending = latest;
      void drain();
    },
    suspend,
    refresh() {
      revision++;
      attached = false;
      acknowledged = undefined;
      attempted = undefined;
      pending = enabled ? latest : undefined;
      void drain();
    },
    reset() {
      latest = undefined;
      suspend();
    },
    resume() {
      if (!enabled) {
        enabled = true;
        attempted = undefined;
        attached = false;
        acknowledged = undefined;
      }
      pending = latest;
      void drain();
    },
  };
}
