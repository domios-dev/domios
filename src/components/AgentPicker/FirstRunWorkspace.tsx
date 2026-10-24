import { useEffect, useRef, type ReactNode } from "react";
import type { Tab } from "../../lib/types";
import type { FirstRunGate } from "../../lib/firstRunGate";
import { isFirstRunGeneralTab } from "./constants";

export function FirstRunWorkspace({ gate, tabs, settings, onGetStarted, onboarding, children }: {
  gate: FirstRunGate;
  tabs: Readonly<Record<string, Tab>>;
  settings?: ReactNode;
  onGetStarted: () => void;
  onboarding: ReactNode;
  children: ReactNode;
}) {
  const content = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!settings) content.current?.focus();
  }, [settings]);
  if (gate.allows("app")) return children;
  const otherTabs = Object.values(tabs).filter((tab) => !isFirstRunGeneralTab(tab) && !tab.hidden && tab.show_in_tabbar !== false);
  return <div className="flex flex-1 min-h-0" data-testid="first-run-workspace">
    <aside aria-label="Sidebar" className="w-56 shrink-0 border-r border-[var(--ct-border)] p-3 overflow-y-auto max-sm:hidden">
      <section aria-label="General">
        <h2 className="text-xs text-[var(--ct-muted-fg)] mb-3">GENERAL</h2>
        <button type="button" aria-label="Get started" aria-current={settings ? undefined : "page"} className="w-full text-left rounded-lg bg-[var(--ct-hover)] p-3" onClick={() => { onGetStarted(); content.current?.focus(); }}>
          <span className="block">Get started</span>
          <span className="block text-xs text-[var(--ct-muted-fg)]">Choose your General Agent</span>
        </button>
      </section>
      {otherTabs.length > 0 && <section aria-label="Existing tabs" className="mt-6">
        <h2 className="text-xs text-[var(--ct-muted-fg)] mb-3">EXISTING TABS</h2>
        <p className="text-xs text-[var(--ct-muted-fg)] mb-2">Available after Get started</p>
        {otherTabs.map((tab) => <button key={tab.id} type="button" aria-label={tab.title} disabled className="block w-full text-left p-2 opacity-50">
          {tab.title}
          {tab.description && <span className="block text-xs">{tab.description}</span>}
        </button>)}
      </section>}
    </aside>
    <main ref={content} tabIndex={-1} aria-label={settings ? "Settings" : "Get started"} className="flex-1 min-w-0 min-h-0 relative">
      {settings ?? onboarding}
    </main>
  </div>;
}
