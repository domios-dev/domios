import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Code2, Copy, Check, Eye, Maximize2, Minimize2, RefreshCw } from "lucide-react";
import { CodeBlock, findScrollableContainer } from "./CodeBlock";
import { loadHtmlFile } from "./htmlFileLoader";

export const HTML_PREVIEW_MIN_HEIGHT = 80;
export const HTML_PREVIEW_MAX_HEIGHT = 720;
export const HTML_PREVIEW_LENGTH_LIMIT = 200_000;
const HTML_PREVIEW_MESSAGE_SOURCE = "ct-html-preview";
const HTML_PREVIEW_MAX_FILE_BYTES = 5 * 1024 * 1024;
const HTML_TAILWIND_RUNTIME = typeof __CODETERM_HTML_TAILWIND_RUNTIME__ === "string"
  ? __CODETERM_HTML_TAILWIND_RUNTIME__
  : "";
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

export type SanitizedLocalHtml = { html: string; removed: number };

export function sanitizeLocalHtml(source: string): SanitizedLocalHtml {
  const parsed = document.implementation.createHTMLDocument("");
  parsed.documentElement.innerHTML = String(source || "");
  let removed = 0;
  parsed.querySelectorAll("script,noscript,iframe,object,embed,link,meta,base").forEach((node) => {
    node.remove();
    removed += 1;
  });
  parsed.querySelectorAll("form").forEach((form) => {
    form.replaceWith(...Array.from(form.childNodes));
    removed += 1;
  });
  parsed.querySelectorAll("style").forEach((style) => {
    if (/(?:@import|url\s*\(|expression\s*\()/i.test(style.textContent || "")) {
      style.remove();
      removed += 1;
    }
  });
  const safeEmbeddedMedia = /^(?:data:(?:image\/(?!svg\+xml)|audio\/|video\/)|blob:)/i;
  parsed.querySelectorAll("*").forEach((element) => {
    Array.from(element.attributes).forEach((attribute) => {
      const name = attribute.name.toLowerCase();
      if (name.startsWith("on") || ["href", "xlink:href", "srcdoc", "srcset", "action", "formaction", "ping", "target", "download"].includes(name)) {
        element.removeAttribute(attribute.name);
        removed += 1;
        return;
      }
      if ((name === "src" || name === "poster") && !safeEmbeddedMedia.test(attribute.value.trim())) {
        element.removeAttribute(attribute.name);
        removed += 1;
        return;
      }
      if (name === "style" && /(?:@import|url\s*\(|expression\s*\(|behavior\s*:)/i.test(attribute.value)) {
        element.removeAttribute(attribute.name);
        removed += 1;
      }
    });
  });
  const styles = Array.from(parsed.head.querySelectorAll("style"), (style) => style.outerHTML).join("");
  return { html: `${styles}${parsed.body.innerHTML}`, removed };
}

const CODETERM_HTML_RUNTIME = `<script>(function(){
var clamp=function(value,min,max){return Math.min(max,Math.max(min,Number(value)||0))};
var sanitizeLocalHtml=${sanitizeLocalHtml.toString()};
var help=Object.freeze({version:1,styling:"Tailwind CSS v4 utilities, hover/focus variants, transitions and animations",animation:"ct.animate(target,keyframes,options) clamps duration, delay and iterations",files:"ct.file.pick(options) reads one user-selected file; ct.file.renderHtml(options) safely renders selected HTML without scripts, navigation, forms, or external URLs; max 5 MiB",network:"Direct network access is disabled"});
var animate=function(target,keyframes,options){var nodes=typeof target==='string'?Array.from(document.querySelectorAll(target)):target instanceof Element?[target]:Array.from(target||[]);var input=options||{};var safe={duration:clamp(input.duration||300,0,10000),delay:clamp(input.delay||0,0,2000),iterations:clamp(input.iterations||1,1,5),easing:typeof input.easing==='string'?input.easing:'ease',fill:['none','forwards','backwards','both'].includes(input.fill)?input.fill:'both'};return nodes.filter(function(node){return node&&typeof node.animate==='function'}).map(function(node){return node.animate(keyframes,safe)})};
var pick=function(options){var config=options||{};return new Promise(function(resolve,reject){var input=document.createElement('input');input.type='file';input.accept=typeof config.accept==='string'?config.accept.slice(0,256):'';input.style.display='none';input.addEventListener('change',function(){var file=input.files&&input.files[0];input.remove();if(!file){resolve(null);return}var limit=Math.min(${HTML_PREVIEW_MAX_FILE_BYTES},Math.max(1,Number(config.maxBytes)||${HTML_PREVIEW_MAX_FILE_BYTES}));if(file.size>limit){reject(new Error('Selected file exceeds '+limit+' bytes'));return}var reader=new FileReader();reader.onerror=function(){reject(new Error('Unable to read selected file'))};reader.onload=function(){resolve({name:file.name,type:file.type,size:file.size,data:reader.result})};if(config.as==='text')reader.readAsText(file);else if(config.as==='arrayBuffer')reader.readAsArrayBuffer(file);else reader.readAsDataURL(file)});document.body.appendChild(input);input.click()})};
var renderHtml=function(options){var config=options||{};return pick({accept:typeof config.accept==='string'?config.accept:'.html,text/html',as:'text',maxBytes:config.maxBytes}).then(function(file){if(!file)return null;var result=sanitizeLocalHtml(file.data);var target=document.body;if(config.target instanceof Element)target=config.target;else if(typeof config.target==='string'){try{target=document.querySelector(config.target)||document.body}catch(_){target=document.body}}target.replaceChildren();var banner=document.createElement('div');banner.setAttribute('data-ct-local-html-warning','');banner.style.cssText='display:block!important;padding:8px 10px!important;margin:0 0 10px!important;border:1px solid #d29922!important;border-radius:8px!important;background:#3b2e00!important;color:#f0c36a!important;font:600 12px/1.4 system-ui!important';banner.textContent='Local untrusted HTML · '+file.name+' · scripts and external access removed';var content=document.createElement('div');content.setAttribute('data-ct-local-html','');content.innerHTML=result.html;target.append(banner,content);return Object.freeze({name:file.name,type:file.type,size:file.size,removed:result.removed})})};
window.ct=Object.freeze({version:1,help:function(){return help},animate:animate,theme:function(){var style=getComputedStyle(document.documentElement);return Object.freeze({background:style.getPropertyValue('--ct-bg').trim(),foreground:style.getPropertyValue('--ct-fg').trim(),accent:style.getPropertyValue('--ct-accent').trim()})},file:Object.freeze({pick:pick,renderHtml:renderHtml})});
})();<\/script>`;

function inlineScript(source: string): string {
  return `<script>${source.replace(/<\/script/gi, "<\\/script")}<\/script>`;
}

type ThemeVars = { bg: string; fg: string; accent: string; muted: string; border: string };
type RectBounds = { top: number; left: number; width: number; height: number };
type FileSourceState =
  | { status: "idle" | "loading"; source: ""; error: "" }
  | { status: "ready"; source: string; error: "" }
  | { status: "error"; source: ""; error: string };

export function parseHtmlFileDirective(source: string): string | null {
  const match = /^\s*@file\s+(?:"([^"]+)"|'([^']+)'|(.+?))\s*$/s.exec(source.replace(/\r/g, ""));
  const path = (match?.[1] || match?.[2] || match?.[3] || "").trim();
  return path || null;
}

function readBounds(element: HTMLElement): RectBounds {
  const rect = element.getBoundingClientRect();
  return { top: rect.top, left: rect.left, width: rect.width, height: rect.height };
}

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

export function buildHtmlPreviewDocument(
  source: string,
  theme: ThemeVars,
  tailwindRuntime = HTML_TAILWIND_RUNTIME,
): string {
  return `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="${HTML_PREVIEW_CSP}">
<style>
:root{--ct-bg:${theme.bg};--ct-fg:${theme.fg};--ct-accent:${theme.accent};--ct-muted-fg:${theme.muted};--ct-border:${theme.border};color-scheme:light dark}
html,body{margin:0;background:${theme.bg};color:${theme.fg};font:13px/1.5 system-ui,-apple-system,Segoe UI,sans-serif}
body{padding:12px}
a{color:${theme.accent}}
</style></head><body>${source}${tailwindRuntime ? inlineScript(tailwindRuntime) : ""}${CODETERM_HTML_RUNTIME}${HEIGHT_REPORTER}</body></html>`;
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
  const filePath = useMemo(() => parseHtmlFileDirective(normalizedText), [normalizedText]);
  const [fileSource, setFileSource] = useState<FileSourceState>({ status: "idle", source: "", error: "" });
  const requestGeneration = useRef(0);
  const oversized = !filePath && normalizedText.length > HTML_PREVIEW_LENGTH_LIMIT;
  const [showCode, setShowCode] = useState(oversized);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [height, setHeight] = useState(HTML_PREVIEW_MIN_HEIGHT);
  const [copied, setCopied] = useState(false);
  const blockRef = useRef<HTMLDivElement>(null);
  const scrollHostRef = useRef<HTMLElement | null>(null);
  const [bounds, setBounds] = useState<RectBounds | null>(null);
  const theme = useMemo(() => readThemeVars(), []);
  const previewSource = useMemo(() => {
    if (!filePath) return normalizedText;
    return fileSource.status === "ready" ? sanitizeLocalHtml(fileSource.source).html : "";
  }, [filePath, fileSource, normalizedText]);
  const doc = useMemo(() => buildHtmlPreviewDocument(previewSource, theme), [previewSource, theme]);
  const lineCountValue = useMemo(() => {
    const source = fileSource.status === "ready" ? fileSource.source : normalizedText;
    return source.replace(/\n$/, "").split("\n").length;
  }, [fileSource, normalizedText]);

  const reloadFile = useCallback(async () => {
    if (!filePath) return;
    const generation = requestGeneration.current + 1;
    requestGeneration.current = generation;
    setFileSource({ status: "loading", source: "", error: "" });
    try {
      const source = await loadHtmlFile(filePath);
      if (requestGeneration.current === generation) {
        setFileSource({ status: "ready", source, error: "" });
      }
    } catch (error) {
      if (requestGeneration.current === generation) {
        setFileSource({ status: "error", source: "", error: error instanceof Error ? error.message : String(error) });
      }
    }
  }, [filePath]);

  useEffect(() => {
    if (!filePath) {
      requestGeneration.current += 1;
      setFileSource({ status: "idle", source: "", error: "" });
      return;
    }
    void reloadFile();
    window.addEventListener("focus", reloadFile);
    return () => window.removeEventListener("focus", reloadFile);
  }, [filePath, reloadFile]);

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

  const updateFullscreenBounds = useCallback(() => {
    const host = scrollHostRef.current;
    if (!host) {
      setBounds(null);
      return;
    }
    const next = readBounds(host);
    setBounds((current) => current
      && current.top === next.top
      && current.left === next.left
      && current.width === next.width
      && current.height === next.height
      ? current
      : next);
  }, []);

  useEffect(() => {
    if (!isFullscreen) {
      return;
    }
    const host = scrollHostRef.current;
    updateFullscreenBounds();
    window.addEventListener("resize", updateFullscreenBounds);
    host?.addEventListener("scroll", updateFullscreenBounds, { passive: true });
    const observer = host && typeof ResizeObserver !== "undefined"
      ? new ResizeObserver(updateFullscreenBounds)
      : null;
    if (observer && host) {
      observer.observe(host);
    }
    return () => {
      window.removeEventListener("resize", updateFullscreenBounds);
      host?.removeEventListener("scroll", updateFullscreenBounds);
      observer?.disconnect();
    };
  }, [isFullscreen, updateFullscreenBounds]);

  const openFullscreen = (event?: React.MouseEvent) => {
    event?.stopPropagation();
    const host = findScrollableContainer(blockRef.current);
    scrollHostRef.current = host;
    setBounds(host ? readBounds(host) : null);
    setIsFullscreen(true);
  };

  const closeFullscreen = (event?: React.MouseEvent) => {
    event?.stopPropagation();
    setIsFullscreen(false);
    setBounds(null);
    scrollHostRef.current = null;
  };

  const toolbar = (fullscreen: boolean) => (
    <div className="flex select-none items-center gap-1.5 px-2 py-1.5 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
      <span className="font-medium" style={{ color: "var(--ct-fg)" }}>HTML</span>
      {filePath && <span className="rounded border border-amber-500/40 px-1 text-amber-400">Local file</span>}
      <span className="opacity-70">·</span>
      <span className="opacity-70">{lineCountValue} lines</span>
      {oversized && <span className="opacity-70">· too large to preview</span>}
      <span className="ml-auto" />
      {filePath && (
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            void reloadFile();
          }}
          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
          style={{ color: "var(--ct-muted-fg)" }}
          title={filePath}
        >
          <RefreshCw size={12} className={fileSource.status === "loading" ? "animate-spin" : ""} />
          <span>Reload</span>
        </button>
      )}
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

  const body = (fill: boolean) => {
    if (filePath && fileSource.status === "loading") {
      return <div className="p-4 text-xs text-[var(--ct-muted-fg)]">Loading local HTML…</div>;
    }
    if (filePath && fileSource.status === "error") {
      return (
        <div className="border-t border-[var(--ct-border)] p-4 text-xs text-red-400" role="alert">
          {fileSource.error}
        </div>
      );
    }
    return showCode ? (
      <div className="min-h-0 overflow-auto" style={{ maxHeight: fill ? "100%" : `${HTML_PREVIEW_MAX_HEIGHT}px` }}>
        <CodeBlock text={normalizedText} language="html" />
      </div>
    ) : (
      <PreviewFrame doc={doc} height={height} onHeight={setHeight} fill={fill} />
    );
  };

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
