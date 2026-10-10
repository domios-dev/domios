import { useEffect, useRef, useState } from "react";
import { AgentStateIndicator } from "../../../src/components/Terminal/AgentStateIndicator";
import { ChatComposer } from "../../../src/components/Terminal/ChatComposer";
import { TabStatusLineModelPicker } from "../../../src/components/Terminal/StatusLineModelPicker";
import { getAgentDescriptor } from "../../../src/lib/agentDescriptorCache";
import { tabActivity } from "../../../src/lib/tabActivity";
import type { Tab, TabStatus } from "../../../src/lib/types";
import { ProductProviders } from "./fixtures";

const noop = () => undefined;

export function typeInto(textarea: HTMLTextAreaElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
  setter?.call(textarea, value);
  textarea.dispatchEvent(new Event("input", { bubbles: true }));
}

export interface ChatBottomBarProps {
  tab: Tab;
  status?: TabStatus | null;
  draft?: string;
  attachments?: string[];
  showStatus?: boolean;
  /** Clicks the linked-kind button so its strip is open. */
  openKind?: "blueprints" | "crons" | "delayed";
  /** Loops: strip open, then a draft is typed and the strip folds away. */
  typingLoop?: boolean;
  narrow?: boolean;
  /** Phone variant: clicks the glass context pill so the header unfolds. */
  unfold?: boolean;
  workspaceId?: string;
}

/** TabSessionChat's bottom bar: context slot, status row, then the shipped ChatComposer. */
export function ChatBottomBar({ tab, status = null, draft, attachments = [], showStatus = true, openKind, typingLoop = false, narrow = false, unfold = false, workspaceId }: ChatBottomBarProps) {
  const root = useRef<HTMLDivElement>(null);
  const [contextSlot, setContextSlot] = useState<HTMLDivElement | null>(null);
  const activity = tabActivity(tab, status);
  const provider = getAgentDescriptor(tab.agent_type);

  useEffect(() => {
    let alive = true;
    let timer = 0;
    const query = <T extends Element>(selector: string) => root.current?.querySelector<T>(selector) ?? null;
    const whenReady = (selector: string, run: (element: HTMLElement) => void, attempts = 80) => {
      if (!alive) return;
      const element = query<HTMLElement>(selector);
      if (element) run(element);
      else if (attempts > 0 && alive) timer = window.setTimeout(() => whenReady(selector, run, attempts - 1), 50);
    };
    if (draft) whenReady('textarea[data-testid="chat-composer"]', (element) => typeInto(element as HTMLTextAreaElement, draft));
    if (unfold) whenReady('[data-testid="composer-context-pill"][aria-expanded="false"]', (pill) => pill.click());
    const kind = typingLoop ? "delayed" : openKind;
    if (kind) {
      whenReady(`.ct-ctx-kind[data-kind="${kind}"]`, (button) => {
        button.click();
        if (!typingLoop) return;
        let typed = false;
        const cycle = () => {
          if (!alive) return;
          const textarea = query<HTMLTextAreaElement>('textarea[data-testid="chat-composer"]');
          if (textarea) typeInto(textarea, typed ? "" : "Ship the gallery update");
          typed = !typed;
          if (!typed) query<HTMLElement>(`.ct-ctx-kind[data-kind="${kind}"]`)?.click();
          timer = window.setTimeout(cycle, 1600);
        };
        timer = window.setTimeout(cycle, 1600);
      });
    }
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [draft, openKind, typingLoop, unfold]);

  return <ProductProviders tabs={[tab]} workspaceId={workspaceId} plans>
    <div ref={root} className={narrow ? "gallery-fill gallery-fill--narrow" : "gallery-fill"}>
      <div data-composer-height-surface className="ct-chat-bottom-bar shrink-0">
        <div ref={setContextSlot} className="ct-ctx-slot" />
        {showStatus && <div className="ct-composer-status w-full">
          <AgentStateIndicator
            activity={activity}
            providerName={provider?.display_name}
            model={tab.agent_session_model}
            reasoningEffort={tab.reasoning_effort_identity}
            className="ct-composer-lane mx-auto w-full"
            onStop={noop}
            onCancel={noop}
            onOpenTerminal={noop}
            renderIdentity={(label) => <TabStatusLineModelPicker tab={tab} label={label} />}
          />
        </div>}
        <ChatComposer
          tabId={tab.id}
          tabProvider={tab.agent_type ?? null}
          currentPtyId={tab.pty_id}
          pendingPrompt={null}
          attachments={attachments}
          addAttachments={noop}
          removeAttachment={noop}
          clearAttachments={noop}
          handlePasteEvent={noop}
          onWillSendMessage={noop}
          addOptimistic={() => "optimistic"}
          updateOptimistic={noop}
          observeChatSendStatus={noop}
          showComposerError={noop}
          contextSlot={contextSlot}
          isActive={false}
        />
      </div>
    </div>
  </ProductProviders>;
}
