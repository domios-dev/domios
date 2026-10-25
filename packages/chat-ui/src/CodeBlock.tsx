import { BoundedMessageContent } from "./BoundedMessageContent";
import React, { useState, useMemo, useCallback, useEffect, useRef } from "react";
import { FullscreenOverlay } from "./FullscreenOverlay";
import { ChevronDown, Copy, Check, Maximize2, Minimize2 } from "lucide-react";
import { copyCodeText } from "./copyCodeText";
import { SyntaxHighlighter } from "./syntaxHighlighter";
import { themeAwarePrismStyle, SYNTAX_HIGHLIGHT_ROOT_CLASS } from "./syntaxTheme";
import { HIGHLIGHT_LINE_LIMIT, highlightSourceEligible } from "./highlightBudget";
import { inferCodeLanguage, languageFromInfoString } from "./codeLanguage";
import {
  ensureMermaidInitialized,
  ensureMermaidLoaded,
  getCachedMermaidRender,
  getMermaidRenderCandidates,
  MERMAID_RENDER_LENGTH_LIMIT,
  remapMermaidSvgSourceId,
  removeMermaidRenderArtifacts,
  setCachedMermaidRender,
} from "./MermaidBlock";
import { MermaidCanvas } from "./MermaidCanvas";

export const PRE_WRAP_CLASS = "whitespace-pre-wrap break-words [overflow-wrap:anywhere]";

export const LONG_CODE_PREVIEW_LINES = HIGHLIGHT_LINE_LIMIT;
const CODE_PREVIEW_HEAD_LINES = 8;
const CODE_PREVIEW_TAIL_LINES = 8;
export const CODE_BLOCK_DEFAULT_MAX_HEIGHT = "500px";
export const MERMAID_BLOCK_MAX_HEIGHT = "72rem";
const CODE_BLOCK_PLAIN_FALLBACK_THRESHOLD = 200_000;

let mobileMediaQuery: MediaQueryList | null = null;
let mediaQueryListener: (() => void) | null = null;
let mobileViewport = false;
const mobileViewportSubscribers = new Set<(value: boolean) => void>();
function getMobileViewport(): boolean {
  if (typeof window === "undefined") {
    return false;
  }
  if (!mobileMediaQuery) {
    mobileMediaQuery = window.matchMedia("(max-width: 768px)");
    mobileViewport = mobileMediaQuery.matches;
  }
  return mobileViewport;
}

function subscribeMobileViewport(listener: (value: boolean) => void): () => void {
  if (typeof window === "undefined") {
    listener(false);
    return () => {};
  }
  listener(getMobileViewport());
  mobileViewportSubscribers.add(listener);

  if (!mediaQueryListener && mobileMediaQuery) {
    mediaQueryListener = () => {
      mobileViewport = mobileMediaQuery!.matches;
      for (const cb of mobileViewportSubscribers) {
        cb(mobileViewport);
      }
    };
    mobileMediaQuery.addEventListener("change", mediaQueryListener);
  }

  return () => {
    mobileViewportSubscribers.delete(listener);
    if (mobileViewportSubscribers.size === 0 && mediaQueryListener && mobileMediaQuery) {
      mobileMediaQuery.removeEventListener("change", mediaQueryListener);
      mediaQueryListener = null;
      mobileMediaQuery = null;
    }
  };
}

export function lineCount(text: string): number {
  if (!text) return 0;
  return text.split("\n").length;
}

function isToolTraceLanguage(language: string): boolean {
  const normalized = language.trim().toLowerCase();
  return normalized === "tool" || normalized.endsWith("-tool");
}

type CodeContentProps = {
  fullSize: boolean;
  reading?: boolean;
  maxHeight: string;
  displayedText: string;
  language: string;
  mermaidMode: boolean;
  wrapLines?: boolean;
  showLineNumbers?: boolean;
  forcePlainText?: boolean;
  explicitHighlight?: boolean;
  mermaidSvg: string | null;
  shouldDisplayMermaid: boolean;
  showMermaidFallback: boolean;
  suppressMermaidErrorOutput: boolean;

};

const CodeBlockContent = React.memo(function CodeBlockContent({
  fullSize,
  reading = false,
  maxHeight,
  displayedText,
  language,
  mermaidMode,
  wrapLines = false,
  showLineNumbers = false,
  forcePlainText = false,
  explicitHighlight = false,
  mermaidSvg,
  shouldDisplayMermaid,
  showMermaidFallback,
  suppressMermaidErrorOutput,
}: CodeContentProps) {
  const resolvedLanguage = language !== "text" ? language : inferCodeLanguage(null, displayedText);
  if (forcePlainText) {
    return (
      <BoundedMessageContent collapsedHeight={maxHeight} unbounded={fullSize}
        className={`min-w-0 w-full max-w-full select-text ${fullSize ? "overflow-auto" : ""}`}
        style={{ maxHeight: fullSize ? "100%" : maxHeight }}
      >
        <pre
          className="m-0 w-max min-w-full whitespace-pre p-3 font-mono text-xs leading-[1.4]"
          style={{
            background: "var(--ct-bg)",
            color: "var(--ct-fg)",
            whiteSpace: wrapLines ? "pre-wrap" : "pre",
            width: wrapLines ? "100%" : undefined,
            overflowWrap: wrapLines ? "anywhere" : undefined,
          }}
        >
          {showLineNumbers ? displayedText.replace(/\n$/, "").split("\n").map((line, index) => <span key={index} className="grid grid-cols-[4ch_minmax(0,1fr)]">
            <span aria-hidden="true" className="select-none pr-2 text-right text-[var(--ct-muted-fg)]">{index + 1}</span><span>{line || " "}</span>
          </span>) : displayedText}
        </pre>
      </BoundedMessageContent>
    );
  }
  return (
    <BoundedMessageContent collapsedHeight={maxHeight} unbounded={fullSize}
      className={`${SYNTAX_HIGHLIGHT_ROOT_CLASS} min-w-0 w-full max-w-full select-text ${fullSize && !reading ? "h-full overflow-hidden" : ""}`}
      style={{ maxHeight: reading ? "none" : fullSize ? "100%" : maxHeight }}
    >
      {suppressMermaidErrorOutput ? null : shouldDisplayMermaid ? (
        <div className="px-2 py-1.5" style={{ color: "var(--ct-fg)" }} dangerouslySetInnerHTML={{ __html: mermaidSvg ?? "" }} />
      ) : (
        <>
          {showMermaidFallback && mermaidMode && (
            <div className="px-2 py-1.5 text-xs opacity-70">Rendering mermaid…</div>
          )}
          <SyntaxHighlighter
            explicitHighlight={explicitHighlight}
            language={resolvedLanguage}
            showLineNumbers={showLineNumbers}
            lineNumberStyle={{ color: "var(--ct-muted-fg)", userSelect: "none" }}
            style={themeAwarePrismStyle}
            useInlineStyles={false}
            PreTag="div"
            customStyle={{
              margin: 0,
              borderRadius: 0,
              padding: reading ? "0" : "0.75rem",
              fontSize: reading ? "0.8125rem" : "0.75rem",
              lineHeight: reading ? "1.65" : "1.4",
              background: reading ? "transparent" : "var(--ct-bg)",
              width: wrapLines ? "100%" : "max-content",
              minWidth: "100%",
              maxWidth: wrapLines ? "100%" : "none",
              whiteSpace: wrapLines ? "pre-wrap" : "pre",
              overflowWrap: wrapLines ? "anywhere" : "normal",
              wordBreak: wrapLines ? "break-word" : "normal",
            }}
            codeTagProps={{
              style: {
                fontFamily: "inherit",
                whiteSpace: wrapLines ? "pre-wrap" : "pre",
                overflowWrap: wrapLines ? "anywhere" : "normal",
                wordBreak: wrapLines ? "break-word" : "normal",
              },
            }}
          >
            {displayedText}
          </SyntaxHighlighter>
        </>
      )}
    </BoundedMessageContent>
  );
});

export function CodeBlock({
  text,
  language: infoString = "text",
  maxHeight = CODE_BLOCK_DEFAULT_MAX_HEIGHT,
  collapsible = false,
  collapseAfterLines = 20,
  wrapLines = false,
  showLineNumbers = false,
  fullscreenAside,
}: {
  text: string;
  language?: string;
  maxHeight?: string;
  collapsible?: boolean;
  collapseAfterLines?: number;
  wrapLines?: boolean;
  showLineNumbers?: boolean;
  /** Describes the code beside the fullscreen reading view (outline, ports, limits); shown when there is room. */
  fullscreenAside?: React.ReactNode;
}) {
  const language = languageFromInfoString(infoString);
  const normalizedText = useMemo(() => text.replace(/\r/g, ""), [text]);
  const codeLines = useMemo(() => normalizedText.replace(/\n$/, "").split("\n"), [normalizedText]);
  const lineCountValue = codeLines.length || 1;
  const mermaidMode = language.toLowerCase() === "mermaid";
  const canCollapse = collapsible && !mermaidMode && lineCountValue > collapseAfterLines;
  const [isMobileViewport, setIsMobileViewport] = useState(() => getMobileViewport());
  const [expanded, setExpanded] = useState(!canCollapse);
  const [richRequested, setRichRequested] = useState(false);
  const [copied, setCopied] = useState(false);
  const [mermaidError, setMermaidError] = useState<string | null>(null);
  const [mermaidSvg, setMermaidSvg] = useState<string | null>(null);
  const [mermaidLoading, setMermaidLoading] = useState(false);
  const mermaidCandidates = useMemo(() => (mermaidMode ? getMermaidRenderCandidates(normalizedText) : []), [mermaidMode, normalizedText]);
  const mermaidRenderKey = mermaidCandidates.join("\u0000");
  const [isFullscreen, setIsFullscreen] = useState(false);
  const highlightEligible = highlightSourceEligible(normalizedText, richRequested || isFullscreen);
  const shouldUsePlainRenderer = (!mermaidMode && (normalizedText.length > CODE_BLOCK_PLAIN_FALLBACK_THRESHOLD || isToolTraceLanguage(language) || !highlightEligible))
    || (mermaidMode && (mermaidCandidates[0]?.length ?? 0) > MERMAID_RENDER_LENGTH_LIMIT);
  const copyTimer = useRef<number | null>(null);
  const fullscreenRef = useRef<HTMLDivElement>(null);
  const mermaidNodeId = useMemo(() => `ct-mermaid-${Math.random().toString(36).slice(2, 11)}`, []);
  const copyText = useCallback(async (event: React.MouseEvent) => {
    event.stopPropagation();
    await copyCodeText(text);
    setCopied(true);
    if (copyTimer.current) {
      window.clearTimeout(copyTimer.current);
    }
    copyTimer.current = window.setTimeout(() => {
      setCopied(false);
    }, 1400);
  }, [text]);

  useEffect(() => {
    return subscribeMobileViewport((value) => {
      setIsMobileViewport(value);
    });
  }, []);

  useEffect(() => {
    return () => {
      if (copyTimer.current) {
        window.clearTimeout(copyTimer.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!mermaidMode || !expanded || shouldUsePlainRenderer || mermaidCandidates.length === 0) {
      return;
    }

    const primaryCandidate = mermaidCandidates[0];
    const cachedSvgCandidate = mermaidCandidates.find((candidate) => {
      const cached = getCachedMermaidRender(candidate);
      return !!cached?.svg;
    });
    if (cachedSvgCandidate) {
      const cached = getCachedMermaidRender(cachedSvgCandidate);
      const cachedSvg = cached!.sourceId
        ? remapMermaidSvgSourceId(cached!.svg as string, cached!.sourceId, mermaidNodeId)
        : cached!.svg;
      setMermaidLoading(false);
      setMermaidError(null);
      setMermaidSvg(cachedSvg);
      return;
    }

    const allCachedErrors = mermaidCandidates.every((candidate) => {
      const cached = getCachedMermaidRender(candidate);
      return cached !== undefined && cached.svg === null;
    });
    if (allCachedErrors) {
      removeMermaidRenderArtifacts(mermaidNodeId);
      const firstError = mermaidCandidates
        .map((candidate) => getCachedMermaidRender(candidate)?.error)
        .find((value) => value);
      setMermaidLoading(false);
      setMermaidError(firstError ?? "Mermaid parse error");
      setMermaidSvg(null);
      return;
    }

    let cancelled = false;
    setMermaidLoading(true);
    setMermaidError(null);
    setMermaidSvg(null);
    (async () => {
      let lastError: string | null = null;
      try {
        await ensureMermaidLoaded();
        if (cancelled) return;
        ensureMermaidInitialized();
        const mermaid = (window as {
          mermaid?: {
            render?: (...args: unknown[]) => unknown;
          };
        }).mermaid;
        if (!mermaid?.render) {
          throw new Error("Mermaid runtime not available");
        }
        for (const candidate of mermaidCandidates) {
          if (cancelled) return;
          const cached = getCachedMermaidRender(candidate);
          if (cached?.svg === null) {
            continue;
          }
          try {
            removeMermaidRenderArtifacts(mermaidNodeId);
            const rendered = (await Promise.resolve(mermaid.render(mermaidNodeId, candidate) as Promise<{ svg: string } | string>));
            removeMermaidRenderArtifacts(mermaidNodeId);
            if (cancelled) return;
            const svg = typeof rendered === "string" ? rendered : rendered.svg;
            setCachedMermaidRender(candidate, { svg, sourceId: mermaidNodeId });
            setMermaidSvg(svg);
            setMermaidError(null);
            return;
          } catch (candidateError) {
            removeMermaidRenderArtifacts(mermaidNodeId);
            const message = candidateError instanceof Error ? candidateError.message : "Mermaid parse error";
            lastError = message;
            setCachedMermaidRender(candidate, { svg: null, error: message });
          }
        }
      } catch (error) {
        lastError = error instanceof Error ? error.message : "Mermaid parse error";
      } finally {
        removeMermaidRenderArtifacts(mermaidNodeId);
        if (!cancelled) {
          setMermaidLoading(false);
          if (lastError) {
            const message = lastError;
            setMermaidError(message);
            if (primaryCandidate) {
              setCachedMermaidRender(primaryCandidate, { svg: null, error: message });
            }
          }
        }
      }
    })();

    return () => {
      cancelled = true;
      removeMermaidRenderArtifacts(mermaidNodeId);
    };
  }, [mermaidMode, expanded, shouldUsePlainRenderer, mermaidRenderKey, mermaidNodeId, mermaidCandidates.length]);

  const dismissFullscreen = useCallback(() => setIsFullscreen(false), []);

  useEffect(() => {
    if (!canCollapse) {
      setExpanded(true);
      return;
    }
    setExpanded(!isMobileViewport);
  }, [canCollapse, isMobileViewport]);

  const isCollapsed = canCollapse && !expanded;
  const displayedText = useMemo(() => {
    if (!isCollapsed || lineCountValue <= CODE_PREVIEW_HEAD_LINES + CODE_PREVIEW_TAIL_LINES + 1) {
      return normalizedText;
    }
    const head = codeLines.slice(0, CODE_PREVIEW_HEAD_LINES).join("\n");
    const tail = codeLines.slice(-CODE_PREVIEW_TAIL_LINES).join("\n");
    return `${head}\n...\n${tail}`;
  }, [isCollapsed, lineCountValue, codeLines, normalizedText]);

  const showMermaidFallback = mermaidMode && !shouldUsePlainRenderer && !mermaidError && !mermaidSvg;
  const shouldDisplayMermaid = mermaidMode && !shouldUsePlainRenderer && expanded && mermaidSvg !== null;
  const suppressMermaidErrorOutput = mermaidMode && !shouldUsePlainRenderer && mermaidError !== null && mermaidSvg === null;
  const openFullscreen = (event?: React.MouseEvent) => {
    if (event?.currentTarget instanceof HTMLElement) event.currentTarget.focus();
    event?.stopPropagation();
    setExpanded(true);
    setIsFullscreen(true);
  };

  const closeFullscreen = (event?: React.MouseEvent) => {
    event?.stopPropagation();
    setIsFullscreen(false);
  };


  return (
    <div className="min-w-0 w-full max-w-full" data-testid="chat-code-block" data-code-language={language}>
      <div className="min-w-0 w-full max-w-full overflow-hidden rounded-md border border-[var(--ct-border)]" style={{ background: "var(--ct-bg)" }}>
        <div data-testid="chat-code-toolbar" className="flex select-none items-center gap-1.5 px-2 py-1.5 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
          <span className="font-medium" style={{ color: "var(--ct-fg)" }}>Code</span>
          <span className="font-mono opacity-70">{language}</span>
          <span className="opacity-70">·</span>
          <span className="opacity-70">{lineCountValue} lines</span>
          <span className="ml-auto" />
          {canCollapse && (
            <button
              type="button"
              onClick={(event) => {
                event.stopPropagation();
                if (!expanded) setRichRequested(true);
                setExpanded((value) => !value);
              }}
              className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
              style={{ color: "var(--ct-muted-fg)" }}
            >
              <ChevronIcon expanded={expanded} />
              <span>{expanded ? "Hide" : "Show all"}</span>
            </button>
          )}
          <button
            type="button"
            onClick={copyText}
            aria-label="Copy code"
            title={copied ? "Copied" : "Copy code"}
            className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
            style={{ color: "var(--ct-muted-fg)" }}
          >
            {copied ? <Check size={12} /> : <Copy size={12} />}
            <span>{copied ? "Copied" : "Copy"}</span>
          </button>
          <button
            type="button"
            onClick={openFullscreen}
            className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
            style={{ color: "var(--ct-muted-fg)" }}
            title="Open in fullscreen"
          >
            <Maximize2 size={12} />
            <span>Fullscreen</span>
          </button>
        </div>
        {!mermaidMode && !highlightEligible && <button type="button" onClick={() => { setRichRequested(true); setExpanded(true); }}
          className="select-none px-2 py-1 text-xs" aria-label="Highlight code">Highlight code</button>}
        <CodeBlockContent
          wrapLines={wrapLines}
          showLineNumbers={showLineNumbers}
          fullSize={false}
          maxHeight={maxHeight}
          displayedText={displayedText}
          language={language}
          mermaidMode={mermaidMode}
          mermaidSvg={mermaidSvg}
          forcePlainText={shouldUsePlainRenderer}
          explicitHighlight={richRequested || isFullscreen}
          shouldDisplayMermaid={shouldDisplayMermaid}
          showMermaidFallback={showMermaidFallback}
          suppressMermaidErrorOutput={suppressMermaidErrorOutput}
        />
        {showMermaidFallback && !mermaidError && mermaidLoading && (
          <div className="px-2 pb-1.5 text-[10px] opacity-60 text-[var(--ct-muted-fg)]">Rendering mermaid diagram...</div>
        )}
        {canCollapse && isCollapsed && !mermaidMode && !mermaidLoading && (
          <div className="select-none px-2 pb-1.5 text-[10px] opacity-60" style={{ color: "var(--ct-muted-fg)" }}>
            Showing preview of a long block. Expand to see all lines.
          </div>
        )}
        {canCollapse && isCollapsed && mermaidMode && (
          <div className="select-none px-2 pb-1.5 text-[10px] opacity-60" style={{ color: "var(--ct-muted-fg)" }}>
            Showing first and last lines only. Expand to render complete diagram.
          </div>
        )}
      </div>
      {isFullscreen && (
        <FullscreenOverlay panelRef={fullscreenRef} closeButton={false} onClose={dismissFullscreen} label={mermaidMode ? "Fullscreen Mermaid diagram" : "Fullscreen code"}>
            <div
              className="flex select-none items-center gap-1.5 px-2 py-1.5 text-xs"
              style={{
                color: "var(--ct-muted-fg)",
                paddingTop: "4px",
              }}
            >
              <span className="font-medium" style={{ color: "var(--ct-fg)" }}>Code</span>
              <span className="font-mono opacity-70">{language}</span>
              <span className="opacity-70">·</span>
              <span className="opacity-70">{lineCountValue} lines</span>
              <span className="ml-auto" />
              <button
                type="button"
                onClick={copyText}
                aria-label="Copy code"
                title={copied ? "Copied" : "Copy code"}
                className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
                style={{ color: "var(--ct-muted-fg)" }}
              >
                {copied ? <Check size={12} /> : <Copy size={12} />}
                <span>{copied ? "Copied" : "Copy"}</span>
              </button>
              <button
                type="button"
                onClick={closeFullscreen}
                className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
                style={{ color: "var(--ct-muted-fg)" }}
              >
                <Minimize2 size={12} />
                <span>Close</span>
              </button>
            </div>
            <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
              {mermaidMode && shouldDisplayMermaid ? <MermaidCanvas svg={mermaidSvg!} /> : <div className="ct-code-reader" data-testid="code-reader">
                {fullscreenAside && <aside className="ct-code-reader__aside" aria-label="About this code">{fullscreenAside}</aside>}
                <div className="ct-code-reader__stage">
                  <div className="ct-code-reader__sheet">
                    <CodeBlockContent
                wrapLines={wrapLines}
                showLineNumbers
                fullSize
                reading
                maxHeight={maxHeight}
                displayedText={displayedText}
                language={language}
                mermaidMode={mermaidMode}
                mermaidSvg={mermaidSvg}
                forcePlainText={shouldUsePlainRenderer}
                explicitHighlight={richRequested || isFullscreen}
                shouldDisplayMermaid={shouldDisplayMermaid}
                showMermaidFallback={showMermaidFallback}
                suppressMermaidErrorOutput={suppressMermaidErrorOutput}
                    />
                  </div>
                </div>
              </div>}
            </div>
        </FullscreenOverlay>
      )}
    </div>
  );
}

/** Chevron icon for collapsible sections. */
export function ChevronIcon({ expanded, className = "" }: { expanded: boolean; className?: string }) {
  return (
    <ChevronDown
      className={`w-3 h-3 flex-shrink-0 transition-transform duration-150 ${expanded ? "rotate-180" : ""} ${className}`}
      strokeWidth={1.5}
    />
  );
}
