import { useEffect, useId, useState } from "react";
import { FormDialog } from "../shared/FormDialog";
import { Button } from "../shared/Button";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import type { TeamTemplatesTransport, TemplateRef } from "../../lib/transport/teamTemplates";
import { startErrorView } from "../../lib/teamTemplates/startModel";

/** The exact text of one package file, so the owner can read a script before keeping it. It never runs anything. */
export function TemplateFileDialog({ template, path, onClose, transport = teamTemplatesTransport }: {
  template: TemplateRef;
  path: string;
  onClose: () => void;
  transport?: () => TeamTemplatesTransport;
}) {
  const titleId = useId();
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { name, version } = template;
  useEffect(() => {
    const controller = new AbortController();
    transport().file({ template: { name, version }, path }, { signal: controller.signal })
      .then((file) => setText(file.text), (failure: unknown) => { if (!controller.signal.aborted) setError(startErrorView(failure).message); });
    return () => controller.abort();
  }, [name, version, path, transport]);
  return (
    <FormDialog titleId={titleId} className="max-w-2xl" onCancel={onClose} onSubmit={(event) => { event.preventDefault(); onClose(); }}>
      <h2 id={titleId} className="font-mono text-[13px] font-semibold">{path}</h2>
      {error ? <p role="alert" className="text-[12px] text-[var(--ct-red)]">{error}</p>
        : text === null ? <p aria-busy="true" className="text-[12px] text-[var(--ct-muted-fg)]">Reading the file…</p>
          : <pre aria-label={path} className="max-h-[60vh] overflow-auto rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2.5 font-mono text-[11px] leading-snug">{text}</pre>}
      <div className="flex justify-end"><Button type="submit" variant="primary">Close</Button></div>
    </FormDialog>
  );
}
