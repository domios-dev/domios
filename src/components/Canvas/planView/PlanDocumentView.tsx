import { RichMarkdown } from "@codeterm/chat-ui";
import type { ReactNode } from "react";
import type { Tab } from "../../../lib/types";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import type { Plan, Section, SpecLink } from "../types";
import { flattenSections } from "../sectionRows";
import { PlanFetchErrorState, PlanFetchReconnectingBanner } from "./PlanFetchErrorState";
import { PlanParagraphAsk } from "./PlanParagraphAsk";
import { PlanSwitcher } from "./PlanSwitcher";
import { usePlanDocumentData } from "./usePlanDocumentData";

type DocumentKind = "spec" | "plan";

interface PlanDocumentViewProps extends PlanViewSurfaceProps {
  kind: DocumentKind;
  onOpenSpec?: () => void;
}

interface SectionEntry {
  section: Section;
  depth: number;
}

interface AskSurface {
  tabs?: Record<string, Tab>;
  onOpenTab?: (tabId: string) => void;
}

function sectionEntries(sections: Section[], depth = 0): SectionEntry[] {
  return sections.flatMap((section) => [
    { section, depth },
    ...sectionEntries(section.subsections ?? [], depth + 1),
  ]);
}

function pageTitle(kind: DocumentKind, plan: Plan): string {
  return kind === "spec" ? `${plan.title.trim() || "Untitled plan"} · Spec` : plan.title.trim() || "Untitled plan";
}

function DocumentEmpty({ kind, message }: { kind: DocumentKind; message: string }) {
  return (
    <div data-testid={`plan-${kind}-empty`} className="plan-document-empty">
      <p>{message}</p>
    </div>
  );
}

function SectionIndex({ kind, entries }: { kind: DocumentKind; entries: SectionEntry[] }) {
  if (entries.length < 3) return null;
  const label = kind === "spec" ? "Spec sections" : "Plan sections";
  return (
    <>
      <nav className="plan-document-index plan-document-index-desktop" aria-label={label}>
        <h2>{label}</h2>
        <ol>
          {entries.map(({ section, depth }) => (
            <li key={section.id} style={{ marginInlineStart: `${depth * 0.75}rem` }}>
              <a href={`#plan-${kind}-section-${section.id}`}>{section.title}</a>
            </li>
          ))}
        </ol>
      </nav>
      <details className="plan-document-index-mobile">
        <summary>{label} · {entries.length}</summary>
        <nav aria-label={label}>
          <ol>
            {entries.map(({ section, depth }) => (
              <li key={section.id} style={{ marginInlineStart: `${depth * 0.75}rem` }}>
                <a href={`#plan-${kind}-section-${section.id}`}>{section.title}</a>
              </li>
            ))}
          </ol>
        </nav>
      </details>
    </>
  );
}

function SectionBody({ sectionId, body }: { sectionId: string; body: string }) {
  if (!body.trim()) {
    return <p className="plan-document-section-empty">No section text yet.</p>;
  }
  return (
    <div className="plan-document-section-body" data-plan-ask-body="" data-section-id={sectionId}>
      <RichMarkdown text={body} sourcePositions />
    </div>
  );
}

function SpecSection({ kind, entry, children }: { kind: DocumentKind; entry: SectionEntry; children?: ReactNode }) {
  const { section, depth } = entry;
  const Heading = depth === 0 ? "h2" : "h3";
  return (
    <section
      id={`plan-${kind}-section-${section.id}`}
      data-testid={`plan-${kind}-section`}
      data-section-id={section.id}
      className="plan-document-section"
    >
      <Heading className="plan-document-section-title">{section.title}</Heading>
      <SectionBody sectionId={section.id} body={section.body ?? ""} />
      {children}
    </section>
  );
}

function SpecDocument({ plan, sections, ask }: { plan: Plan; sections: Section[]; ask: AskSurface }) {
  const entries = sectionEntries(sections);
  return (
    <DocumentLayout kind="spec" title={pageTitle("spec", plan)} entries={entries}>
      <PlanParagraphAsk kind="spec" plan={plan} sections={sections} tabs={ask.tabs} onOpenTab={ask.onOpenTab}>
        {entries.map((entry) => <SpecSection key={entry.section.id} kind="spec" entry={entry} />)}
      </PlanParagraphAsk>
    </DocumentLayout>
  );
}

function goalValues(plan: Plan): Array<{ label: string; value: string; list?: string[] }> {
  const goal = plan.goal;
  if (!goal) return [];
  return [
    goal.vector?.trim() ? { label: "Goal vector", value: goal.vector.trim() } : null,
    goal.vision?.trim() ? { label: "Vision", value: goal.vision.trim() } : null,
    goal.constraints?.length ? { label: "Constraints", value: "", list: goal.constraints } : null,
    goal.postulates?.length ? { label: "Postulates", value: "", list: goal.postulates } : null,
  ].filter((item): item is { label: string; value: string; list?: string[] } => Boolean(item));
}

function PlanGoal({ plan }: { plan: Plan }) {
  const values = goalValues(plan);
  return (
    <section data-testid="plan-document-goal" className="plan-document-goal" aria-labelledby="plan-document-goal-heading">
      <h2 id="plan-document-goal-heading">Goal</h2>
      {values.length === 0 && <p className="plan-document-section-empty">No goal yet.</p>}
      {values.map(({ label, value, list }) => (
        <div key={label} className="plan-document-goal-field">
          <h3>{label}</h3>
          {list ? (
            <ul>
              {list.map((item) => <li key={item}>{item}</li>)}
            </ul>
          ) : (
            <p>{value}</p>
          )}
        </div>
      ))}
    </section>
  );
}

function specSectionTitle(sectionId: string, sections: Section[]): string {
  return flattenSections(sections).find((section) => section.id === sectionId)?.title ?? sectionId;
}

function PlanSectionLinks({
  section,
  specSections,
  links,
  onOpenSpec,
}: {
  section: Section;
  specSections: Section[];
  links: SpecLink[];
  onOpenSpec?: () => void;
}) {
  const sectionLinks = links.filter((link) => link.plan_section_id === section.id);
  if (sectionLinks.length === 0) return null;
  return (
    <div className="plan-document-section-links" aria-label="Spec sections served">
      <span className="plan-document-section-links-label">Serves</span>
      {sectionLinks.map((link) => (
        <a
          key={`${link.spec_id}:${link.spec_section_id}`}
          href={`#plan-spec-section-${link.spec_section_id}`}
          data-testid="plan-document-spec-link"
          data-spec-section-id={link.spec_section_id}
          onClick={(event) => {
            if (!onOpenSpec) return;
            event.preventDefault();
            onOpenSpec();
          }}
        >
          Spec · {specSectionTitle(link.spec_section_id, specSections)}
        </a>
      ))}
    </div>
  );
}

function PlanDocument({
  plan,
  specSections,
  onOpenSpec,
  ask,
}: {
  plan: Plan;
  specSections: Section[];
  onOpenSpec?: () => void;
  ask: AskSurface;
}) {
  const entries = sectionEntries(plan.sections);
  return (
    <DocumentLayout kind="plan" title={pageTitle("plan", plan)} entries={entries}>
      <PlanParagraphAsk kind="plan" plan={plan} sections={plan.sections} tabs={ask.tabs} onOpenTab={ask.onOpenTab}>
        <PlanGoal plan={plan} />
        {entries.length === 0 ? (
          <DocumentEmpty kind="plan" message="No plan sections yet." />
        ) : (
          entries.map((entry) => (
            <SpecSection key={entry.section.id} kind="plan" entry={entry}>
              <PlanSectionLinks
                section={entry.section}
                specSections={specSections}
                links={plan.spec_links ?? []}
                onOpenSpec={onOpenSpec}
              />
            </SpecSection>
          ))
        )}
      </PlanParagraphAsk>
    </DocumentLayout>
  );
}

function DocumentLayout({
  kind,
  title,
  entries,
  children,
}: {
  kind: DocumentKind;
  title: string;
  entries: SectionEntry[];
  children: ReactNode;
}) {
  return (
    <main data-testid={`plan-${kind}-view`} className="plan-document-view" aria-labelledby={`plan-${kind}-title`}>
      <div className="plan-document-layout">
        <SectionIndex kind={kind} entries={entries} />
        <article className="plan-document-readable">
          <p className="plan-document-eyebrow">{kind === "spec" ? "Specification" : "Plan"}</p>
          <h1 id={`plan-${kind}-title`} data-testid={`plan-${kind}-title`} className="plan-document-title">{title}</h1>
          {children}
        </article>
      </div>
    </main>
  );
}

export function PlanDocumentView({ kind, onOpenSpec, ...surfaceProps }: PlanDocumentViewProps) {
  const data = usePlanDocumentData(surfaceProps);
  const { plans, loading = false, reconnecting = false, error = null, onRetry, teamId, tabs, onOpenTab } = surfaceProps;
  const selectedPlan = data.selectedPlan;
  const ask: AskSurface = { tabs, onOpenTab };

  return (
    <div data-testid={`plan-${kind}-surface`} className="plan-document-surface">
      <PlanFetchErrorState
        loading={loading}
        error={error}
        itemCount={plans.length}
        onRetry={onRetry}
        testId={`plan-${kind}`}
        className="plan-document-state"
      />
      <PlanFetchReconnectingBanner
        reconnecting={reconnecting}
        itemCount={plans.length}
        testId={`plan-${kind}-reconnecting`}
        className="plan-document-reconnecting"
      />
      {data.showsData && !selectedPlan && (
        <DocumentEmpty kind={kind} message="No active plans in this workspace." />
      )}
      {data.showsData && selectedPlan && (
        <div className="plan-document-content">
          {!teamId && (
            <PlanSwitcher
              plans={data.switcherOptions}
              selectedId={selectedPlan.id}
              onSelect={data.selectPlan}
              onAction={data.handleAction}
              label="Plan"
            />
          )}
          {kind === "spec" && data.specLoading && (
            <p data-testid="plan-spec-loading" className="plan-document-state">Loading the Spec…</p>
          )}
          {kind === "spec" && !data.specLoading && !selectedPlan.primary_spec_id && (
            <DocumentEmpty kind="spec" message="No Spec yet." />
          )}
          {kind === "spec" && !data.specLoading && selectedPlan.primary_spec_id && data.selectedSpecSections.length === 0 && (
            <DocumentEmpty kind="spec" message="No Spec sections yet." />
          )}
          {kind === "spec" && !data.specLoading && data.selectedSpecSections.length > 0 && (
            <SpecDocument plan={selectedPlan} sections={data.selectedSpecSections} ask={ask} />
          )}
          {kind === "plan" && (
            <PlanDocument plan={selectedPlan} specSections={data.selectedSpecSections} onOpenSpec={onOpenSpec} ask={ask} />
          )}
        </div>
      )}
    </div>
  );
}

export function PlanSpecView(props: PlanViewSurfaceProps) {
  return <PlanDocumentView kind="spec" {...props} />;
}

export function PlanPlanView(props: PlanViewSurfaceProps & { onOpenSpec?: () => void }) {
  return <PlanDocumentView kind="plan" {...props} />;
}
