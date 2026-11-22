import { useEffect } from "react";
import { IS_WEB, selfHost, tokenFor } from "../lib/webApi";
import { registerServiceWorker } from "../lib/serviceWorkerRegistration";
import { setPwaInstallOffer } from "../lib/pwaInstallOffer";

const HINT_DISMISSED_KEY = "codeterm-pwa-hint-dismissed";
const INSTALL_DISMISSED_KEY = "codeterm-pwa-install-dismissed";

interface BeforeInstallPromptEvent extends Event {
  prompt(): Promise<void>;
  userChoice: Promise<{ outcome: "accepted" | "dismissed" }>;
}

async function bootstrapManifestCookie(token: string): Promise<boolean> {
  try {
    // biome-ignore lint/style/noRestrictedGlobals: HttpOnly manifest cookie requires a browser HTTP response.
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

export function usePwaSetup() {
  useEffect(() => {
    if (!IS_WEB) return;
    registerServiceWorker();
    const token = tokenFor(selfHost());
    if (token && !document.querySelector('link[rel="manifest"]')) {
      void bootstrapManifestCookie(token).then(() => {
        if (document.querySelector('link[rel="manifest"]')) return;
        const link = document.createElement("link");
        link.rel = "manifest";
        link.href = "/manifest.webmanifest";
        document.head.appendChild(link);
      });
    }
    if (isIos() && !isStandalone() && !localStorage.getItem(HINT_DISMISSED_KEY)) {
      setPwaInstallOffer({
        kind: "ios",
        description: "In Safari, tap Share, then Add to Home Screen for fullscreen access.",
        dismiss: () => { localStorage.setItem(HINT_DISMISSED_KEY, "1"); setPwaInstallOffer(null); },
      });
    }
  }, []);

  useEffect(() => {
    if (!IS_WEB || isStandalone()) return;
    const onInstallPrompt = (event: Event) => {
      event.preventDefault();
      if (localStorage.getItem(INSTALL_DISMISSED_KEY)) return;
      const prompt = event as BeforeInstallPromptEvent;
      setPwaInstallOffer({
        kind: "prompt",
        description: "Add Domios to your home screen or app dock.",
        install: async () => { await prompt.prompt(); await prompt.userChoice; setPwaInstallOffer(null); },
        dismiss: () => { localStorage.setItem(INSTALL_DISMISSED_KEY, "1"); setPwaInstallOffer(null); },
      });
    };
    window.addEventListener("beforeinstallprompt", onInstallPrompt);
    return () => window.removeEventListener("beforeinstallprompt", onInstallPrompt);
  }, []);
}
