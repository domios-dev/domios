import { isReadOnlyTab } from "../../lib/tabInputPolicy";
import type { Tab } from "../../lib/types";
import type { ReactNode } from "react";
import { composerVisible, type ChatBodyView } from "./chatBodyVisibility";
import { ProviderUpdateBubble } from "./ProviderUpdateBubble";
import { LockedComposer } from "../billing/LockedComposer";
import { lockedView } from "../billing/freeLimitModel";

export function ChatComposerSurface({ view, children, providerId, tabId, tab }: { view: ChatBodyView; children: ReactNode; providerId?: string; tabId?: string; tab?: Pick<Tab, "role_profile" | "locked"> | null }) {
  if (isReadOnlyTab(tab)) return <div role="status" className="ct-chat-bottom-bar">Curator round, read-only</div>;
  const lock = lockedView("agent.tabs", tab);
  if (lock.locked) return <LockedComposer view={lock} />;
  return composerVisible(view) ? <div data-composer-height-surface className="shrink-0">
    {providerId && <div className="ct-provider-update-slot"><ProviderUpdateBubble providerId={providerId} callerTabId={tabId} /></div>}
    <div role="group" aria-label="Message composer" className="ct-chat-bottom-bar">{children}</div>
  </div> : null;
}
