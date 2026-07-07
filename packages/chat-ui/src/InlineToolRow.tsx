import React from "react";
import { ChevronDown } from "lucide-react";
import { buildToolPreview } from "@codeterm/chat-core";
import { shortPath } from "./MarkdownBody";

type InlineToolStatus = "pending" | "ok" | "failed";
type InlineToolGlyph = "shell" | "read" | "search" | "generic";

function statusGlyph(status: InlineToolStatus): React.ReactNode {
  if (status === "pending") {
    return (
      <span
        className="inline-block animate-spin opacity-60"
        style={{ color: "var(--ct-muted-fg)" }}
        title="running"
      >
        ◜
      </span>
    );
  }
  if (status === "failed") {
    return (
      <span
        className="opacity-100"
        style={{ color: "var(--ct-muted-fg)" }}
        title="failed"
      >
        ✕
      </span>
    );
  }
  return (
    <span className="opacity-60" style={{ color: "var(--ct-muted-fg)" }} title="completed">
      ✓
    </span>
  );
}

function kindGlyph(kind: InlineToolGlyph): string {
  if (kind === "shell") return "$";
  if (kind === "read") return "→";
  if (kind === "search") return "✱";
  return "⚙";
}

export function InlineToolRow({
  label,
  primaryArg,
  resultText,
  status,
  glyphKind,
  expanded,
  onToggle,
  expandedContent,
}: {
  label: string;
  primaryArg: string;
  resultText?: string;
  status: InlineToolStatus;
  glyphKind: InlineToolGlyph;
  expanded: boolean;
  onToggle: () => void;
  expandedContent?: React.ReactNode;
}) {
  const preview = buildToolPreview(resultText ?? "", 1);
  const shownPrimary = primaryArg.includes("/") ? shortPath(primaryArg) : primaryArg;
  return (
    <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto group" data-testid="chat-tool-inline">
      <button
        type="button"
        onClick={onToggle}
        className="flex w-full min-w-0 items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-xs transition-colors hover:bg-[color-mix(in_srgb,var(--ct-surface,#161b22)_84%,transparent)]"
        style={{ color: "var(--ct-muted-fg)" }}
      >
        <span className="w-4 text-center font-mono">{statusGlyph(status)}</span>
        <span className="w-3 text-center opacity-55">{kindGlyph(glyphKind)}</span>
        <span className="font-mono">{label}</span>
        <span className="min-w-0 truncate font-mono text-[11px]" style={{ color: "var(--ct-fg)" }}>
          {shownPrimary}
        </span>
        {preview.lines[0] && (
          <span className="min-w-0 flex-1 truncate font-mono text-[11px] opacity-70">
            {"→ "}
            {preview.lines[0]}
          </span>
        )}
        <ChevronDown
          size={12}
          className={`ml-auto transition-transform opacity-0 group-hover:opacity-100 ${expanded ? "rotate-180" : ""}`}
        />
      </button>
      {expanded && expandedContent && (
        <div
          className="mt-1 w-full min-w-0 max-w-full rounded-md border border-[var(--ct-border)] p-2 text-xs"
          style={{ background: "color-mix(in srgb, var(--ct-bg, #0d1117) 88%, black 4%)" }}
        >
          {expandedContent}
        </div>
      )}
    </div>
  );
}
