import { attachmentPathName as fileName } from "@codeterm/chat-core";
import { BoundedMessageContent } from "./BoundedMessageContent";
import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { FileText, RefreshCw, Maximize2, FolderOpen, X } from "lucide-react";
import { LocalImageGallery } from "./LocalImageGallery";
import { FullscreenOverlay } from "./FullscreenOverlay";
import { HtmlPreviewBlock } from "./HtmlPreviewBlock";
import {
  loadLocalArtifact,
  openLocalArtifact,
  localArtifactOpeningAvailable,
  type LocalPreviewFile,
} from "./localArtifactLoader";

import { useLocalArtifactTabId } from "./LocalArtifactScope";

import type { LocalArtifactDirective } from "@codeterm/chat-core";
export { parseLocalArtifactDirective } from "@codeterm/chat-core";

type LoadState =
  | { status: "loading"; file: null; error: "" }
  | { status: "ready"; file: LocalPreviewFile; error: "" }
  | { status: "error"; file: null; error: string };

interface LocalArtifactProps {
  directive: LocalArtifactDirective;
  renderMarkdown: (source: string) => React.ReactNode;
  renderError?: (error: string) => React.ReactNode;
  renderText?: (source: string) => React.ReactNode;
  fullscreenRequest?: number;
  /** Renders only the content, for a host that supplies its own header and actions. */
  bare?: boolean;
  onLoaded?: (file: LocalPreviewFile | null) => void;
}

export function LocalArtifactBlock({
  directive,
  renderMarkdown,
  renderText,
  renderError,
  fullscreenRequest = 0,
  bare = false,
  onLoaded,
}: LocalArtifactProps) {
  if (directive.kind === "image") return <LocalImageGallery directive={directive} fullscreenRequest={fullscreenRequest} onLoaded={onLoaded} bare={bare} renderError={renderError} />;
  if (directive.kind === "page" && !/\.(md|markdown)$/i.test(directive.path)) {
    return <HtmlPreviewBlock filePath={directive.path} renderError={renderError} fullscreenRequest={fullscreenRequest} />;
  }
  return (
    <LoadedLocalArtifact directive={directive.kind === "page" ? { ...directive, kind: "document" } : directive} renderMarkdown={renderMarkdown} renderText={renderText} renderError={renderError} fullscreenRequest={fullscreenRequest} bare={bare} onLoaded={onLoaded} />
  );
}

function LoadedLocalArtifact({
  directive,
  renderMarkdown,
  renderText,
  renderError,
  fullscreenRequest = 0,
  bare = false,
  onLoaded,
}: LocalArtifactProps) {
  const tabId = useLocalArtifactTabId();
  const canReveal = localArtifactOpeningAvailable();
  const [state, setState] = useState<LoadState>({ status: "loading", file: null, error: "" });
  const [fullscreen, setFullscreen] = useState(false);
  const fullscreenRef = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setFullscreen(false), []);
  const sectionRef = useRef<HTMLElement>(null);
  useEffect(() => { if (fullscreenRequest) setFullscreen(true); }, [fullscreenRequest]);
  const generation = useRef(0);
  const onLoadedRef = useRef(onLoaded);
  onLoadedRef.current = onLoaded;
  useEffect(() => { onLoadedRef.current?.(state.status === "ready" ? state.file : null); }, [state]);
  const name = useMemo(() => fileName(directive.path), [directive.path]);
  const reload = useCallback(async () => {
    const request = generation.current + 1;
    generation.current = request;
    setState({ status: "loading", file: null, error: "" });
    try {
      const file = await loadLocalArtifact(directive.path, directive.kind, tabId);
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
  }, [directive.kind, directive.path, tabId]);

  useEffect(() => {
    void reload();
    return () => {
      generation.current += 1;
    };
  }, [reload]);

  const icon = <FileText size={14} aria-hidden />;

  const content = <>
    {state.status === "loading" && <p>Loading…</p>}
    {state.status === "error" && (renderError ? renderError(state.error) : <p role="alert">{state.error}</p>)}
    {state.status === "ready" && (state.file.media_type === "text/markdown" ? renderMarkdown(state.file.data)
        : renderText ? renderText(state.file.data) : <pre className="whitespace-pre-wrap break-words">{state.file.data}</pre>)}
  </>;
  const reveal = () => void openLocalArtifact(directive.path).catch((error: unknown) => setState({ status: "error", file: null, error: String(error) }));
  return (
    <section ref={sectionRef}
      data-testid={`chat-ds-${directive.kind}`}
      className={bare ? "" : "my-2 overflow-hidden rounded-lg border"}
      style={bare ? undefined : { borderColor: "var(--ct-border)", background: "var(--ct-surface)" }}
      aria-label={`${directive.kind} preview: ${name}`}
    >
      {!bare && <div
        className="flex items-center gap-2 border-b px-2.5 py-1.5 text-[11px]"
        style={{ borderColor: "var(--ct-border)", color: "var(--ct-muted-fg)" }}
      >
        {icon}
        <span className="font-medium" style={{ color: "var(--ct-fg)" }}>{name}</span>
        <span className="min-w-0 flex-1 truncate opacity-60" title={directive.path}>{directive.path}</span>
        <button type="button" onClick={() => setFullscreen(true)} aria-label={`Fullscreen ${name}`} title="Fullscreen"><Maximize2 size={13} aria-hidden /></button>
        <button type="button" onClick={reveal} disabled={!canReveal} aria-label={`Reveal ${name}`} title={canReveal ? "Reveal in file manager" : "Revealing files is available only in the desktop app"}><FolderOpen size={13} aria-hidden /></button>
        <button
          type="button"
          onClick={() => void reload()}
          className="rounded p-1 hover:brightness-125"
          aria-label={`Reload ${name}`}
          title="Reload"
        >
          <RefreshCw size={13} aria-hidden />
        </button>
      </div>}
      {bare ? <div className="text-sm">{content}</div> : <BoundedMessageContent collapsedHeight="720px" className="px-3 py-2 text-sm">{content}</BoundedMessageContent>}
      {fullscreen && <FullscreenOverlay panelRef={fullscreenRef} label={`Fullscreen ${name}`} onClose={close}>
        <header className="flex items-center gap-2 p-2 text-sm"><span className="min-w-0 flex-1 break-words" title={directive.path}>{directive.path}</span>
          <button type="button" onClick={reveal} aria-label={`Reveal ${name}`}><FolderOpen size={15} /></button>
          <button type="button" onClick={close} aria-label="Close fullscreen"><X size={15} /></button>
        </header>
        <div className="min-h-0 flex-1 overflow-auto p-4 text-sm">{content}</div>
      </FullscreenOverlay>}
    </section>
  );
}
