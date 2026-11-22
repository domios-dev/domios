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
  "Install local live dictation automatically now (clicking Set up with General is my explicit consent to download the ~487 MB Parakeet TDT 0.6B v3 model and its local Python runtime — do not ask again or send me to Extensions). Skip `mem search`, `domios docs`, `--help`, `channel list/diff`, and `plugin get`; run only these exact commands (note: plugin verbs do not take `--json`):\n" +
  "1. `domios plugin channel refresh codeterm-plugins` then `domios plugin update transcriber` (if not installed or refused as a local install, run `domios plugin install transcriber --channel codeterm-plugins`).\n" +
  "2. `domios plugin transcriber dictation-status` — if `ready: true`, skip to step 4.\n" +
  "3. `domios plugin transcriber dictation-install parakeet-tdt-0.6b-v3-int8 --consent`, check `domios plugin transcriber dictation-progress` until completed, then `domios plugin transcriber dictation-select parakeet-tdt-0.6b-v3-int8` and `domios plugin transcriber dictation-test parakeet-tdt-0.6b-v3-int8`.\n" +
  "4. Reply in one short friendly line once ready (or the exact error if a step failed). Never touch voice-note settings or my draft.";

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

const UNCONFIRMED_RECEIPT_MESSAGE =
  "Setup delivery to General could not be confirmed; it may have reached General. Check General before retrying.";

function classifySendResult(result: unknown): HandoffOutcome {
  if (!result || typeof result !== "object" || Array.isArray(result)) {
    return { status: "uncertain", message: UNCONFIRMED_RECEIPT_MESSAGE };
  }
  const receipt = result as Partial<ChatSendResult> & { state?: unknown; error?: unknown };
  const detail = typeof receipt.error === "string" ? receipt.error.trim() : "";
  if (receipt.state === "held" || receipt.state === "written" || receipt.state === "delivered") {
    return { status: "accepted" };
  }
  if (receipt.state === "failed") {
    throw new Error(detail || "Could not deliver the setup request to General.");
  }
  if (receipt.state === "submit_unconfirmed") {
    return {
      status: "uncertain",
      message: detail
        ? `Setup delivery to General is unconfirmed (${detail}); check General before retrying.`
        : "Setup delivery to General is unconfirmed; it may have reached General. Check General before retrying.",
    };
  }
  return { status: "uncertain", message: UNCONFIRMED_RECEIPT_MESSAGE };
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
  let raw: Promise<unknown>;
  try {
    raw = Promise.resolve(invoke<ChatSendResult>("chat_send_text", {
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
        <p className="ct-dictation-bubble__body">{reason} General will install the plugin and local model automatically.</p>
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
