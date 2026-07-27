import React, { useState, useMemo, useCallback, useEffect, useRef } from "react";
import { ChevronDown, Copy, Check, Maximize2, Minimize2 } from "lucide-react";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { themeAwarePrismStyle, SYNTAX_HIGHLIGHT_ROOT_CLASS } from "./syntaxTheme";
import { inferCodeLanguage } from "./codeLanguage";
import {
  ensureMermaidInitialized,
  ensureMermaidLoaded,
  getCachedMermaidRender,
  getMermaidRenderCandidates,
  MERMAID_FULLSCREEN_MAX_SCALE,
  MERMAID_FULLSCREEN_MIN_SCALE,
  MERMAID_RENDER_LENGTH_LIMIT,
  remapMermaidSvgSourceId,
  removeMermaidRenderArtifacts,
  setCachedMermaidRender,
} from "./MermaidBlock";

export const PRE_WRAP_CLASS = "whitespace-pre-wrap break-words [overflow-wrap:anywhere]";

export const LONG_CODE_PREVIEW_LINES = 300;
const CODE_PREVIEW_HEAD_LINES = 8;
const CODE_PREVIEW_TAIL_LINES = 8;
export const CODE_BLOCK_DEFAULT_MAX_HEIGHT = "500px";
export const MERMAID_BLOCK_MAX_HEIGHT = "72rem";
const CODE_BLOCK_PLAIN_FALLBACK_THRESHOLD = 200_000;

let mobileMediaQuery: MediaQueryList | null = null;
let mediaQueryListener: (() => void) | null = null;
let mobileViewport = false;
const mobileViewportSubscribers = new Set<(value: boolean) => void>();
type RectLike = {
  top: number;
  left: number;
  width: number;
  height: number;
};
type PointLike = {
  x: number;
  y: number;
};
type MermaidGesture = {
  distance: number;
  midpoint: PointLike;
  pan: PointLike;
  scale: number;
};

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

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function distanceBetween(a: PointLike, b: PointLike) {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

function midpointBetween(a: PointLike, b: PointLike): PointLike {
  return {
    x: (a.x + b.x) / 2,
    y: (a.y + b.y) / 2,
  };
}

export function findScrollableContainer(start: HTMLElement | null): HTMLElement | null {
  if (typeof window === "undefined" || !start) {
    return null;
  }
  let current: HTMLElement | null = start.parentElement;
  while (current) {
    const style = window.getComputedStyle(current);
    const overflowY = style.overflowY;
    const overflowX = style.overflowX;
    const hasYScroll = overflowY === "auto" || overflowY === "scroll" || overflowY === "overlay";
    const hasXScroll = overflowX === "auto" || overflowX === "scroll" || overflowX === "overlay";
    if (hasYScroll || hasXScroll) {
      return current;
    }
    current = current.parentElement;
  }
  return null;
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
  maxHeight: string;
  displayedText: string;
  language: string;
  mermaidMode: boolean;
  forcePlainText?: boolean;
  mermaidSvg: string | null;
  shouldDisplayMermaid: boolean;
  showMermaidFallback: boolean;
  suppressMermaidErrorOutput: boolean;
  interactiveMermaid?: boolean;
  mermaidPan?: PointLike;
  mermaidScale?: number;
  onMermaidPointerDown?: React.PointerEventHandler<HTMLDivElement>;
  onMermaidPointerMove?: React.PointerEventHandler<HTMLDivElement>;
  onMermaidPointerUp?: React.PointerEventHandler<HTMLDivElement>;
  onMermaidWheel?: React.WheelEventHandler<HTMLDivElement>;
  onMermaidDoubleClick?: React.MouseEventHandler<HTMLDivElement>;
};

const CodeBlockContent = React.memo(function CodeBlockContent({
  fullSize,
  maxHeight,
  displayedText,
  language,
  mermaidMode,
  forcePlainText = false,
  mermaidSvg,
  shouldDisplayMermaid,
  showMermaidFallback,
  suppressMermaidErrorOutput,
  interactiveMermaid = false,
  mermaidPan,
  mermaidScale = 1,
  onMermaidPointerDown,
  onMermaidPointerMove,
  onMermaidPointerUp,
  onMermaidWheel,
  onMermaidDoubleClick,
}: CodeContentProps) {
  const resolvedLanguage = language !== "text" ? language : inferCodeLanguage(null, displayedText);
  if (forcePlainText) {
    return (
      <div
        className="min-w-0 w-full max-w-full overflow-auto"
        style={{ maxHeight: fullSize ? "100%" : maxHeight }}
      >
        <pre
          className="m-0 w-max min-w-full whitespace-pre p-3 font-mono text-xs leading-[1.4]"
          style={{
            background: "var(--ct-bg)",
            color: "var(--ct-fg)",
            whiteSpace: "pre",
          }}
        >
          {displayedText}
        </pre>
      </div>
    );
  }
  return (
    <div
      className={`${SYNTAX_HIGHLIGHT_ROOT_CLASS} min-w-0 w-full max-w-full overflow-x-auto overflow-y-auto`}
      style={{ maxHeight: fullSize ? "100%" : maxHeight }}
    >
      {suppressMermaidErrorOutput ? null : shouldDisplayMermaid ? (
        <div
          className="px-2 py-1.5"
          style={{
            color: "var(--ct-fg)",
            cursor: interactiveMermaid ? "grab" : undefined,
            touchAction: interactiveMermaid ? "none" : undefined,
          }}
          onPointerDown={onMermaidPointerDown}
          onPointerMove={onMermaidPointerMove}
          onPointerUp={onMermaidPointerUp}
          onPointerCancel={onMermaidPointerUp}
          onWheel={onMermaidWheel}
          onDoubleClick={onMermaidDoubleClick}
        >
          <div
            className="w-full overflow-auto flex items-center justify-center"
            style={{
              background: "var(--ct-surface)",
              minHeight: "max(100%, 0px)",
            }}
          >
            <div
              style={{
                transform: interactiveMermaid
                  ? `translate(${mermaidPan?.x ?? 0}px, ${mermaidPan?.y ?? 0}px) scale(${mermaidScale})`
                  : undefined,
                transformOrigin: "center center",
                transition: interactiveMermaid ? "transform 80ms linear" : undefined,
              }}
              dangerouslySetInnerHTML={{ __html: mermaidSvg ?? "" }}
            />
          </div>
        </div>
      ) : (
        <>
          {showMermaidFallback && mermaidMode && (
            <div className="px-2 py-1.5 text-xs opacity-70">Rendering mermaid…</div>
          )}
          <SyntaxHighlighter
            language={resolvedLanguage}
            style={themeAwarePrismStyle}
            useInlineStyles={false}
            PreTag="div"
            customStyle={{
              margin: 0,
              borderRadius: 0,
              padding: "0.75rem",
              fontSize: "0.75rem",
              lineHeight: "1.4",
              background: fullSize ? "var(--ct-bg)" : "var(--ct-bg)",
              width: "max-content",
              minWidth: "100%",
              maxWidth: "none",
              whiteSpace: "pre",
              overflowWrap: "normal",
              wordBreak: "normal",
            }}
            codeTagProps={{
              style: {
                fontFamily: "inherit",
                whiteSpace: "pre",
                overflowWrap: "normal",
                wordBreak: "normal",
              },
            }}
          >
            {displayedText}
          </SyntaxHighlighter>
        </>
      )}
    </div>
  );
});

export function CodeBlock({
  text,
  language = "text",
  maxHeight = CODE_BLOCK_DEFAULT_MAX_HEIGHT,
  collapsible = false,
  collapseAfterLines = 20,
}: {
  text: string;
  language?: string;
  maxHeight?: string;
  collapsible?: boolean;
  collapseAfterLines?: number;
}) {
  const normalizedText = useMemo(() => text.replace(/\r/g, ""), [text]);
  const codeLines = useMemo(() => normalizedText.replace(/\n$/, "").split("\n"), [normalizedText]);
  const lineCountValue = codeLines.length || 1;
  const mermaidMode = language.toLowerCase() === "mermaid";
  const canCollapse = collapsible && !mermaidMode && lineCountValue > collapseAfterLines;
  const [isMobileViewport, setIsMobileViewport] = useState(() => getMobileViewport());
  const [expanded, setExpanded] = useState(!canCollapse);
  const [copied, setCopied] = useState(false);
  const [mermaidError, setMermaidError] = useState<string | null>(null);
  const [mermaidSvg, setMermaidSvg] = useState<string | null>(null);
  const [mermaidLoading, setMermaidLoading] = useState(false);
  const mermaidCandidates = useMemo(() => (mermaidMode ? getMermaidRenderCandidates(normalizedText) : []), [mermaidMode, normalizedText]);
  const mermaidRenderKey = mermaidCandidates.join("\u0000");
  const shouldUsePlainRenderer = (!mermaidMode && (normalizedText.length > CODE_BLOCK_PLAIN_FALLBACK_THRESHOLD || isToolTraceLanguage(language)))
    || (mermaidMode && (mermaidCandidates[0]?.length ?? 0) > MERMAID_RENDER_LENGTH_LIMIT);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [fullscreenBounds, setFullscreenBounds] = useState<RectLike | null>(null);
  const copyTimer = useRef<number | null>(null);
  const fullscreenBoundsRaf = useRef<number | null>(null);
  const blockRef = useRef<HTMLDivElement>(null);
  const scrollHostRef = useRef<HTMLElement | null>(null);
  const mermaidPointerRef = useRef<Map<number, PointLike>>(new Map());
  const mermaidGestureRef = useRef<MermaidGesture | null>(null);
  const mermaidNodeId = useMemo(() => `ct-mermaid-${Math.random().toString(36).slice(2, 11)}`, []);
  const [mermaidPan, setMermaidPan] = useState<PointLike>({ x: 0, y: 0 });
  const [mermaidScale, setMermaidScale] = useState(1);
  const copyText = useCallback(async (event: React.MouseEvent) => {
    event.stopPropagation();
    try {
      await navigator.clipboard.writeText(normalizedText);
    } catch {
      const fallback = document.createElement("textarea");
      fallback.value = normalizedText;
      fallback.style.position = "fixed";
      fallback.style.opacity = "0";
      document.body.appendChild(fallback);
      fallback.select();
      document.execCommand("copy");
      document.body.removeChild(fallback);
    }
    setCopied(true);
    if (copyTimer.current) {
      window.clearTimeout(copyTimer.current);
    }
    copyTimer.current = window.setTimeout(() => {
      setCopied(false);
    }, 1400);
  }, [normalizedText]);

  useEffect(() => {
    return subscribeMobileViewport((value) => {
      setIsMobileViewport(value);
    });
  }, []);

  const updateFullscreenBounds = useCallback(() => {
    const host = scrollHostRef.current;
    if (!host) {
      return;
    }
    const rect = host.getBoundingClientRect();
    setFullscreenBounds((previous) => {
      if (
        previous &&
        previous.top === rect.top &&
        previous.left === rect.left &&
        previous.width === rect.width &&
        previous.height === rect.height
      ) {
        return previous;
      }
      return {
        top: rect.top,
        left: rect.left,
        width: rect.width,
        height: rect.height,
      };
    });
  }, []);

  const scheduleBoundsUpdate = useCallback(() => {
    if (fullscreenBoundsRaf.current !== null) {
      return;
    }
    fullscreenBoundsRaf.current = window.requestAnimationFrame(() => {
      fullscreenBoundsRaf.current = null;
      updateFullscreenBounds();
    });
  }, [updateFullscreenBounds]);

  useEffect(() => {
    return () => {
      if (copyTimer.current) {
        window.clearTimeout(copyTimer.current);
      }
      if (fullscreenBoundsRaf.current !== null) {
        window.cancelAnimationFrame(fullscreenBoundsRaf.current);
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

  useEffect(() => {
    if (!isFullscreen) {
      return;
    }
    const host = scrollHostRef.current;
    if (!host) {
      return;
    }
    const onScrollOrResize = () => {
      scheduleBoundsUpdate();
    };
    host.addEventListener("scroll", onScrollOrResize, { passive: true });
    window.addEventListener("resize", onScrollOrResize);
    updateFullscreenBounds();
    return () => {
      host.removeEventListener("scroll", onScrollOrResize);
      window.removeEventListener("resize", onScrollOrResize);
    };
  }, [isFullscreen, scheduleBoundsUpdate, updateFullscreenBounds]);

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
  const needsFullscreenSafeTop = !fullscreenBounds || fullscreenBounds.top <= 0;
  const fullscreenHostStyle = fullscreenBounds
    ? {
      top: `${fullscreenBounds.top}px`,
      left: `${fullscreenBounds.left}px`,
      width: `${fullscreenBounds.width}px`,
      height: `${fullscreenBounds.height}px`,
    }
    : undefined;

  const resetMermaidView = useCallback(() => {
    mermaidPointerRef.current.clear();
    mermaidGestureRef.current = null;
    setMermaidPan({ x: 0, y: 0 });
    setMermaidScale(1);
  }, []);

  const openFullscreen = (event?: React.MouseEvent) => {
    event?.stopPropagation();
    if (mermaidMode) {
      resetMermaidView();
    }
    setExpanded(true);
    const host = findScrollableContainer(blockRef.current);
    scrollHostRef.current = host;
    if (host) {
      const rect = host.getBoundingClientRect();
      setFullscreenBounds({
        top: rect.top,
        left: rect.left,
        width: rect.width,
        height: rect.height,
      });
    } else {
      setFullscreenBounds(null);
    }
    setIsFullscreen(true);
  };

  const closeFullscreen = (event?: React.MouseEvent) => {
    event?.stopPropagation();
    if (mermaidMode) {
      resetMermaidView();
    }
    setIsFullscreen(false);
    setFullscreenBounds(null);
    scrollHostRef.current = null;
  };

  const startMermaidGesture = useCallback(() => {
    const points = Array.from(mermaidPointerRef.current.values());
    if (points.length >= 2) {
      const [first, second] = points;
      mermaidGestureRef.current = {
        distance: distanceBetween(first, second),
        midpoint: midpointBetween(first, second),
        pan: mermaidPan,
        scale: mermaidScale,
      };
      return;
    }
    if (points.length === 1) {
      mermaidGestureRef.current = {
        distance: 0,
        midpoint: points[0],
        pan: mermaidPan,
        scale: mermaidScale,
      };
      return;
    }
    mermaidGestureRef.current = null;
  }, [mermaidPan, mermaidScale]);

  const handleMermaidPointerDown = useCallback<React.PointerEventHandler<HTMLDivElement>>((event) => {
    if (!isFullscreen || !mermaidMode || !shouldDisplayMermaid) {
      return;
    }
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    mermaidPointerRef.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    startMermaidGesture();
  }, [isFullscreen, mermaidMode, shouldDisplayMermaid, startMermaidGesture]);

  const handleMermaidPointerMove = useCallback<React.PointerEventHandler<HTMLDivElement>>((event) => {
    if (!isFullscreen || !mermaidMode || !mermaidPointerRef.current.has(event.pointerId)) {
      return;
    }
    event.preventDefault();
    mermaidPointerRef.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    const gesture = mermaidGestureRef.current;
    const points = Array.from(mermaidPointerRef.current.values());
    if (!gesture || points.length === 0) {
      return;
    }

    if (points.length >= 2 && gesture.distance > 0) {
      const [first, second] = points;
      const distance = distanceBetween(first, second);
      const midpoint = midpointBetween(first, second);
      setMermaidScale(clamp(
        gesture.scale * (distance / gesture.distance),
        MERMAID_FULLSCREEN_MIN_SCALE,
        MERMAID_FULLSCREEN_MAX_SCALE,
      ));
      setMermaidPan({
        x: gesture.pan.x + midpoint.x - gesture.midpoint.x,
        y: gesture.pan.y + midpoint.y - gesture.midpoint.y,
      });
      return;
    }

    const point = points[0];
    setMermaidPan({
      x: gesture.pan.x + point.x - gesture.midpoint.x,
      y: gesture.pan.y + point.y - gesture.midpoint.y,
    });
  }, [isFullscreen, mermaidMode]);

  const handleMermaidPointerUp = useCallback<React.PointerEventHandler<HTMLDivElement>>((event) => {
    if (!mermaidPointerRef.current.has(event.pointerId)) {
      return;
    }
    event.preventDefault();
    mermaidPointerRef.current.delete(event.pointerId);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    startMermaidGesture();
  }, [startMermaidGesture]);

  const handleMermaidWheel = useCallback<React.WheelEventHandler<HTMLDivElement>>((event) => {
    if (!isFullscreen || !mermaidMode || !shouldDisplayMermaid) {
      return;
    }
    event.preventDefault();
    const rect = event.currentTarget.getBoundingClientRect();
    const originX = event.clientX - rect.left - rect.width / 2;
    const originY = event.clientY - rect.top - rect.height / 2;
    const direction = event.deltaY < 0 ? 1.12 : 0.88;
    setMermaidScale((currentScale) => {
      const nextScale = clamp(currentScale * direction, MERMAID_FULLSCREEN_MIN_SCALE, MERMAID_FULLSCREEN_MAX_SCALE);
      const ratio = nextScale / currentScale;
      setMermaidPan((currentPan) => ({
        x: originX - (originX - currentPan.x) * ratio,
        y: originY - (originY - currentPan.y) * ratio,
      }));
      return nextScale;
    });
  }, [isFullscreen, mermaidMode, shouldDisplayMermaid]);

  const fullscreenHeaderTopPadding = needsFullscreenSafeTop
    ? "calc(0.5rem + var(--ct-safe-top, 0px))"
    : undefined;

  return (
    <div ref={blockRef} className="min-w-0 w-full max-w-full" data-testid="chat-code-block" data-code-language={language}>
      <div className="min-w-0 w-full max-w-full overflow-hidden rounded-md border border-[var(--ct-border)]" style={{ background: "var(--ct-bg)" }}>
        <div className="flex items-center gap-1.5 px-2 py-1.5 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
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
        <CodeBlockContent
          fullSize={false}
          maxHeight={maxHeight}
          displayedText={displayedText}
          language={language}
          mermaidMode={mermaidMode}
          mermaidSvg={mermaidSvg}
          forcePlainText={shouldUsePlainRenderer}
          shouldDisplayMermaid={shouldDisplayMermaid}
          showMermaidFallback={showMermaidFallback}
          suppressMermaidErrorOutput={suppressMermaidErrorOutput}
        />
        {showMermaidFallback && !mermaidError && mermaidLoading && (
          <div className="px-2 pb-1.5 text-[10px] opacity-60 text-[var(--ct-muted-fg)]">Rendering mermaid diagram...</div>
        )}
        {canCollapse && isCollapsed && !mermaidMode && !mermaidLoading && (
          <div className="px-2 pb-1.5 text-[10px] opacity-60" style={{ color: "var(--ct-muted-fg)" }}>
            Showing preview of a long block. Expand to see all lines.
          </div>
        )}
        {canCollapse && isCollapsed && mermaidMode && (
          <div className="px-2 pb-1.5 text-[10px] opacity-60" style={{ color: "var(--ct-muted-fg)" }}>
            Showing first and last lines only. Expand to render complete diagram.
          </div>
        )}
      </div>
      {isFullscreen && (
        <div
          className="fixed z-50 bg-black/85 backdrop-blur-sm"
          style={fullscreenHostStyle ?? { inset: "0px" }}
          onClick={closeFullscreen}
        >
          <div
            className="mx-auto flex h-full w-full flex-col border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2 shadow-2xl"
            onClick={(event) => event.stopPropagation()}
          >
            <div
              className="flex items-center gap-1.5 px-2 py-1.5 text-xs"
              style={{
                color: "var(--ct-muted-fg)",
                paddingTop: fullscreenHeaderTopPadding,
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
            <div className="min-h-0 flex-1 overflow-hidden">
              <CodeBlockContent
                fullSize
                maxHeight={maxHeight}
                displayedText={displayedText}
                language={language}
                mermaidMode={mermaidMode}
                mermaidSvg={mermaidSvg}
                forcePlainText={shouldUsePlainRenderer}
                shouldDisplayMermaid={shouldDisplayMermaid}
                showMermaidFallback={showMermaidFallback}
                suppressMermaidErrorOutput={suppressMermaidErrorOutput}
                interactiveMermaid={mermaidMode}
                mermaidPan={mermaidPan}
                mermaidScale={mermaidScale}
                onMermaidPointerDown={handleMermaidPointerDown}
                onMermaidPointerMove={handleMermaidPointerMove}
                onMermaidPointerUp={handleMermaidPointerUp}
                onMermaidWheel={handleMermaidWheel}
                onMermaidDoubleClick={resetMermaidView}
              />
            </div>
          </div>
        </div>
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
