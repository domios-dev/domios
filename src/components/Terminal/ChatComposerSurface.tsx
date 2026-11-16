import { isReadOnlyTab } from "../../lib/tabInputPolicy";
import type { Tab } from "../../lib/types";
import type { ReactNode } from "react";
import { composerVisible, type ChatBodyView } from "./chatBodyVisibility";
import { ProviderUpdateBubble } from "./ProviderUpdateBubble";
import { LockedComposer } from "../billing/LockedComposer";
import { lockedView, type LockedView } from "../billing/freeLimitModel";
import { ComposerDock } from "./ComposerDock";

export function ChatComposerSurface({ view, children, providerId, tabId, tab, lock = lockedView("agent.tabs", tab) }: { view: ChatBodyView; children: ReactNode; providerId?: string; tabId?: string; tab?: Pick<Tab, "role_profile" | "locked"> | null; lock?: LockedView }) {
  if (isReadOnlyTab(tab)) return <div className="ct-chat-bottom-bar shrink-0">
    <ComposerDock mode="floating">
      <div role="status" className="ct-composer-card">
        <div className="ct-composer-row flex items-center justify-center px-3.5 text-center font-[family-name:var(--ct-font-ui)] text-[length:var(--ct-capsule-font-size)] leading-[var(--ct-capsule-line-height)] text-[var(--ct-muted-fg)]">
          Curator round, read-only
        </div>
      </div>
    </ComposerDock>
  </div>;
  if (lock.locked) return <LockedComposer view={lock} />;
  return composerVisible(view) ? <div data-composer-height-surface className="shrink-0">
    {providerId && <div className="ct-provider-update-slot"><ProviderUpdateBubble providerId={providerId} callerTabId={tabId} /></div>}
    <div role="group" aria-label="Message composer" inert={view.kind === "starting" || undefined} className="ct-chat-bottom-bar">{children}</div>
  </div> : null;
}
