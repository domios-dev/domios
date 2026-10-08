import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { FileText, Image as ImageIcon, RefreshCw, Maximize2, FolderOpen, X } from "lucide-react";
import { createPortal } from "react-dom";
import { FullscreenOverlay, useFullscreenKeyboard } from "./FullscreenOverlay";
import { HtmlPreviewBlock } from "./HtmlPreviewBlock";
import {
  loadLocalArtifact,
  openLocalArtifact,
  type LocalPreviewFile,
} from "./localArtifactLoader";

import type { LocalArtifactDirective } from "@codeterm/chat-core";
export { parseLocalArtifactDirective } from "@codeterm/chat-core";

type LoadState =
  | { status: "loading"; file: null; error: "" }
  | { status: "ready"; file: LocalPreviewFile; error: "" }
  | { status: "error"; file: null; error: string };

function fileName(path: string): string {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments[segments.length - 1] ?? path;
}

export function LocalArtifactBlock({
  directive,
  renderMarkdown,
  fullscreenRequest = 0,
  fullscreenHostSelector,
}: {
  directive: LocalArtifactDirective;
  renderMarkdown: (source: string) => React.ReactNode;
  fullscreenRequest?: number;
  fullscreenHostSelector?: string;
}) {
  if (directive.kind === "page" && !/\.(md|markdown)$/i.test(directive.path)) {
    return <HtmlPreviewBlock text={`@file ${directive.path}`} fullscreenRequest={fullscreenRequest} />;
  }
  return (
    <LoadedLocalArtifact directive={directive.kind === "page" ? { ...directive, kind: "document" } : directive} renderMarkdown={renderMarkdown} fullscreenRequest={fullscreenRequest} fullscreenHostSelector={fullscreenHostSelector} />
  );
}

function LoadedLocalArtifact({
  directive,
  renderMarkdown,
  fullscreenRequest = 0,
  fullscreenHostSelector,
}: {
  directive: LocalArtifactDirective;
  renderMarkdown: (source: string) => React.ReactNode;
  fullscreenRequest?: number;
  fullscreenHostSelector?: string;
}) {
  const [state, setState] = useState<LoadState>({ status: "loading", file: null, error: "" });
  const [fullscreen, setFullscreen] = useState(false);
  const fullscreenRef = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setFullscreen(false), []);
  useFullscreenKeyboard(fullscreen, fullscreenRef, close);
  const sectionRef = useRef<HTMLElement>(null);
  const [bounds, setBounds] = useState<React.CSSProperties>();
  useEffect(() => { if (fullscreenRequest) setFullscreen(true); }, [fullscreenRequest]);
  useEffect(() => {
    if (!fullscreen) return;
    const host = fullscreenHostSelector ? document.querySelector(fullscreenHostSelector) : sectionRef.current?.closest("[data-tab-id]");
    if (!host) return;
    const update = () => { const rect = host.getBoundingClientRect(); setBounds({ top: rect.top, left: rect.left, width: rect.width, height: rect.height }); };
    update();
    window.addEventListener("resize", update);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    observer?.observe(host);
    return () => { observer?.disconnect(); window.removeEventListener("resize", update); };
  }, [fullscreen, fullscreenHostSelector]);
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
    return () => {
      generation.current += 1;
    };
  }, [reload]);

  const icon = directive.kind === "image"
    ? <ImageIcon size={14} aria-hidden />
    : <FileText size={14} aria-hidden />;

  const fullscreenHost = fullscreenHostSelector
    ? document.querySelector(fullscreenHostSelector)
    : sectionRef.current?.closest("[data-tab-id]");
  const content = <>
    {state.status === "loading" && <p>Loading…</p>}
    {state.status === "error" && <p role="alert">{state.error}</p>}
    {state.status === "ready" && (directive.kind === "image"
      ? <img src={state.file.data} alt={name} className="max-w-full object-contain" />
      : state.file.media_type === "text/markdown" ? renderMarkdown(state.file.data)
        : <pre className="whitespace-pre-wrap break-words">{state.file.data}</pre>)}
  </>;
  const reveal = () => void openLocalArtifact(directive.path).catch((error: unknown) => setState({ status: "error", file: null, error: String(error) }));
  return (
    <section ref={sectionRef}
      data-testid={`chat-ds-${directive.kind}`}
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
        <button type="button" onClick={() => setFullscreen(true)} aria-label={`Fullscreen ${name}`} title="Fullscreen"><Maximize2 size={13} aria-hidden /></button>
        <button type="button" onClick={reveal} aria-label={`Reveal ${name}`} title="Reveal in file manager"><FolderOpen size={13} aria-hidden /></button>
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
      <div className="max-h-[720px] overflow-auto px-3 py-2 text-sm">{content}</div>
      {fullscreen && createPortal(<FullscreenOverlay panelRef={fullscreenRef} label={`Fullscreen ${name}`} onClose={close} style={bounds}>
        <header className="flex items-center gap-2 p-2 text-sm"><span className="min-w-0 flex-1 break-words" title={directive.path}>{directive.path}</span>
          <button type="button" onClick={reveal} aria-label={`Reveal ${name}`}><FolderOpen size={15} /></button>
          <button type="button" onClick={close} aria-label="Close fullscreen"><X size={15} /></button>
        </header>
        <div className="min-h-0 flex-1 overflow-auto p-4 text-sm">{content}</div>
      </FullscreenOverlay>, fullscreenHost ?? document.body)}
    </section>
  );
}
