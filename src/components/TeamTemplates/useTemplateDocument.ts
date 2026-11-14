import { useEffect, useState } from "react";
import { parse } from "yaml";
import type { TeamDocument } from "../../../gen/TeamDocument";
import type { TemplateRef } from "../../lib/transport/teamTemplates";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import { startErrorView } from "../../lib/teamTemplates/startModel";

export function useTemplateDocument(template: TemplateRef) {
  const [result, setResult] = useState<{ text: string; document: TeamDocument } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { name, version } = template;
  useEffect(() => {
    const controller = new AbortController();
    teamTemplatesTransport().file({ template: { name, version }, path: "team.yaml" }, { signal: controller.signal })
      .then((file) => {
        const document = parse(file.text, { maxAliasCount: 0 }) as TeamDocument;
        if (!document || typeof document !== "object" || !document.roles) throw new Error("The package document has no roles.");
        if (!controller.signal.aborted) setResult({ text: file.text, document });
      })
      .catch((failure: unknown) => { if (!controller.signal.aborted) setError(startErrorView(failure).message); });
    return () => controller.abort();
  }, [name, version]);
  return { result, error };
}
