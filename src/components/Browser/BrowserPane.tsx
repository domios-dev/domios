import { useState, useRef, useCallback, useEffect } from "react";

function isNavigableUrl(url: string): boolean {
  const trimmed = url.trim();
  return !(!trimmed || trimmed === "about:blank");
}

function resolveUrl(url: string): string {
  const fileMatch = url.match(/^file:\/\/\/(.+)$/);
  if (fileMatch) {
    const filePath = decodeURIComponent(fileMatch[1]);
    return `http://127.0.0.1:7685/api/serve-file?path=${encodeURIComponent(filePath)}`;
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
  const resolvedInitial = initialUrl && isNavigableUrl(initialUrl) ? initialUrl : "";
  const [currentUrl, setCurrentUrl] = useState(resolvedInitial || "");
  const [inputUrl, setInputUrl] = useState(resolvedInitial || "");
  const [history, setHistory] = useState<string[]>(resolvedInitial ? [resolvedInitial] : []);
  const [historyIndex, setHistoryIndex] = useState(resolvedInitial ? 0 : -1);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [srcdoc, setSrcdoc] = useState<string | null>(null);
  const iframeRef = useRef<HTMLIFrameElement>(null);

  useEffect(() => {
    if (!currentUrl || !isNavigableUrl(currentUrl)) {
      setSrcdoc(null);
      return;
    }
    const resolved = resolveUrl(currentUrl);
    if (resolved.includes("/api/serve-file") || resolved.startsWith("http://127.0.0.1") || resolved.startsWith("http://localhost")) {
      fetch(resolved)
        .then((r) => r.text())
        .then((html) => setSrcdoc(html))
        .catch(() => setSrcdoc(null));
    } else {
      setSrcdoc(null);
    }
  }, [currentUrl]);

  const navigate = useCallback(
    (url: string) => {
      let normalized = url.trim();
      if (!normalized) return;
      if (
        !normalized.startsWith("http://") &&
        !normalized.startsWith("https://") &&
        !normalized.startsWith("data:") &&
        !normalized.startsWith("about:")
      ) {
        normalized = "https://" + normalized;
      }
      setErrorMsg(null);
      setCurrentUrl(normalized);
      setInputUrl(normalized);
      const newHistory = [...history.slice(0, historyIndex + 1), normalized];
      setHistory(newHistory);
      setHistoryIndex(newHistory.length - 1);
    },
    [history, historyIndex],
  );

  const handleBack = useCallback(() => {
    if (historyIndex > 0) {
      const i = historyIndex - 1;
      setHistoryIndex(i);
      setCurrentUrl(history[i]);
      setInputUrl(history[i]);
    }
  }, [history, historyIndex]);

  const handleForward = useCallback(() => {
    if (historyIndex < history.length - 1) {
      const i = historyIndex + 1;
      setHistoryIndex(i);
      setCurrentUrl(history[i]);
      setInputUrl(history[i]);
    }
  }, [history, historyIndex]);

  const handleReload = useCallback(() => {
    if (iframeRef.current) {
      iframeRef.current.src = resolveUrl(currentUrl);
    }
  }, [currentUrl]);

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
      <div className="flex items-center gap-1 px-2 py-1 shrink-0 bg-[var(--ct-sidebar-bg)] border-b border-[var(--ct-border)]">
        {/* Back */}
        <NavButton onClick={handleBack} disabled={!canGoBack} title="Back">
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M10 3L5 8l5 5" />
          </svg>
        </NavButton>
        {/* Forward */}
        <NavButton onClick={handleForward} disabled={!canGoForward} title="Forward">
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M6 3l5 5-5 5" />
          </svg>
        </NavButton>
        {/* Reload */}
        <NavButton onClick={handleReload} title="Reload">
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M2.5 8a5.5 5.5 0 0 1 9.3-4" />
            <path d="M13.5 8a5.5 5.5 0 0 1-9.3 4" />
            <path d="M12 2v3h-3" />
            <path d="M4 14v-3h3" />
          </svg>
        </NavButton>

        {/* URL input — Enter to navigate */}
        <input
          type="text"
          value={inputUrl}
          onChange={(e) => setInputUrl(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); navigate(inputUrl); } }}
          placeholder="Enter URL..."
          className="flex-1 bg-[var(--ct-bg)] text-[var(--ct-fg)] border border-[var(--ct-border)] rounded-md px-2.5 py-1 text-[12px] outline-none focus:border-[var(--ct-accent)] transition-colors"
          style={{ fontFamily: "inherit" }}
        />
      </div>

      {/* Content */}
      {currentUrl && isNavigableUrl(currentUrl) ? (
        <iframe
          ref={iframeRef}
          {...(srcdoc ? { srcDoc: srcdoc } : { src: resolveUrl(currentUrl) })}
          className="flex-1 w-full border-none"
          style={{ backgroundColor: "#ffffff" }}
          title={`Browser - ${currentUrl}`}
        />
      ) : (
        <div className="flex-1 flex flex-col items-center justify-center text-[var(--ct-muted)] text-sm gap-2">
          <svg width="32" height="32" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="0.8" strokeLinecap="round" className="opacity-30">
            <circle cx="8" cy="8" r="6.5" />
            <ellipse cx="8" cy="8" rx="3" ry="6.5" />
            <line x1="1.5" y1="8" x2="14.5" y2="8" />
          </svg>
          <span>{errorMsg || "Enter a URL to get started"}</span>
          <span className="text-[11px] opacity-60">
            Tip: use http://localhost:PORT for local servers
          </span>
        </div>
      )}
    </div>
  );
}

function NavButton({
  onClick,
  disabled,
  title,
  children,
}: {
  onClick: () => void;
  disabled?: boolean;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      className={`w-6 h-6 flex items-center justify-center rounded-md transition-colors ${
        disabled
          ? "text-[var(--ct-muted)]/40 cursor-default"
          : "text-[var(--ct-muted)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)]"
      }`}
    >
      {children}
    </button>
  );
}
