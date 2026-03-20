import { useState, useRef, useCallback, useEffect } from "react";

/** Check if a URL is a real navigable URL (not blank/placeholder) */
function isNavigableUrl(url: string): boolean {
  const trimmed = url.trim();
  if (!trimmed || trimmed === "about:blank") return false;
  return true;
}

/** Convert file:// URLs to serve via REST API (iframes can't load file:// cross-origin) */
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

  // Fetch HTML content for local/serve-file URLs and use srcdoc
  // This avoids cross-origin iframe restrictions in WebView2
  useEffect(() => {
    if (!currentUrl || !isNavigableUrl(currentUrl)) {
      setSrcdoc(null);
      return;
    }
    const resolved = resolveUrl(currentUrl);
    // Only fetch for local/serve-file URLs
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

      // Add protocol if missing
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

      // Trim forward history and push
      const newHistory = [...history.slice(0, historyIndex + 1), normalized];
      setHistory(newHistory);
      setHistoryIndex(newHistory.length - 1);
    },
    [history, historyIndex],
  );

  const handleGo = useCallback(() => {
    navigate(inputUrl);
  }, [inputUrl, navigate]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleGo();
      }
    },
    [handleGo],
  );

  const handleBack = useCallback(() => {
    if (historyIndex > 0) {
      const newIndex = historyIndex - 1;
      setHistoryIndex(newIndex);
      setCurrentUrl(history[newIndex]);
      setInputUrl(history[newIndex]);
    }
  }, [history, historyIndex]);

  const handleForward = useCallback(() => {
    if (historyIndex < history.length - 1) {
      const newIndex = historyIndex + 1;
      setHistoryIndex(newIndex);
      setCurrentUrl(history[newIndex]);
      setInputUrl(history[newIndex]);
    }
  }, [history, historyIndex]);

  const handleReload = useCallback(() => {
    if (iframeRef.current) {
      iframeRef.current.src = resolveUrl(currentUrl);
    }
  }, [currentUrl]);

  const handleClick = useCallback(() => {
    onFocus?.(paneId);
  }, [onFocus, paneId]);

  const borderColor = isFocused ? "var(--ct-accent, #1f6feb)" : "transparent";

  return (
    <div
      className="w-full h-full flex flex-col"
      style={{
        boxSizing: "border-box",
        border: `1px solid ${borderColor}`,
        backgroundColor: "var(--ct-bg, #0d1117)",
      }}
      onMouseDown={handleClick}
    >
      {/* URL bar */}
      <div
        className="flex items-center gap-1 px-2 py-1 shrink-0"
        style={{
          backgroundColor: "var(--ct-sidebar-bg, #161b22)",
          borderBottom: "1px solid var(--ct-border, #30363d)",
        }}
      >
        {/* Back */}
        <button
          onClick={handleBack}
          disabled={historyIndex <= 0}
          title="Back"
          style={{
            background: "none",
            border: "none",
            color: historyIndex > 0 ? "var(--ct-fg, #c9d1d9)" : "var(--ct-muted, #484f58)",
            cursor: historyIndex > 0 ? "pointer" : "default",
            fontSize: "14px",
            padding: "2px 6px",
            borderRadius: "4px",
          }}
        >
          &#9664;
        </button>

        {/* Forward */}
        <button
          onClick={handleForward}
          disabled={historyIndex >= history.length - 1}
          title="Forward"
          style={{
            background: "none",
            border: "none",
            color:
              historyIndex < history.length - 1
                ? "var(--ct-fg, #c9d1d9)"
                : "var(--ct-muted, #484f58)",
            cursor: historyIndex < history.length - 1 ? "pointer" : "default",
            fontSize: "14px",
            padding: "2px 6px",
            borderRadius: "4px",
          }}
        >
          &#9654;
        </button>

        {/* Reload */}
        <button
          onClick={handleReload}
          title="Reload"
          style={{
            background: "none",
            border: "none",
            color: "var(--ct-fg, #c9d1d9)",
            cursor: "pointer",
            fontSize: "14px",
            padding: "2px 6px",
            borderRadius: "4px",
          }}
        >
          &#8635;
        </button>

        {/* URL input */}
        <input
          type="text"
          value={inputUrl}
          onChange={(e) => setInputUrl(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Enter URL..."
          style={{
            flex: 1,
            background: "var(--ct-bg, #0d1117)",
            color: "var(--ct-fg, #c9d1d9)",
            border: "1px solid var(--ct-border, #30363d)",
            borderRadius: "4px",
            padding: "4px 8px",
            fontSize: "13px",
            outline: "none",
            fontFamily: "inherit",
          }}
        />

        {/* Go */}
        <button
          onClick={handleGo}
          title="Go"
          style={{
            background: "var(--ct-accent, #1f6feb)",
            border: "none",
            color: "#ffffff",
            cursor: "pointer",
            fontSize: "12px",
            padding: "4px 10px",
            borderRadius: "4px",
            fontWeight: 600,
          }}
        >
          Go
        </button>
      </div>

      {/* Content area: iframe or placeholder */}
      {currentUrl && isNavigableUrl(currentUrl) ? (
        <iframe
          ref={iframeRef}
          {...(srcdoc ? { srcDoc: srcdoc } : { src: resolveUrl(currentUrl) })}
          style={{
            flex: 1,
            width: "100%",
            border: "none",
            backgroundColor: "#ffffff",
          }}
          title={`Browser - ${currentUrl}`}
        />
      ) : (
        <div
          style={{
            flex: 1,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--ct-muted, #484f58)",
            fontSize: "14px",
            gap: "8px",
          }}
        >
          <span style={{ fontSize: "32px", opacity: 0.5 }}>&#127760;</span>
          <span>{errorMsg || "Enter a URL above to get started"}</span>
          <span style={{ fontSize: "12px", opacity: 0.7 }}>
            Tip: use http://localhost:PORT for local servers
          </span>
        </div>
      )}
    </div>
  );
}
