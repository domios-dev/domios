import { ActivityLogRow, type ActivityLogRowProps } from "../../../../src/components/Canvas/planView/ActivityLogRow";
import { ClickOnce, ForcePseudo } from "../stageTools";
import { defineSpecimen } from "../types";

const open = () => undefined;

const ROWS: ActivityLogRowProps[] = [
  { kind: "progress", color: "var(--ct-accent)", age: "6m", stamp: "Today 14:02 · 6m ago", author: "Detail designer", text: "Agent card renders the provider logo and model; lane rows moved under it." },
  { kind: "decision", color: "var(--ct-green)", age: "22m", stamp: "Today 13:46 · 22m ago", author: "Detail designer", text: "Dropped the status chip row: the status lives in the header pill only.\n\nThe board card keeps its own compact chip because it has no header." },
  { kind: "blocker", color: "var(--ct-red)", age: "48m", stamp: "Today 13:20 · 48m ago", author: "Release lead", text: "Waiting on the shared ActivityLogRow before the timeline can switch over." },
];

const LEADS: ActivityLogRowProps[] = ROWS.map((row, index) => ({ ...row, lead: ["REL-12", "REL-12", "REL-13"][index], onOpen: open }));

type RowsSpecimenProps = { rows: ActivityLogRowProps[]; expand?: boolean; hover?: boolean };

function RowsSpecimen({ rows, expand = false, hover = false }: RowsSpecimenProps) {
  const list = <section className="ct-activity" aria-label="Activity">
    <h3 className="ct-activity__title">{rows.some((row) => row.lead) ? "Recent activity" : "Activity"}</h3>
    {rows.map((row) => <ActivityLogRow key={row.kind} {...row} />)}
  </section>;
  if (expand) return <ClickOnce selector='.ct-activity-row:nth-of-type(2) .ct-activity-row__head'>{list}</ClickOnce>;
  if (hover) return <ForcePseudo selector=".ct-activity-row" state="hover">{list}</ForcePseudo>;
  return <div className="gallery-fill">{list}</div>;
}

export default defineSpecimen({
  id: "activity-log-row",
  title: "Activity log row",
  category: "Plans",
  where: "src/components/Canvas/planView/ActivityLogRow.tsx",
  stage: "wide",
  notes: "One activity event as a calm ct-row: age, the kind dot on its tone, who, and the flattened text. Without onOpen the head is a disclosure that expands to the stamp and the full text (task activity); with onOpen the whole row opens its task and a lead names it (plan recent activity).",
  Component: RowsSpecimen,
  states: [
    { id: "collapsed", label: "Task log · collapsed", props: { rows: ROWS } },
    { id: "expanded", label: "Expanded · stamp + full text", props: { rows: ROWS, expand: true } },
    { id: "lead", label: "Clickable · task lead", props: { rows: LEADS } },
    { id: "lead-hover", label: "Clickable · hover", props: { rows: LEADS, hover: true } },
  ],
});
