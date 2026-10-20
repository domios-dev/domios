import { BoundedMessageContent } from "./BoundedMessageContent";
import React from "react";
import { FileText } from "lucide-react";
import type { AttachedDiscussionDoc } from "./discussionDoc";
import { ChevronIcon, PRE_WRAP_CLASS } from "./CodeBlock";
import { RichMarkdown, fileName } from "./MarkdownBody";

export interface AttachedDiscussionDocProps {
  doc: AttachedDiscussionDoc;
  variant?: "chat" | "canvas";
}

export function AttachedDiscussionDocView({ doc, variant = "chat" }: AttachedDiscussionDocProps) {
  const [expanded, setExpanded] = React.useState(false);
  const label = fileName(doc.path);
  const dense = variant === "canvas";

  return (
    <div
      data-testid="attached-discussion-doc"
      data-doc-path={doc.path}
      className={dense ? "min-w-0" : "w-full min-w-0"}
    >
      {doc.summary.length > 0 && (
        <div
          data-testid="attached-discussion-summary"
          className={dense ? "text-[0.82em] leading-relaxed" : "mb-2 text-sm leading-relaxed"}
        >
          <RichMarkdown text={doc.summary} />
        </div>
      )}
      <div
        className="rounded-md border"
        style={{
          background: "color-mix(in srgb, var(--ct-surface, #161b22) 88%, transparent)",
          borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
        }}
      >
        <button
          type="button"
          data-testid="attached-discussion-toggle"
          onClick={() => setExpanded((v) => !v)}
          className={`flex w-full min-w-0 items-center gap-2 text-left ${dense ? "px-2 py-1 text-[11px]" : "px-2.5 py-1.5 text-xs"}`}
          style={{
            color: "var(--ct-muted-fg)",
            background: "transparent",
            border: "none",
          }}
        >
          <FileText
            size={dense ? 11 : 12}
            strokeWidth={2}
            className="shrink-0"
            style={{ color: "var(--ct-accent, #58a6ff)" }}
          />
          <span className="shrink-0 font-mono" style={{ color: "var(--ct-accent, #58a6ff)" }}>
            {label}
          </span>
          <span className="min-w-0 flex-1 truncate opacity-70" title={doc.path}>
            {doc.path}
          </span>
          <ChevronIcon expanded={expanded} className="shrink-0 opacity-50" />
        </button>
        {expanded && (
          <BoundedMessageContent collapsedHeight={dense ? "16rem" : "28rem"}
            data-testid="attached-discussion-body"
            className={`border-t ${dense ? "px-2 py-1.5 text-[11px]" : "px-3 py-2 text-xs"}`}
            style={{
              borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 16%, transparent)",
              color: "var(--ct-fg)",
            }}
          >
            <div className="text-sm leading-relaxed">
              <RichMarkdown text={doc.body} />
            </div>
          </BoundedMessageContent>
        )}
        {!expanded && doc.summary.length === 0 && (
          <div className={`border-t px-2.5 py-1 ${PRE_WRAP_CLASS} opacity-60 ${dense ? "text-[10px]" : "text-[11px]"}`}>
            Discussion doc attached. Expand to read.
          </div>
        )}
      </div>
    </div>
  );
}
