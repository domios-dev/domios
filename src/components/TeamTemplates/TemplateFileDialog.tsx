import { useEffect, useId, useState } from "react";
import { FormDialog } from "../shared/FormDialog";
import { Button } from "../shared/Button";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import type { TeamTemplatesTransport, TemplateRef } from "../../lib/transport/teamTemplates";
import { startErrorView } from "../../lib/teamTemplates/startModel";

/** A package file by its path, or a role's instructions wherever the package keeps them. */
export type TemplateFileTarget = { path: string } | { role: string; title: string };

/** The exact text of one package file or role's instructions, so the owner can read it before keeping it. It never runs anything. */
export function TemplateFileDialog({ template, target, onClose, transport = teamTemplatesTransport }: {
  template: TemplateRef;
  target: TemplateFileTarget;
  onClose: () => void;
  transport?: () => TeamTemplatesTransport;
}) {
  const titleId = useId();
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { name, version } = template;
  const path = "path" in target ? target.path : null;
  const role = "role" in target ? target.role : null;
  const title = "path" in target ? target.path : target.title;
  useEffect(() => {
    const controller = new AbortController();
    const which = path !== null ? { path } : { role: role ?? "" };
    transport().file({ template: { name, version }, ...which }, { signal: controller.signal })
      .then((file) => setText(file.text), (failure: unknown) => { if (!controller.signal.aborted) setError(startErrorView(failure).message); });
    return () => controller.abort();
  }, [name, version, path, role, transport]);
  return (
    <FormDialog titleId={titleId} className="max-w-2xl" onCancel={onClose} onSubmit={(event) => { event.preventDefault(); onClose(); }}>
      <h2 id={titleId} className={`${path !== null ? "font-mono text-[13px]" : "text-base"} font-semibold`}>{title}</h2>
      {error ? <p role="alert" className="text-[12px] text-[var(--ct-red)]">{error}</p>
        : text === null ? <p aria-busy="true" className="text-[12px] text-[var(--ct-muted-fg)]">Reading the file…</p>
          : <pre aria-label={title} className="max-h-[60vh] overflow-auto whitespace-pre-wrap rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2.5 font-mono text-[11px] leading-snug">{text}</pre>}
      <div className="flex justify-end"><Button type="submit" variant="primary">Close</Button></div>
    </FormDialog>
  );
}
