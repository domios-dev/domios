import { useState, useRef, useCallback, useEffect } from "react";
import { Webview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalPosition, LogicalSize } from "@tauri-apps/api/dpi";

function isNavigableUrl(url: string): boolean {
  const trimmed = url.trim();
  return !(!trimmed || trimmed === "about:blank");
}

function normalizeUrl(raw: string): string {
  let url = raw.trim();
  if (!url) return "";
  if (
    !url.startsWith("http://") &&
    !url.startsWith("https://") &&
    !url.startsWith("data:") &&
    !url.startsWith("about:") &&
    !url.startsWith("file://")
  ) {
    url = "https://" + url;
  }
  return url;
}

interface BrowserPaneProps {
  paneId: string;
  url?: string;
  isFocused?: boolean;
  onFocus?: (paneId: string) => void;
}

export function BrowserPane({
  paneId,
  url: initialUrl,
  isFocused = false,
  onFocus,
}: BrowserPaneProps) {
  const initial = initialUrl && isNavigableUrl(initialUrl) ? initialUrl : "";
  const [inputUrl, setInputUrl] = useState(initial);
  const [currentUrl, setCurrentUrl] = useState(initial);
  const [history, setHistory] = useState<string[]>(initial ? [initial] : []);
  const [historyIndex, setHistoryIndex] = useState(initial ? 0 : -1);
  const containerRef = useRef<HTMLDivElement>(null);
  const webviewRef = useRef<Webview | null>(null);
  const counterRef = useRef(0); // unique label per creation

  // Create native webview for a URL
  const createWebview = useCallback(async (url: string) => {
    const container = containerRef.current;
    if (!container || !isNavigableUrl(url)) return;

    // Destroy previous
    const prev = webviewRef.current;
    if (prev) {
      webviewRef.current = null;
      prev.close().catch(() => {});
    }

    const rect = container.getBoundingClientRect();
    counterRef.current += 1;
    const label = `browser-${paneId}-${counterRef.current}`;

    try {
      const wv = new Webview(getCurrentWindow(), label, {
        x: rect.left,
        y: rect.top,
        width: rect.width,
        height: rect.height,
        url,
        transparent: false,
      });
      webviewRef.current = wv;
    } catch (e) {
      console.error("Failed to create webview:", e);
    }
  }, [paneId]);

  // Destroy webview on unmount
  useEffect(() => {
    return () => {
      webviewRef.current?.close().catch(() => {});
      webviewRef.current = null;
    };
  }, []);

  // Create on initial URL
  useEffect(() => {
    if (currentUrl && isNavigableUrl(currentUrl)) {
      createWebview(currentUrl);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Reposition webview when container resizes
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const reposition = () => {
      const wv = webviewRef.current;
      if (!wv) return;
      const rect = container.getBoundingClientRect();
      wv.setPosition(new LogicalPosition(rect.left, rect.top)).catch(() => {});
      wv.setSize(new LogicalSize(rect.width, rect.height)).catch(() => {});
    };

    const observer = new ResizeObserver(reposition);
    observer.observe(container);
    window.addEventListener("codeterm-pane-resize", reposition);
    window.addEventListener("resize", reposition);

    return () => {
      observer.disconnect();
      window.removeEventListener("codeterm-pane-resize", reposition);
      window.removeEventListener("resize", reposition);
    };
  }, []);

  const navigate = useCallback((url: string) => {
    const normalized = normalizeUrl(url);
    if (!normalized) return;
    setCurrentUrl(normalized);
    setInputUrl(normalized);
    setHistory((prev) => {
      const newH = [...prev.slice(0, historyIndex + 1), normalized];
      setHistoryIndex(newH.length - 1);
      return newH;
    });
    createWebview(normalized);
  }, [historyIndex, createWebview]);

  const handleBack = useCallback(() => {
    if (historyIndex <= 0) return;
    const i = historyIndex - 1;
    setHistoryIndex(i);
    setCurrentUrl(history[i]);
    setInputUrl(history[i]);
    createWebview(history[i]);
  }, [history, historyIndex, createWebview]);

  const handleForward = useCallback(() => {
    if (historyIndex >= history.length - 1) return;
    const i = historyIndex + 1;
    setHistoryIndex(i);
    setCurrentUrl(history[i]);
    setInputUrl(history[i]);
    createWebview(history[i]);
  }, [history, historyIndex, createWebview]);

  const handleReload = useCallback(() => {
    if (currentUrl) createWebview(currentUrl);
  }, [currentUrl, createWebview]);

  const canGoBack = historyIndex > 0;
  const canGoForward = historyIndex < history.length - 1;

  return (
    <div
      className="w-full h-full flex flex-col"
      style={{
        borderTop: isFocused ? "1px solid var(--ct-accent)" : "1px solid transparent",
        backgroundColor: "var(--ct-bg)",
      }}
      onMouseDown={() => onFocus?.(paneId)}
    >
      {/* Navigation bar */}
      <div className="flex items-center gap-1 px-2 py-1 shrink-0 bg-[var(--ct-sidebar-bg)] border-b border-[var(--ct-border)] z-20 relative">
        <NavButton onClick={handleBack} disabled={!canGoBack} title="Back">
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d="M10 3L5 8l5 5" /></svg>
        </NavButton>
        <NavButton onClick={handleForward} disabled={!canGoForward} title="Forward">
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d="M6 3l5 5-5 5" /></svg>
        </NavButton>
        <NavButton onClick={handleReload} title="Reload">
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M2.5 8a5.5 5.5 0 0 1 9.3-4" /><path d="M13.5 8a5.5 5.5 0 0 1-9.3 4" /><path d="M12 2v3h-3" /><path d="M4 14v-3h3" />
          </svg>
        </NavButton>
        <input
          type="text"
          value={inputUrl}
          onChange={(e) => setInputUrl(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); navigate(inputUrl); } }}
          placeholder="Enter URL..."
          className="flex-1 bg-[var(--ct-bg)] text-[var(--ct-fg)] border border-[var(--ct-border)] rounded-md px-2.5 py-1 text-[12px] outline-none focus:border-[var(--ct-accent)] transition-colors"
        />
      </div>

      {/* Webview container — native webview attaches here */}
      <div ref={containerRef} className="flex-1 relative">
        {(!currentUrl || !isNavigableUrl(currentUrl)) && (
          <div className="absolute inset-0 flex flex-col items-center justify-center text-[var(--ct-muted)] text-sm gap-2">
            <svg width="32" height="32" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="0.8" strokeLinecap="round" className="opacity-30">
              <circle cx="8" cy="8" r="6.5" /><ellipse cx="8" cy="8" rx="3" ry="6.5" /><line x1="1.5" y1="8" x2="14.5" y2="8" />
            </svg>
            <span>Enter a URL to get started</span>
            <span className="text-[11px] opacity-60">Works with any website — no iframe restrictions</span>
          </div>
        )}
      </div>
    </div>
  );
}

function NavButton({ onClick, disabled, title, children }: {
  onClick: () => void; disabled?: boolean; title: string; children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick} disabled={disabled} title={title}
      className={`w-6 h-6 flex items-center justify-center rounded-md transition-colors ${
        disabled ? "text-[var(--ct-muted)]/40 cursor-default" : "text-[var(--ct-muted)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)]"
      }`}
    >{children}</button>
  );
}
