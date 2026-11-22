import { useEffect, useRef, useState, type RefObject } from "react";
import { X } from "lucide-react";
import type { ChatSendResult } from "../../../../gen/ChatSendResult";
import { AnchoredPopover } from "../../shared/AnchoredPopover";
import { Button } from "../../shared/Button";
import { invoke } from "../../../lib/bridge";
import { focusTab } from "../../../lib/generalAgent";
import type { Tab } from "../../../lib/types";
import type { ExtendedState } from "../../../store/multiplexer/tree";
import { useMuxStateStore } from "../../../store/multiplexer/selectorStore";

const DICTATION_SETUP_REQUEST =
  "Help me set up local live composer dictation in Domios. Use the official Transcriber plugin and its Russian-capable Parakeet default. Explain the model download size, permissions, progress, and any failure. Use the plugin's supported installation and configuration controls. Keep the existing voice-note transcription settings and my draft intact.";

interface GeneralAuthority {
  tabId: string;
  workspaceId: string;
  ptyId: string | null;
  lifecycleEpoch: number | null;
  bindingNonce: string | null;
  sessionId: string | null;
}

type HandoffOutcome =
  | { status: "accepted" }
  | { status: "uncertain"; message: string };

interface InFlightHandoff {
  authority: GeneralAuthority;
  promise: Promise<HandoffOutcome>;
  focused: boolean;
  cancelled: boolean;
}

const activeHandoffs = new Set<InFlightHandoff>();

export function clearDictationSetupHandoff() {
  for (const handoff of activeHandoffs) handoff.cancelled = true;
  activeHandoffs.clear();
}

function isLocalGeneralTab(
  tab: Tab | undefined,
  tabId: string,
  workspaceId: string,
  pendingCloses: readonly string[] | undefined,
): tab is Tab {
  if (!tab?.protected) return false;
  if (tabId.startsWith("mesh:") || tab.remote_peer_id || tab.remote_tab_id) return false;
  if (tab.id !== undefined && tab.id !== tabId) return false;
  if (tab.workspace_id && tab.workspace_id !== workspaceId) return false;
  if (pendingCloses?.includes(tabId)) return false;
  return true;
}

function localGeneralAuthority(state: ExtendedState): GeneralAuthority | null {
  for (const workspace of state.workspaces) {
    if (!workspace.protected || workspace.id.startsWith("mesh:")) continue;
    const tabId = workspace.tabs.find(id => isLocalGeneralTab(state.tabs[id], id, workspace.id, state.pendingCloses));
    if (!tabId) continue;
    const tab = state.tabs[tabId];
    return {
      tabId,
      workspaceId: workspace.id,
      ptyId: tab.pty_id ?? null,
      lifecycleEpoch: tab.lifecycle_epoch ?? null,
      bindingNonce: tab.session_binding_nonce ?? null,
      sessionId: tab.agent_session_id ?? null,
    };
  }
  return null;
}

function sameGeneralAuthority(origin: GeneralAuthority, current: GeneralAuthority | null): current is GeneralAuthority {
  if (!current) return false;
  if (origin.workspaceId !== current.workspaceId || origin.tabId !== current.tabId) return false;
  if (origin.lifecycleEpoch !== current.lifecycleEpoch) return false;
  if (origin.ptyId && current.ptyId !== origin.ptyId) return false;
  if (origin.bindingNonce && current.bindingNonce !== origin.bindingNonce) return false;
  if (origin.sessionId && current.sessionId !== origin.sessionId) return false;
  return true;
}

function classifySendResult(result: ChatSendResult | void | null): HandoffOutcome {
  if (!result || typeof result !== "object" || !("state" in result)) return { status: "accepted" };
  if (result.state === "failed") {
    throw new Error(result.error?.trim() || "Could not deliver the setup request to General.");
  }
  if (result.state === "submit_unconfirmed") {
    const detail = result.error?.trim();
    return {
      status: "uncertain",
      message: detail
        ? `Setup delivery to General is unconfirmed (${detail}); check General before retrying.`
        : "Setup delivery to General is unconfirmed; it may have reached General. Check General before retrying.",
    };
  }
  return { status: "accepted" };
}

function requestGeneralSetup(authority: GeneralAuthority): InFlightHandoff {
  for (const existing of activeHandoffs) {
    if (!existing.cancelled && sameGeneralAuthority(existing.authority, authority)) return existing;
  }
  const entry: InFlightHandoff = {
    authority,
    focused: false,
    cancelled: false,
    promise: Promise.resolve({ status: "accepted" }),
  };
  let raw: Promise<ChatSendResult | void>;
  try {
    raw = Promise.resolve(invoke<ChatSendResult | void>("chat_send_text", {
      tabId: authority.tabId,
      text: DICTATION_SETUP_REQUEST,
    }));
  } catch (failure) {
    raw = Promise.reject(failure);
  }
  entry.promise = raw
    .then(classifySendResult)
    .finally(() => {
      activeHandoffs.delete(entry);
    });
  activeHandoffs.add(entry);
  return entry;
}

export function DictationSetupPrompt({
  reason,
  onDismiss,
  anchor,
}: {
  reason: string;
  onDismiss(): void;
  anchor: RefObject<HTMLElement | null>;
}) {
  const store = useMuxStateStore();
  const mounted = useRef(true);
  const onDismissRef = useRef(onDismiss);
  onDismissRef.current = onDismiss;
  const issued = useRef(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  async function install() {
    if (issued.current) return;
    issued.current = true;
    setPending(true);
    setError(null);
    try {
      const authority = localGeneralAuthority(store.getState());
      if (!authority) throw new Error("The General Agent is not running. Open General and try again.");
      const handoff = requestGeneralSetup(authority);
      const outcome = await handoff.promise;
      if (!mounted.current || handoff.cancelled) return;
      if (outcome.status === "uncertain") {
        setError(outcome.message);
        return;
      }
      const current = localGeneralAuthority(store.getState());
      if (!sameGeneralAuthority(handoff.authority, current)) {
        setError("The General Agent changed before setup could open. Try again to ask the current General.");
        return;
      }
      if (!handoff.focused) {
        handoff.focused = true;
        focusTab(current.tabId, current.workspaceId);
      }
      onDismissRef.current();
    } catch (failure) {
      if (!mounted.current) return;
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      issued.current = false;
      if (mounted.current) setPending(false);
    }
  }

  return (
    <AnchoredPopover open anchor={anchor} placement="top-end" gap={12} onClose={pending ? undefined : onDismiss}>
      <div
        className="ct-dictation-bubble ct-dictation-setup"
        data-testid="dictation-setup-bubble"
        role="group"
        aria-label="Install dictation"
      >
        <div className="ct-dictation-bubble__head">
          <strong>Install dictation?</strong>
          <button
            type="button"
            className="ct-dictation-bubble__close"
            aria-label="Dismiss dictation setup"
            title="Dismiss"
            disabled={pending}
            onClick={onDismiss}
          >
            <X size={14} strokeWidth={2.2} aria-hidden="true" />
          </button>
        </div>
        <p className="ct-dictation-bubble__body">{reason} General can guide the local model download.</p>
        <div className="ct-dictation-bubble__actions">
          <Button type="button" variant="primary" size="sm" loading={pending} disabled={pending} onClick={() => void install()}>
            Set up with General
          </Button>
        </div>
        {error && <div role="alert" className="ct-dictation-bubble__alert">{error}</div>}
      </div>
    </AnchoredPopover>
  );
}
