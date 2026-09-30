import { useCallback, useEffect, useRef, useState } from "react";
import {
  BookOpen,
  Bot,
  Brain,
  GitBranch,
  Layers,
  Lightbulb,
  Network,
  Plug,
  Sparkles,
  type LucideIcon,
} from "lucide-react";
import type { AppConfig } from "../lib/types";
import { getHelpContent } from "./content";
import { HelpBlockView } from "./HelpBlockView";
import type { HelpSectionContent, HelpSectionId } from "./types";

const SECTION_ICONS: Record<HelpSectionId, LucideIcon> = {
  overview: BookOpen,
  workspaces: Layers,
  agents: Bot,
  plans: GitBranch,
  memory: Brain,
  "decision-rerank": Sparkles,
  mesh: Network,
  integrations: Plug,
  tips: Lightbulb,
};

interface HelpPanelProps {
  config: AppConfig;
  locale?: string;
  initialSection?: HelpSectionId;
}

export function HelpPanel({ config, locale = "en", initialSection }: HelpPanelProps) {
  const content = getHelpContent(locale);
  const [activeId, setActiveId] = useState<HelpSectionId>(
    initialSection ?? content.sections[0]?.id ?? "overview",
  );
  const tabRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (initialSection) setActiveId(initialSection);
  }, [initialSection]);

  const scrollToSection = useCallback((id: HelpSectionId) => {
    setActiveId(id);
    const el = tabRef.current?.querySelector(`[data-help-section="${id}"]`);
    el?.scrollIntoView({ behavior: "smooth", block: "start" });
  }, []);

  return (
    <div className="flex flex-col md:flex-row h-full min-h-0 gap-4 max-w-5xl mx-auto" data-testid="help-panel">
      <nav
        className="w-full md:w-[220px] shrink-0 md:overflow-y-auto md:pr-1 space-y-1 hidden md:block"
        data-testid="help-nav"
        aria-label="Guide sections"
      >
        {content.sections.map((section) => (
          <HelpNavRow
            key={section.id}
            section={section}
            active={section.id === activeId}
            onSelect={() => scrollToSection(section.id)}
          />
        ))}
      </nav>

      <div className="md:hidden shrink-0 w-full">
        <label className="sr-only" htmlFor="help-section-select">
          Guide section
        </label>
        <select
          id="help-section-select"
          data-testid="help-section-select"
          value={activeId}
          onChange={(e) => scrollToSection(e.target.value as HelpSectionId)}
          className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded-lg px-3 py-2 text-xs text-[var(--ct-fg)]"
        >
          {content.sections.map((s) => (
            <option key={s.id} value={s.id}>
              {s.title}
            </option>
          ))}
        </select>
      </div>

      <div ref={tabRef} className="flex-1 min-w-0 overflow-y-auto space-y-10 pb-8" data-testid="help-content-tab">
        {content.sections.map((section) => (
          <section
            key={section.id}
            id={`help-${section.id}`}
            data-help-section={section.id}
            data-testid="help-section"
            data-help-section-id={section.id}
            className="scroll-mt-4"
          >
            <header className="mb-4">
              <h3 className="text-base font-semibold text-[var(--ct-fg)]">{section.title}</h3>
              <p className="text-sm text-[var(--ct-muted-fg)] mt-0.5">{section.subtitle}</p>
            </header>
            <div className="space-y-3">
              {section.blocks.map((block, i) => (
                <HelpBlockView key={`${section.id}-${i}`} block={block} config={config} />
              ))}
            </div>
          </section>
        ))}
      </div>
    </div>
  );
}

function HelpNavRow({
  section,
  active,
  onSelect,
}: {
  section: HelpSectionContent;
  active: boolean;
  onSelect: () => void;
}) {
  const Icon = SECTION_ICONS[section.id];
  return (
    <button
      type="button"
      data-testid="help-nav-item"
      data-help-nav={section.id}
      onClick={onSelect}
      className={`flex w-full items-start gap-2.5 rounded-xl px-2.5 py-2 text-left transition-colors ${
        active
          ? "bg-[color-mix(in_srgb,var(--ct-accent)_16%,transparent)] border border-[var(--ct-accent)]/40"
          : "hover:bg-[var(--ct-hover)] border border-transparent"
      }`}
    >
      <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-[color-mix(in_srgb,var(--ct-accent)_14%,transparent)] text-[var(--ct-accent)]">
        <Icon size={16} strokeWidth={2} />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-[12px] font-medium leading-tight text-[var(--ct-fg)]">{section.title}</span>
        <span className="mt-0.5 block text-[10px] leading-tight text-[var(--ct-muted-fg)] line-clamp-2">
          {section.subtitle}
        </span>
      </span>
    </button>
  );
}
