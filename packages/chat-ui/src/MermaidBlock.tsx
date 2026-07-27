export const MERMAID_RENDER_LENGTH_LIMIT = 120_000;
const MERMAID_CLEANUP_PASSES = 2;
export const MERMAID_FULLSCREEN_MIN_SCALE = 0.35;
export const MERMAID_FULLSCREEN_MAX_SCALE = 5;
const MERMAID_CDN_URL = "https://cdn.jsdelivr.net/npm/mermaid@10.9.1/dist/mermaid.min.js";
const MERMAID_RENDER_CACHE_LIMIT = 64;
const MERMAID_BLOCK_GUARDS = [
  "flowchart",
  "graph",
  "sequenceDiagram",
  "stateDiagram",
  "stateDiagram-v2",
  "gantt",
  "classDiagram",
  "erDiagram",
  "mindmap",
  "journey",
  "pie",
  "gitGraph",
  "quadrantChart",
  "timeline",
  "packet",
  "kanban",
];

let mermaidLoadPromise: Promise<void> | null = null;
let mermaidInitialized = false;
type CachedMermaidRender = {
  svg: string | null;
  error?: string;
  sourceId?: string;
};
const mermaidRenderCache = new Map<string, CachedMermaidRender>();

export function getCachedMermaidRender(source: string) {
  return mermaidRenderCache.get(source);
}

export function setCachedMermaidRender(source: string, value: CachedMermaidRender) {
  if (mermaidRenderCache.has(source)) {
    mermaidRenderCache.delete(source);
  }
  mermaidRenderCache.set(source, value);
  while (mermaidRenderCache.size > MERMAID_RENDER_CACHE_LIMIT) {
    const oldest = mermaidRenderCache.keys().next();
    if (oldest.done) {
      break;
    }
    mermaidRenderCache.delete(oldest.value);
  }
}

export function remapMermaidSvgSourceId(svg: string, from: string, to: string) {
  if (!from || !to || from === to) {
    return svg;
  }
  return svg.split(from).join(to);
}

export function removeMermaidRenderArtifacts(sourceId: string) {
  if (typeof document === "undefined") {
    return;
  }
  const tempIdPrefix = `d${sourceId}`;
  const elements = Array.from(document.body.querySelectorAll<HTMLElement>("[id]"));
  for (const element of elements) {
    if (!(element instanceof HTMLElement)) {
      continue;
    }
    const isOwnTempNode = element.id === tempIdPrefix || element.id.startsWith(tempIdPrefix);
    if (isOwnTempNode) {
      element.remove();
    }
  }
}

export function ensureMermaidLoaded(): Promise<void> {
  if (typeof window === "undefined") {
    return Promise.resolve();
  }
  if ((window as { mermaid?: unknown }).mermaid) {
    return Promise.resolve();
  }
  if (!mermaidLoadPromise) {
    mermaidLoadPromise = new Promise((resolve, reject) => {
      const existing = document.querySelector<HTMLScriptElement>('script[data-ct-mermaid="1"]');
      if (existing) {
        if (existing.dataset.loaded === "1") {
          resolve();
          return;
        }
        existing.addEventListener("load", () => {
          existing.dataset.loaded = "1";
          resolve();
        }, { once: true });
        existing.addEventListener("error", () => {
          mermaidLoadPromise = null;
          reject(new Error("Failed to load mermaid script"));
        }, { once: true });
        return;
      }

      const script = document.createElement("script");
      script.src = MERMAID_CDN_URL;
      script.async = true;
      script.dataset.ctMermaid = "1";
      script.onload = () => {
        script.dataset.loaded = "1";
        resolve();
      };
      script.onerror = () => {
        mermaidLoadPromise = null;
        reject(new Error("Failed to load mermaid script"));
      };
      document.head.appendChild(script);
    });
  }
  return mermaidLoadPromise;
}

export function ensureMermaidInitialized() {
  if (typeof window === "undefined") return false;
  const mermaid = (window as { mermaid?: { initialize?: (...args: unknown[]) => unknown; render?: (...args: unknown[]) => unknown } }).mermaid;
  if (!mermaid || mermaidInitialized) return true;
  // Rendered SVG is injected via innerHTML, so label sanitization must stay on.
  mermaid.initialize?.({
    startOnLoad: false,
    securityLevel: "strict",
    theme: "neutral",
  });
  mermaidInitialized = true;
  return true;
}

export function looksLikeMermaidSource(text: string): boolean {
  const firstLine = text.trim().split("\n").find((line) => line.trim() !== "");
  if (!firstLine || !text.includes("\n")) {
    return false;
  }
  return firstLine.trim().startsWith("---") || MERMAID_BLOCK_GUARDS.some((keyword) => {
    const token = firstLine.trim();
    return token === keyword || token.startsWith(`${keyword} `) || token.startsWith(`${keyword}\t`);
  });
}

function stripMermaidWrapper(text: string): string {
  let value = text.trim();
  for (let i = 0; i < MERMAID_CLEANUP_PASSES; i += 1) {
    const match = value.match(/^\s*```[a-z0-9+#-]*\s*\n([\s\S]*?)\n\s*```\s*$/i);
    if (!match) {
      break;
    }
    value = match[1];
  }
  return value.trim();
}

function stripMermaidLeadingMarker(text: string): string {
  const lines = text.split("\n");
  if (!lines.length) {
    return text;
  }
  const firstLine = lines[0].trim().toLowerCase();
  if (firstLine === "mermaid" || firstLine === "mmd") {
    lines.shift();
  }
  while (lines.length && lines[lines.length - 1].trim() === "```") {
    lines.pop();
  }
  while (lines.length && lines[0].trim() === "```") {
    lines.shift();
  }
  return lines.join("\n").trim();
}

export function getMermaidRenderCandidates(text: string): string[] {
  const base = text.replace(/\r/g, "").replace(/^\uFEFF/, "").trim();
  const candidates: string[] = [];
  const seen = new Set<string>();
  const pushCandidate = (value: string) => {
    const candidate = value.trim();
    if (!candidate || seen.has(candidate)) {
      return;
    }
    seen.add(candidate);
    candidates.push(candidate);
  };

  const withoutFences = stripMermaidWrapper(base);
  const fullyTrimmed = stripMermaidLeadingMarker(withoutFences);

  pushCandidate(base);
  pushCandidate(withoutFences);
  pushCandidate(fullyTrimmed);
  pushCandidate(stripMermaidWrapper(fullyTrimmed));
  return candidates;
}
