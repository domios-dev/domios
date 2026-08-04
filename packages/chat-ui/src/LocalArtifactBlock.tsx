import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { FileText, Image as ImageIcon, RefreshCw } from "lucide-react";
import { HtmlPreviewBlock } from "./HtmlPreviewBlock";
import {
  loadLocalArtifact,
  type LocalArtifactKind,
  type LocalPreviewFile,
} from "./localArtifactLoader";

export interface LocalArtifactDirective {
  kind: LocalArtifactKind;
  path: string;
}

type LoadState =
  | { status: "loading"; file: null; error: "" }
  | { status: "ready"; file: LocalPreviewFile; error: "" }
  | { status: "error"; file: null; error: string };

export function parseLocalArtifactDirective(source: string): LocalArtifactDirective | null {
  const match = /^\s*@ct-(page|image|document)\s+(?:"([^"]+)"|'([^']+)'|(.+?))\s*$/i.exec(
    source.replace(/\r/g, ""),
  );
  const path = (match?.[2] || match?.[3] || match?.[4] || "").trim();
  if (!match || !path || path.includes("\n")) return null;
  return { kind: match[1].toLowerCase() as LocalArtifactKind, path };
}

function fileName(path: string): string {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments[segments.length - 1] ?? path;
}

export function LocalArtifactBlock({
  directive,
  renderMarkdown,
}: {
  directive: LocalArtifactDirective;
  renderMarkdown: (source: string) => React.ReactNode;
}) {
  if (directive.kind === "page") {
    return <HtmlPreviewBlock text={`@file ${directive.path}`} />;
  }
  return (
    <LoadedLocalArtifact directive={directive} renderMarkdown={renderMarkdown} />
  );
}

function LoadedLocalArtifact({
  directive,
  renderMarkdown,
}: {
  directive: LocalArtifactDirective;
  renderMarkdown: (source: string) => React.ReactNode;
}) {
  const [state, setState] = useState<LoadState>({ status: "loading", file: null, error: "" });
  const generation = useRef(0);
  const name = useMemo(() => fileName(directive.path), [directive.path]);
  const reload = useCallback(async () => {
    const request = generation.current + 1;
    generation.current = request;
    setState({ status: "loading", file: null, error: "" });
    try {
      const file = await loadLocalArtifact(directive.path, directive.kind);
      if (generation.current === request) setState({ status: "ready", file, error: "" });
    } catch (error) {
      if (generation.current === request) {
        setState({
          status: "error",
          file: null,
          error: error instanceof Error ? error.message : String(error),
        });
      }
    }
  }, [directive.kind, directive.path]);

  useEffect(() => {
    void reload();
    window.addEventListener("focus", reload);
    return () => {
      generation.current += 1;
      window.removeEventListener("focus", reload);
    };
  }, [reload]);

  const icon = directive.kind === "image"
    ? <ImageIcon size={14} aria-hidden />
    : <FileText size={14} aria-hidden />;

  return (
    <section
      data-testid={`chat-ct-${directive.kind}`}
      className="my-2 overflow-hidden rounded-lg border"
      style={{ borderColor: "var(--ct-border)", background: "var(--ct-surface)" }}
      aria-label={`${directive.kind} preview: ${name}`}
    >
      <div
        className="flex items-center gap-2 border-b px-2.5 py-1.5 text-[11px]"
        style={{ borderColor: "var(--ct-border)", color: "var(--ct-muted-fg)" }}
      >
        {icon}
        <span className="font-medium" style={{ color: "var(--ct-fg)" }}>{name}</span>
        <span className="min-w-0 flex-1 truncate opacity-60" title={directive.path}>{directive.path}</span>
        <button
          type="button"
          onClick={() => void reload()}
          className="rounded p-1 hover:brightness-125"
          aria-label={`Reload ${name}`}
          title="Reload"
        >
          <RefreshCw size={13} aria-hidden />
        </button>
      </div>
      {state.status === "loading" && (
        <div className="px-3 py-6 text-center text-xs opacity-60">Loading…</div>
      )}
      {state.status === "error" && (
        <div role="alert" className="px-3 py-3 text-xs text-red-400">{state.error}</div>
      )}
      {state.status === "ready" && directive.kind === "image" && (
        <div className="flex justify-center p-2">
          <img
            src={state.file.data}
            alt={name}
            className="max-h-[720px] max-w-full rounded object-contain"
          />
        </div>
      )}
      {state.status === "ready" && directive.kind === "document" && (
        <div className="max-h-[720px] overflow-auto px-3 py-2 text-sm">
          {state.file.media_type === "text/markdown"
            ? renderMarkdown(state.file.data)
            : <pre className="m-0 whitespace-pre-wrap break-words font-mono text-xs">{state.file.data}</pre>}
        </div>
      )}
    </section>
  );
}
