import { useCallback, useEffect, useState } from "react";
import { IS_WEB, selfHost, tokenFor } from "../../lib/webApi";
import { registerServiceWorker } from "../../lib/serviceWorkerRegistration";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { useInstallHintLayout } from "./useInstallHintLayout";
import { OfflineIndicator } from "./OfflineIndicator";

const HINT_DISMISSED_KEY = "codeterm-pwa-hint-dismissed";
const INSTALL_DISMISSED_KEY = "codeterm-pwa-install-dismissed";

interface BeforeInstallPromptEvent extends Event {
  prompt(): Promise<void>;
  userChoice: Promise<{ outcome: "accepted" | "dismissed" }>;
}

async function bootstrapManifestCookie(token: string): Promise<boolean> {
  try {
    const resp = await fetch("/manifest-bootstrap", {
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

export function PwaSetup() {
  const [showIosHint, setShowIosHint] = useState(false);
  const [installPrompt, setInstallPrompt] = useState<BeforeInstallPromptEvent | null>(null);
  const [showInstall, setShowInstall] = useState(false);

  const touch = useTouchOptimizedUi();
  const hintLayout = useInstallHintLayout(touch, showIosHint || showInstall);

  useEffect(() => {
    if (!IS_WEB) return;

    registerServiceWorker();

    const token = tokenFor(selfHost());
    if (token && !document.querySelector('link[rel="manifest"]')) {
      bootstrapManifestCookie(token).then(() => {
        const link = document.createElement("link");
        link.rel = "manifest";
        link.href = "/manifest.webmanifest";
        document.head.appendChild(link);
      });
    }

    if (isIos() && !isStandalone() && !localStorage.getItem(HINT_DISMISSED_KEY)) {
      setShowIosHint(true);
    }
  }, []);

  useEffect(() => {
    if (!IS_WEB || isStandalone()) return;

    const onInstallPrompt = (event: Event) => {
      event.preventDefault();
      if (localStorage.getItem(INSTALL_DISMISSED_KEY)) return;
      setInstallPrompt(event as BeforeInstallPromptEvent);
      setShowInstall(true);
    };

    window.addEventListener("beforeinstallprompt", onInstallPrompt);
    return () => window.removeEventListener("beforeinstallprompt", onInstallPrompt);
  }, []);

  const dismissIosHint = useCallback(() => {
    localStorage.setItem(HINT_DISMISSED_KEY, "1");
    setShowIosHint(false);
  }, []);

  const dismissInstall = useCallback(() => {
    localStorage.setItem(INSTALL_DISMISSED_KEY, "1");
    setShowInstall(false);
    setInstallPrompt(null);
  }, []);

  const acceptInstall = useCallback(async () => {
    if (!installPrompt) return;
    await installPrompt.prompt();
    await installPrompt.userChoice;
    setShowInstall(false);
    setInstallPrompt(null);
  }, [installPrompt]);

  return (
    <>
      <OfflineIndicator />
      {showInstall && installPrompt && (
        <div
          style={hintLayout}
          data-testid="pwa-chrome-install-hint"
          className="fixed inset-x-0 bottom-0 z-50 p-3 pb-[calc(0.75rem+var(--ct-safe-bottom,0px))] pointer-events-none"
        >
          <div className="mx-auto max-w-md rounded-lg border border-[var(--ct-border,#30363d)] bg-[var(--ct-titlebar-bg,#161b22)] text-[var(--ct-fg,#e6edf3)] p-3 shadow-xl flex items-start gap-3 pointer-events-auto">
            <img src="./icons/pwa-192.png" alt="" className="w-9 h-9 rounded-lg shrink-0" />
            <div className="flex-1 text-xs leading-relaxed">
              <div className="font-semibold mb-0.5">Install Domios</div>
              <span className="opacity-80">
                Add to your home screen or app dock for fullscreen access and faster
                startup.
              </span>
              <button
                data-testid="pwa-chrome-install-accept"
                type="button"
                onClick={() => void acceptInstall()}
                className="mt-2 block w-full min-h-[36px] rounded bg-[var(--ct-accent,#1f6feb)] text-xs font-medium"
              >
                Install
              </button>
            </div>
            <button
              data-testid="pwa-chrome-install-dismiss"
              type="button"
              onClick={dismissInstall}
              aria-label="Dismiss"
              className="shrink-0 -m-1 p-1 opacity-60 hover:opacity-100 text-sm leading-none"
            >
              ✕
            </button>
          </div>
        </div>
      )}
      {showIosHint && (
        <div
          style={hintLayout}
          data-testid="pwa-install-hint"
          className="fixed inset-x-0 bottom-0 z-50 p-3 pb-[calc(0.75rem+var(--ct-safe-bottom,0px))] pointer-events-none"
        >
          <div className="mx-auto max-w-md rounded-lg border border-[var(--ct-border,#30363d)] bg-[var(--ct-titlebar-bg,#161b22)] text-[var(--ct-fg,#e6edf3)] p-3 shadow-xl flex items-start gap-3 pointer-events-auto">
            <img src="./icons/pwa-192.png" alt="" className="w-9 h-9 rounded-lg shrink-0" />
            <div className="flex-1 text-xs leading-relaxed">
              <div className="font-semibold mb-0.5">Install Domios</div>
              <span className="opacity-80">
                Tap <ShareGlyph /> Share, then &ldquo;Add to Home Screen&rdquo; — opens
                fullscreen with its own icon, no address bar.
              </span>
            </div>
            <button
              data-testid="pwa-install-hint-dismiss"
              type="button"
              onClick={dismissIosHint}
              aria-label="Dismiss"
              className="shrink-0 -m-1 p-1 opacity-60 hover:opacity-100 text-sm leading-none"
            >
              ✕
            </button>
          </div>
        </div>
      )}
    </>
  );
}
