import type { ReactNode } from "react";
import { composerVisible, type ChatBodyView } from "./chatBodyVisibility";
import { ProviderUpdateBubble } from "./ProviderUpdateBubble";

export function ChatComposerSurface({ view, children, providerId, tabId }: { view: ChatBodyView; children: ReactNode; providerId?: string; tabId?: string }) {
  return composerVisible(view) ? <div data-composer-height-surface className="shrink-0">
    {providerId && <div className="ct-provider-update-slot"><ProviderUpdateBubble providerId={providerId} callerTabId={tabId} /></div>}
    <div role="group" aria-label="Message composer" className="ct-chat-bottom-bar">{children}</div>
  </div> : null;
}
