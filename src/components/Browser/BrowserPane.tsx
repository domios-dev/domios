import { useState, useRef, useCallback } from "react";

/** Convert file:// URLs to serve via REST API */
function resolveUrl(url: string): string {
  const fileMatch = url.match(/^file:\/\/\/(.+)$/);
  if (fileMatch) {
    return `http://127.0.0.1:7685/api/serve-file?path=${encodeURIComponent(decodeURIComponent(fileMatch[1]))}`;
  }
  return url;
}

interface BrowserPaneProps {
  paneId: string;
  url?: string;
  isFocused?: boolean;
  onFocus?: (paneId: string) => void;
}

export function BrowserPane({ paneId, url: initialUrl, isFocused = false, onFocus }: BrowserPaneProps) {
  const [inputUrl, setInputUrl] = useState(initialUrl || "");
  const [currentUrl, setCurrentUrl] = useState(initialUrl || "");
  const iframeRef = useRef<HTMLIFrameElement>(null);

  const navigate = useCallback((raw: string) => {
    let url = raw.trim();
    if (!url) return;
    if (!url.startsWith("http") && !url.startsWith("data:") && !url.startsWith("file://")) {
      url = "http://" + url;
    }
    setCurrentUrl(url);
    setInputUrl(url);
  }, []);

  const handleReload = useCallback(() => {
    if (iframeRef.current && currentUrl) {
      iframeRef.current.src = resolveUrl(currentUrl);
    }
  }, [currentUrl]);

  const hasContent = currentUrl && currentUrl !== "about:blank";

  return (
    <div
      className="w-full h-full flex flex-col"
      style={{
        borderTop: isFocused ? "1px solid var(--ct-accent)" : "1px solid transparent",
        backgroundColor: "var(--ct-bg)",
      }}
      onMouseDown={() => onFocus?.(paneId)}
    >
      {/* URL bar */}
      <div className="flex items-center gap-1 px-2 py-1 shrink-0 bg-[var(--ct-sidebar-bg)] border-b border-[var(--ct-border)]">
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
          placeholder="localhost:3000, data:text/html,..."
          className="flex-1 bg-[var(--ct-bg)] text-[var(--ct-fg)] border border-[var(--ct-border)] rounded-md px-2.5 py-1 text-[12px] outline-none focus:border-[var(--ct-accent)] transition-colors"
        />
      </div>

      {/* Content */}
      {hasContent ? (
        <iframe
          ref={iframeRef}
          src={resolveUrl(currentUrl)}
          className="flex-1 w-full border-none"
          style={{ backgroundColor: "#ffffff" }}
          title="Canvas"
          sandbox="allow-same-origin allow-scripts allow-forms allow-popups allow-downloads allow-modals"
        />
      ) : (
        <div className="flex-1 flex flex-col items-center justify-center text-[var(--ct-muted)] text-sm gap-2">
          <svg width="28" height="28" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="0.8" strokeLinecap="round" className="opacity-25">
            <rect x="1" y="1" width="14" height="14" rx="2" />
            <line x1="1" y1="5" x2="15" y2="5" />
            <circle cx="3.5" cy="3" r="0.7" fill="currentColor" stroke="none" />
            <circle cx="5.5" cy="3" r="0.7" fill="currentColor" stroke="none" />
          </svg>
          <span>Visualizations &amp; local content</span>
          <span className="text-[11px] opacity-50">
            For localhost apps, data URLs, and agent-generated HTML
          </span>
        </div>
      )}
    </div>
  );
}

function NavButton({ onClick, title, children }: { onClick: () => void; title: string; children: React.ReactNode }) {
  return (
    <button onClick={onClick} title={title}
      className="w-6 h-6 flex items-center justify-center rounded-md text-[var(--ct-muted)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors"
    >{children}</button>
  );
}
