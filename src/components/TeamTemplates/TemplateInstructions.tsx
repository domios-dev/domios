import { useEffect, useState } from "react";
import { RichMarkdown } from "@codeterm/chat-ui/MarkdownBody";
import { BackPill } from "../shared/BackPill";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import type { TeamTemplatesTransport, TemplatePreview } from "../../lib/transport/teamTemplates";
import { startErrorView, templateTitle } from "../../lib/teamTemplates/startModel";
import { roleHeading, reviewRoles } from "./templateRoles";

/** A role's instructions as a step of the same dialog: back to the template, a role switcher, then the rendered Markdown. */
export function TemplateInstructions({ preview, role, onRole, onBack, transport = teamTemplatesTransport }: {
  preview: TemplatePreview;
  role: string;
  onRole: (role: string) => void;
  onBack: () => void;
  transport?: () => TeamTemplatesTransport;
}) {
  const [text, setText] = useState<{ role: string; text: string } | null>(null);
  const [error, setError] = useState<{ role: string; message: string } | null>(null);
  const { name, version } = preview.form.template;
  const roles = reviewRoles(preview.form.roles);
  const current = roles.find((candidate) => candidate.role === role) ?? roles[0];
  useEffect(() => {
    const controller = new AbortController();
    transport().file({ template: { name, version }, role }, { signal: controller.signal })
      .then((file) => setText({ role, text: file.text }), (failure: unknown) => {
        if (!controller.signal.aborted) setError({ role, message: startErrorView(failure).message });
      });
    return () => controller.abort();
  }, [name, version, role, transport]);
  const heading = current ? roleHeading(current) : { name: role, slot: null };
  const body = text?.role === role ? text.text : null;
  const failure = error?.role === role ? error.message : null;
  return (
    <section aria-labelledby="template-instructions-title" className="flex flex-col gap-3" data-testid="template-instructions">
      <div><BackPill label={templateTitle(preview.form.template)} onBack={onBack} /></div>
      {roles.length > 1 && <div role="group" aria-label="Roles" className="flex flex-wrap gap-1.5">
        {roles.map((candidate) => {
          const label = roleHeading(candidate);
          return <button key={`${candidate.group}:${candidate.role}`} type="button" aria-pressed={candidate.role === role}
            className={`ct-small-pill ${candidate.role === role ? "text-[var(--ct-fg)] ring-1 ring-[var(--ct-accent)]" : "hover:text-[var(--ct-fg)]"}`}
            onClick={() => onRole(candidate.role)}>{label.name}</button>;
        })}
      </div>}
      <div>
        <div className="text-[11px] text-[var(--ct-muted-fg)]">Instructions</div>
        <h3 id="template-instructions-title" className="text-[15px] font-semibold">
          {heading.name}{heading.slot && <span className="font-normal text-[var(--ct-muted-fg)]"> · {heading.slot}</span>}
        </h3>
      </div>
      {failure ? <p role="alert" className="text-[12px] text-[var(--ct-red)]">{failure}</p>
        : body === null ? <p aria-busy="true" className="text-[12px] text-[var(--ct-muted-fg)]">Reading the instructions…</p>
          : <div className="text-[13px] leading-relaxed [font-family:var(--ct-font-ui)]" aria-label={`${heading.name} instructions`}><RichMarkdown text={body} /></div>}
    </section>
  );
}
