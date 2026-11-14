import { useEffect, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { SyntaxHighlighter } from "@codeterm/chat-ui/syntaxHighlighter";
import { themeAwarePrismStyle, SYNTAX_HIGHLIGHT_ROOT_CLASS } from "@codeterm/chat-ui/syntaxTheme";
import type { TemplateRef } from "../../lib/transport/teamTemplates";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import { startErrorView } from "../../lib/teamTemplates/startModel";

export function TemplateMarkdown({ text }: { text: string }) {
  return <div className="text-[13px] leading-relaxed [overflow-wrap:anywhere] [&_p]:my-3 [&_h1]:text-xl [&_h2]:text-lg [&_h3]:text-base [&_ul]:list-disc [&_ul]:pl-5 [&_ol]:list-decimal [&_ol]:pl-5 [&_pre]:overflow-auto [&_a]:text-[var(--ct-accent)]">
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={{ img: ({ alt }) => <span>{alt}</span> }}>{text}</ReactMarkdown>
  </div>;
}

export function TemplateYaml({ text }: { text: string }) {
  return <div role="region" aria-label="team.yaml" tabIndex={0} className="min-w-0 text-[12px]" style={{ maxWidth: "100%", overflowX: "auto" }}>
    <SyntaxHighlighter language="yaml" explicitHighlight style={themeAwarePrismStyle} className={SYNTAX_HIGHLIGHT_ROOT_CLASS}
      customStyle={{ margin: 0, background: "var(--ct-bg)", whiteSpace: "pre", overflow: "visible", width: "max-content", minWidth: "100%" }}
      codeTagProps={{ style: { whiteSpace: "pre" } }}>{text}</SyntaxHighlighter>
  </div>;
}

function usePackageText(template: TemplateRef, target: { path: string } | { role: string }) {
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { name, version } = template;
  const path = "path" in target ? target.path : null;
  const role = "role" in target ? target.role : null;
  useEffect(() => {
    const controller = new AbortController();
    teamTemplatesTransport().file({ template: { name, version }, ...(path === null ? { role: role! } : { path }) }, { signal: controller.signal })
      .then((file) => { if (!controller.signal.aborted) setText(file.text); },
        (failure: unknown) => { if (!controller.signal.aborted) setError(startErrorView(failure).message); });
    return () => controller.abort();
  }, [name, version, path, role]);
  return { text, error };
}

export function TemplatePackageText({ template, target, markdown = true }: { template: TemplateRef; target: { path: string } | { role: string }; markdown?: boolean }) {
  const { text, error } = usePackageText(template, target);
  return error ? <p role="alert">{error}</p>
    : text === null ? <p aria-busy="true" className="text-[var(--ct-muted-fg)]">Reading…</p>
      : markdown ? <TemplateMarkdown text={text} /> : <pre className="whitespace-pre-wrap break-words text-[12px]">{text}</pre>;
}
