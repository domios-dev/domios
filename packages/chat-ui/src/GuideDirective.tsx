import React, { createContext, useContext } from "react";
import { BookOpen } from "lucide-react";
import { ChatBlockFrame } from "./blocks/ChatBlockFrame";

interface GuideSection {
  id: string;
  title: string;
  subtitle?: string;
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
  const destination = section ?? navigation?.sections[0];
  const notice = section ? null : (
    <span data-testid="chat-guide-notice">
      {navigation ? "Guide section unavailable. Open the first section." : "Guide navigation is unavailable in this view."}
    </span>
  );
  return (
    <div data-testid="chat-ds-guide" data-guide-section={sectionId}>
      <ChatBlockFrame
        name="guide"
        source={`@ds-guide ${sectionId}`}
        title={destination?.title ?? "Guide"}
        icon={<BookOpen size={18} aria-hidden="true" />}
        summary={notice ?? destination?.subtitle}
        actions={[{
          id: "open",
          label: "Open guide",
          ariaLabel: destination ? `Open Guide: ${destination.title}` : "Open Guide",
          primary: true,
          disabled: !navigation || !destination,
          run: () => {
            if (destination) navigation?.onOpen(destination.id);
          },
        }]}
      />
    </div>
  );
}
