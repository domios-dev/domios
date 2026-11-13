import { roleDisplayLabel } from "../../lib/roleKind";
import { roleTiming } from "../../lib/teamTemplates/startModel";
type TemplatePreview = import("../../lib/transport/teamTemplates").TemplatePreview;

/** The workers a template brings, each with what it does and when it joins; nothing is spawned at start. */
export function WorkerChips({ preview }: { preview: TemplatePreview }) {
  const workers = preview.form.roles.filter((role) => role.kind === "worker");
  if (workers.length === 0) return null;
  return (
    <div className="flex flex-col gap-1 text-[11px]" aria-label="Workers on demand" role="group">
      <span className="text-[var(--ct-muted-fg)]">Workers on demand</span>
      <ul className="flex flex-col gap-0.5" aria-label="Workers">
        {workers.map((role) => {
          const timing = role.start === "on_demand" ? null : roleTiming(role.start);
          const details = [role.description?.trim(), timing].filter(Boolean).join(" · ");
          return <li key={role.role} className="flex min-w-0 items-baseline gap-1.5">
            <span className="shrink-0">{roleDisplayLabel(role.role, role.label)}</span>
            {details && <span className="truncate text-[var(--ct-muted-fg)]" title={details}>{details}</span>}
          </li>;
        })}
      </ul>
    </div>
  );
}

