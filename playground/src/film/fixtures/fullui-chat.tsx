import "../../../../src/components/Usage/styles.css";
import "./fullui-daemon";
import { MessageBubble } from "@codeterm/chat-ui";
import { chatMessageAccessibleName } from "../../../../src/components/Terminal/chatMessageAccessibleName";
import { chatLayoutTokens } from "../../../../src/components/Terminal/chatLayoutTokens";
import { ChatMessageRow, type MessageRowHandlers } from "../../../../src/components/Terminal/ChatMessageRow";
import { TabTopBar, type HeaderTabMenu } from "../../../../src/components/Terminal/TabTopBar";
import { editFrontendDraft } from "../../../../src/lib/frontendUpdate/appState";
import { getAgentDescriptor, listAgentDescriptors } from "../../../../src/lib/agentDescriptorCache";
import type { Tab, TabStatus } from "../../../../src/lib/types";
import { ChatBottomBar } from "../../gallery/ChatBottomBar";
import { ProductProviders } from "../../gallery/fixtures";
import { Reveal } from "../engine";
import type { ChatCard } from "./chat";
import { FILM_WORKSPACE } from "./team";

const noop = () => undefined;

const HANDLERS: MessageRowHandlers = {
  clipFor: () => null,
  partsFor: () => [],
  selectedFor: () => false,
  cursorFor: () => false,
  joinNextFor: () => false,
  select: noop,
  rowMouseDown: noop,
  toggle: noop,
  range: noop,
  endRange: noop,
  quote: noop,
  ask: noop,
};

export type ChatRow = ChatCard & { at: number };

function headerMenu(tab: Tab, status: TabStatus): HeaderTabMenu {
  return {
    tab,
    context: { status, descriptor: getAgentDescriptor(tab.agent_type), descriptors: listAgentDescriptors(), activity: null, orchestrators: [] },
    onRename: noop,
    onClose: noop,
  };
}

/** TabContent's column for a chat tab: TabTopBar over TabSessionChat's message surface and bottom bar. */
export function OrchestratorPane({ t, tab, status, rows, draft }: { t: number; tab: Tab; status: TabStatus; rows: ChatRow[]; draft: string }) {
  // The composer seeds its text from the draft store on mount, so a keyed remount shows exactly this frame's prefix.
  editFrontendDraft(`tab-session-chat:${tab.id}`, draft);
  return <div data-tab-id={tab.id} className="w-full h-full relative group/Tab">
    <div className="flex h-full w-full flex-col">
      <ProductProviders tabs={[tab]} workspaceId={FILM_WORKSPACE.id}>
        <TabTopBar
          tabId={tab.id}
          shellTab={tab}
          cwd={tab.cwd ?? null}
          workspaceId={FILM_WORKSPACE.id}
          showSwitch
          usageSource={tab.agent_type ? { kind: "meter", provider: tab.agent_type, trackingProvider: tab.agent_type, borrowedAccount: false } : null}
          usageVisible
          usageModelId={tab.agent_session_model ?? null}
          tabMenu={headerMenu(tab, status)}
          isActive
        />
      </ProductProviders>
      <div className="relative flex min-h-0 flex-1 flex-col" style={{ background: "var(--ct-bg)" }}>
        <div className="min-h-0 flex-1">
          <div className="ct-chat-shell w-full h-full flex flex-col relative" data-testid="tab-session-chat" style={{ ...chatLayoutTokens().variables, background: "var(--ct-bg, #0d1117)", color: "var(--ct-fg, #c9d1d9)" }}>
            <div className="relative flex-1 min-h-0">
              <div className="ct-chat-surface absolute inset-0 overflow-hidden px-5 py-3 select-text" data-testid="tab-session-chat-messages" role="log" aria-label="Conversation">
                <div style={{ position: "absolute", left: 0, right: 0, bottom: 0, padding: "0 1.25rem 0.75rem" }}>
                  {rows.map(({ message, origin, at }) => (
                    <Reveal key={message.id} t={t} at={at} dur={0.35} dy={18}>
                      <ChatMessageRow msg={message} ariaLabel={chatMessageAccessibleName(message.type, message.content)} selected={false} joinNext={false} selecting={false} cursor={false} handlers={HANDLERS}>
                        <MessageBubble message={message} origin={origin} onOriginClick={noop} onOpenJobs={noop} onStopReminder={noop} />
                      </ChatMessageRow>
                    </Reveal>
                  ))}
                </div>
              </div>
            </div>
            <ChatBottomBar key={draft} tab={tab} status={status} workspaceId={FILM_WORKSPACE.id} />
          </div>
        </div>
      </div>
    </div>
  </div>;
}
