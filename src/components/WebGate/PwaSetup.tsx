import { useCallback, useEffect, useState } from "react";
import { IS_WEB, selfHost, tokenFor } from "../../lib/webApi";

const HINT_DISMISSED_KEY = "codeterm-pwa-hint-dismissed";

/** Bootstrap the PWA manifest cookie so the manifest endpoint picks up the
 *  token from a cookie instead of a query string (which leaks via logs/Referer).
 *  Returns true if the cookie was set successfully. */
async function bootstrapManifestCookie(token: string): Promise<boolean> {
  try {
    const resp = await fetch("/web/manifest-bootstrap", {
      method: "POST",
      headers: { Authorization: `Bearer ${token}` },
      credentials: "same-origin",
    });
    return resp.ok;
  } catch {
    return false;
  }
}

function isIos(): boolean {
  return (
    /iPhone|iPad|iPod/.test(navigator.userAgent) ||
    // iPadOS reports as Mac; touch points tell it apart.
    (navigator.userAgent.includes("Mac") && navigator.maxTouchPoints > 1)
  );
}

function isStandalone(): boolean {
  return (
    window.matchMedia("(display-mode: standalone)").matches ||
    (navigator as Navigator & { standalone?: boolean }).standalone === true
  );
}

function ShareGlyph() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className="inline-block align-[-2px]"
    >
      <path d="M12 3v12" />
      <path d="M8 7l4-4 4 4" />
      <path d="M5 11v8a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-8" />
    </svg>
  );
}

/**
 * Web-mirror PWA glue: injects the manifest link with the self-host token
 * baked into start_url (installed iOS apps don't share Safari's storage)
 * and shows a dismissable add-to-home-screen hint on iOS browsers.
 */
export function PwaSetup() {
  const [showHint, setShowHint] = useState(false);

  useEffect(() => {
    if (!IS_WEB || !window.location.pathname.startsWith("/web")) return;
    const token = tokenFor(selfHost());
    if (token && !document.querySelector('link[rel="manifest"]')) {
      // Bootstrap an HttpOnly cookie so the manifest endpoint authenticates
      // via cookie instead of a query-string token (which leaks in logs/Referer).
      bootstrapManifestCookie(token).then(() => {
        // Always inject the manifest link — even if the cookie bootstrap failed,
        // the backend still accepts the legacy ?token= fallback for backward compat.
        const link = document.createElement("link");
        link.rel = "manifest";
        // No query param — the token is in the cookie (or we fall through to
        // the unauthenticated manifest, which just shows the plain mirror).
        link.href = "/web/manifest.webmanifest";
        document.head.appendChild(link);
      });
    }
    if (isIos() && !isStandalone() && !localStorage.getItem(HINT_DISMISSED_KEY)) {
      setShowHint(true);
    }
  }, []);

  const dismiss = useCallback(() => {
    localStorage.setItem(HINT_DISMISSED_KEY, "1");
    setShowHint(false);
  }, []);

  if (!showHint) return null;

  return (
    <div
      data-testid="pwa-install-hint"
      className="fixed inset-x-0 bottom-0 z-50 p-3 pb-[calc(0.75rem+var(--ct-safe-bottom,0px))] pointer-events-none"
    >
      <div className="mx-auto max-w-md rounded-lg border border-[var(--ct-border,#30363d)] bg-[var(--ct-titlebar-bg,#161b22)] text-[var(--ct-fg,#e6edf3)] p-3 shadow-xl flex items-start gap-3 pointer-events-auto">
        <img src="./icons/pwa-192.png" alt="" className="w-9 h-9 rounded-lg shrink-0" />
        <div className="flex-1 text-xs leading-relaxed">
          <div className="font-semibold mb-0.5">Install CodeTerm</div>
          <span className="opacity-80">
            Tap <ShareGlyph /> Share, then &ldquo;Add to Home Screen&rdquo; — opens
            fullscreen with its own icon, no address bar.
          </span>
        </div>
        <button
          data-testid="pwa-install-hint-dismiss"
          onClick={dismiss}
          aria-label="Dismiss"
          className="shrink-0 -m-1 p-1 opacity-60 hover:opacity-100 text-sm leading-none"
        >
          ✕
        </button>
      </div>
    </div>
  );
}
