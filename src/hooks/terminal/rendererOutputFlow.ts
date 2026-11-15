import type { NativeOutputFlowUpdate } from "../../lib/nativeBridge";

export function rendererFlowEligible(tab: { agent_booting?: boolean; agent_type?: string | null; boot_stage?: string | null } | undefined): boolean {
  if (!tab || tab.agent_booting) return false;
  if (tab.agent_type) return tab.boot_stage === "agent_ready";
  return tab.boot_stage == null || ["shell_ready", "pty_reattached", "agent_ready"].includes(tab.boot_stage);
}

export function createRendererOutputFlow(
  tabId: string,
  ptyId: string,
  attachment: string,
  send: (update: NativeOutputFlowUpdate) => Promise<void>,
) {
  let enabled = true;
  let attached = false;
  let sending = false;
  let failed = false;
  let release = false;
  let latest: number | undefined;
  let pending: number | undefined;

  const drain = async () => {
    if (sending || failed) return;
    sending = true;
    try {
      while (release || pending !== undefined) {
        if (release) {
          release = false;
          attached = false;
          await send({ action: "detach", pty_id: ptyId, attachment });
        } else {
          const offset = pending!;
          pending = undefined;
          const attach = !attached;
          attached = true;
          await send({ action: "rendered", tab_id: tabId, pty_id: ptyId, attachment, offset, attach });
        }
      }
    } catch {
      failed = true;
      pending = undefined;
    } finally {
      sending = false;
    }
  };

  return {
    rendered(offset: number) {
      if (!Number.isSafeInteger(offset) || offset < 0) return;
      latest = Math.max(latest ?? 0, offset);
      if (!enabled) return;
      pending = latest;
      void drain();
    },
    suspend() {
      enabled = false;
      pending = undefined;
      release = true;
      void drain();
    },
    resume() {
      enabled = true;
      pending = latest;
      void drain();
    },
  };
}
