import * as React from "react";
import { FileJson } from "lucide-react";
import { ChevronIcon, PRE_WRAP_CLASS } from "./CodeBlock";
import { TEXT_WRAP_CLASS } from "./MarkdownBody";
import { pickUiFontScaleValue } from "./uiFontScale";

interface ContextRequestCardProps {
  content: string;
  fontScale?: number;
}

export function ContextRequestCard({ content, fontScale }: ContextRequestCardProps) {
  const [expanded, setExpanded] = React.useState(false);
  const bodyText = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);

  return (
    <div className="mb-3 w-full max-w-4xl min-w-0 mx-auto">
      <div
        data-testid="chat-context-request-card"
        className="max-w-full min-w-0 rounded-md border"
        style={{
          background: "color-mix(in srgb, var(--ct-surface, #161b22) 92%, var(--ct-accent, #58a6ff) 8%)",
          borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 28%, transparent)",
          color: "var(--ct-fg)",
        }}
      >
        <button
          type="button"
          onClick={() => setExpanded(!expanded)}
          className="flex w-full min-w-0 items-center gap-2 px-3 py-2 text-left transition-colors hover:brightness-110"
          style={{ color: "var(--ct-fg)" }}
        >
          <FileJson size={14} strokeWidth={2} style={{ color: "var(--ct-accent, #58a6ff)" }} />
          <span className="min-w-0 flex-1">
            <span className="block text-xs font-medium">Context sent</span>
            <span className="block truncate text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
              Assembled machine input for this tick
            </span>
          </span>
          <ChevronIcon expanded={expanded} className="opacity-60" />
        </button>
        {expanded && (
          <div
            className={`border-t px-3 py-2 ${bodyText} ${TEXT_WRAP_CLASS}`}
            style={{ borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 18%, transparent)" }}
          >
            <pre className={`${PRE_WRAP_CLASS} m-0 font-mono text-[11px]`} style={{ color: "var(--ct-fg)" }}>
              {content}
            </pre>
          </div>
        )}
      </div>
    </div>
  );
}
