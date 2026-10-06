import React, { createContext, useContext } from "react";
import { BookOpen } from "lucide-react";

interface GuideSection {
  id: string;
  title: string;
}

interface GuideNavigation {
  sections: readonly GuideSection[];
  onOpen: (sectionId: string) => void;
}

const GuideNavigationContext = createContext<GuideNavigation | null>(null);

export function GuideNavigationProvider({
  sections,
  onOpen,
  children,
}: GuideNavigation & { children: React.ReactNode }) {
  return (
    <GuideNavigationContext.Provider value={{ sections, onOpen }}>
      {children}
    </GuideNavigationContext.Provider>
  );
}

export function parseGuideDirective(source: string): string | null {
  return /^[\t ]*@ds-guide[\t ]+([a-z0-9][a-z0-9-]*)[\t ]*$/i.exec(source.replace(/\r/g, ""))?.[1] ?? null;
}

export function GuideDirective({ sectionId }: { sectionId: string }) {
  const navigation = useContext(GuideNavigationContext);
  const section = navigation?.sections.find((entry) => entry.id === sectionId);
  const fallback = navigation?.sections[0];
  const destination = section ?? fallback;
  const label = destination ? `Open Guide: ${destination.title}` : "Open Guide";
  return (
    <div className="my-2" data-testid="chat-ds-guide" data-guide-section={sectionId}>
      {!section && (
        <p className="mb-1 text-xs opacity-70" data-testid="chat-guide-notice">
          {navigation ? "Guide section unavailable. Open the first section." : "Guide navigation is unavailable in this view."}
        </p>
      )}
      <button
        type="button"
        disabled={!navigation || !destination}
        onClick={() => destination && navigation?.onOpen(destination.id)}
        className="inline-flex items-center gap-2 rounded-lg border px-3 py-2 text-sm hover:brightness-110 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2"
        style={{ borderColor: "var(--ct-border)", color: "var(--ct-accent)" }}
      >
        <BookOpen size={16} aria-hidden="true" />
        {label}
      </button>
    </div>
  );
}
