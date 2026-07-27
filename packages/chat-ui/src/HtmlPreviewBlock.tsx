import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Code2, Copy, Check, Eye, Maximize2, Minimize2 } from "lucide-react";
import { CodeBlock, findScrollableContainer } from "./CodeBlock";

export const HTML_PREVIEW_MIN_HEIGHT = 80;
export const HTML_PREVIEW_MAX_HEIGHT = 720;
export const HTML_PREVIEW_LENGTH_LIMIT = 200_000;
const HTML_PREVIEW_MESSAGE_SOURCE = "ct-html-preview";
const HTML_PREVIEW_CSP = [
  "default-src 'none'",
  "script-src 'unsafe-inline' 'unsafe-eval'",
  "style-src 'unsafe-inline'",
  "img-src data: blob:",
  "font-src data:",
  "media-src data: blob:",
  "connect-src 'none'",
  "form-action 'none'",
  "base-uri 'none'",
  "frame-src 'none'",
].join("; ");

const HEIGHT_REPORTER = `<script>(function(){
var post=function(){var d=document;var h=Math.max(d.documentElement.scrollHeight,d.body?d.body.scrollHeight:0);parent.postMessage({source:${JSON.stringify(HTML_PREVIEW_MESSAGE_SOURCE)},height:h},'*')};
window.addEventListener('load',post);
document.addEventListener('DOMContentLoaded',post);
if(window.ResizeObserver){new ResizeObserver(post).observe(document.documentElement)}
})();<\/script>`;

type ThemeVars = { bg: string; fg: string; accent: string; muted: string; border: string };

function readThemeVars(): ThemeVars {
  const fallback = { bg: "#0d1117", fg: "#c9d1d9", accent: "#58a6ff", muted: "#8b949e", border: "#30363d" };
  if (typeof window === "undefined") {
    return fallback;
  }
  const style = window.getComputedStyle(document.documentElement);
  const pick = (name: string, backup: string) => style.getPropertyValue(name).trim() || backup;
  return {
    bg: pick("--ct-bg", fallback.bg),
    fg: pick("--ct-fg", fallback.fg),
    accent: pick("--ct-accent", fallback.accent),
    muted: pick("--ct-muted-fg", fallback.muted),
    border: pick("--ct-border", fallback.border),
  };
}

export function buildHtmlPreviewDocument(source: string, theme: ThemeVars): string {
  return `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="${HTML_PREVIEW_CSP}">
<style>
:root{--ct-bg:${theme.bg};--ct-fg:${theme.fg};--ct-accent:${theme.accent};--ct-muted-fg:${theme.muted};--ct-border:${theme.border};color-scheme:light dark}
html,body{margin:0;background:${theme.bg};color:${theme.fg};font:13px/1.5 system-ui,-apple-system,Segoe UI,sans-serif}
body{padding:12px}
a{color:${theme.accent}}
</style></head><body>${source}${HEIGHT_REPORTER}</body></html>`;
}

function PreviewFrame({
  doc,
  height,
  onHeight,
  fill,
}: {
  doc: string;
  height: number;
  onHeight: (value: number) => void;
  fill: boolean;
}) {
  const frameRef = useRef<HTMLIFrameElement>(null);

  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      if (event.source !== frameRef.current?.contentWindow) {
        return;
      }
      const data = event.data as { source?: string; height?: number } | null;
      if (!data || data.source !== HTML_PREVIEW_MESSAGE_SOURCE || typeof data.height !== "number") {
        return;
      }
      onHeight(Math.min(HTML_PREVIEW_MAX_HEIGHT, Math.max(HTML_PREVIEW_MIN_HEIGHT, Math.ceil(data.height))));
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [onHeight]);

  return (
    <iframe
      ref={frameRef}
      title="HTML preview"
      data-testid="chat-html-preview-frame"
      sandbox="allow-scripts"
      referrerPolicy="no-referrer"
      srcDoc={doc}
      className="block w-full border-0"
      style={{ height: fill ? "100%" : `${height}px`, background: "var(--ct-bg)" }}
    />
  );
}

export function HtmlPreviewBlock({ text }: { text: string }) {
  const normalizedText = useMemo(() => text.replace(/\r/g, ""), [text]);
  const oversized = normalizedText.length > HTML_PREVIEW_LENGTH_LIMIT;
  const [showCode, setShowCode] = useState(oversized);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [height, setHeight] = useState(HTML_PREVIEW_MIN_HEIGHT);
  const [copied, setCopied] = useState(false);
  const blockRef = useRef<HTMLDivElement>(null);
  const scrollHostRef = useRef<HTMLElement | null>(null);
  const [bounds, setBounds] = useState<{ top: number; left: number; width: number; height: number } | null>(null);
  const theme = useMemo(() => readThemeVars(), []);
  const doc = useMemo(() => buildHtmlPreviewDocument(normalizedText, theme), [normalizedText, theme]);
  const lineCountValue = useMemo(() => normalizedText.replace(/\n$/, "").split("\n").length, [normalizedText]);

  const copySource = useCallback(async (event: React.MouseEvent) => {
    event.stopPropagation();
    try {
      await navigator.clipboard.writeText(normalizedText);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }, [normalizedText]);

  useEffect(() => {
    setCopied(false);
  }, [normalizedText]);

  useEffect(() => {
    if (typeof window === "undefined" || !isFullscreen) {
      return;
    }
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = previous;
    };
  }, [isFullscreen]);

  const openFullscreen = (event?: React.MouseEvent) => {
    event?.stopPropagation();
    const host = findScrollableContainer(blockRef.current);
    scrollHostRef.current = host;
    setBounds(host ? host.getBoundingClientRect().toJSON() : null);
    setIsFullscreen(true);
  };

  const closeFullscreen = (event?: React.MouseEvent) => {
    event?.stopPropagation();
    setIsFullscreen(false);
    setBounds(null);
    scrollHostRef.current = null;
  };

  const toolbar = (fullscreen: boolean) => (
    <div className="flex items-center gap-1.5 px-2 py-1.5 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
      <span className="font-medium" style={{ color: "var(--ct-fg)" }}>HTML</span>
      <span className="opacity-70">·</span>
      <span className="opacity-70">{lineCountValue} lines</span>
      {oversized && <span className="opacity-70">· too large to preview</span>}
      <span className="ml-auto" />
      {!oversized && (
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            setShowCode((value) => !value);
          }}
          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
          style={{ color: "var(--ct-muted-fg)" }}
        >
          {showCode ? <Eye size={12} /> : <Code2 size={12} />}
          <span>{showCode ? "Preview" : "Code"}</span>
        </button>
      )}
      <button
        type="button"
        onClick={copySource}
        className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
        style={{ color: "var(--ct-muted-fg)" }}
      >
        {copied ? <Check size={12} /> : <Copy size={12} />}
        <span>{copied ? "Copied" : "Copy"}</span>
      </button>
      <button
        type="button"
        onClick={fullscreen ? closeFullscreen : openFullscreen}
        className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
        style={{ color: "var(--ct-muted-fg)" }}
        title={fullscreen ? "Close fullscreen" : "Open in fullscreen"}
      >
        {fullscreen ? <Minimize2 size={12} /> : <Maximize2 size={12} />}
        <span>{fullscreen ? "Close" : "Fullscreen"}</span>
      </button>
    </div>
  );

  const body = (fill: boolean) =>
    showCode ? (
      <div className="min-h-0 overflow-auto" style={{ maxHeight: fill ? "100%" : `${HTML_PREVIEW_MAX_HEIGHT}px` }}>
        <CodeBlock text={normalizedText} language="html" />
      </div>
    ) : (
      <PreviewFrame doc={doc} height={height} onHeight={setHeight} fill={fill} />
    );

  return (
    <div ref={blockRef} className="min-w-0 w-full max-w-full" data-testid="chat-html-block">
      <div
        className="min-w-0 w-full max-w-full overflow-hidden rounded-md border border-[var(--ct-border)]"
        style={{ background: "var(--ct-bg)" }}
      >
        {toolbar(false)}
        {body(false)}
      </div>
      {isFullscreen && (
        <div
          className="fixed z-50 bg-black/85 backdrop-blur-sm"
          style={bounds ? { top: bounds.top, left: bounds.left, width: bounds.width, height: bounds.height } : { inset: 0 }}
          onClick={closeFullscreen}
        >
          <div
            className="mx-auto flex h-full w-full flex-col border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2 shadow-2xl"
            onClick={(event) => event.stopPropagation()}
          >
            {toolbar(true)}
            <div className="min-h-0 flex-1 overflow-hidden">{body(true)}</div>
          </div>
        </div>
      )}
    </div>
  );
}
