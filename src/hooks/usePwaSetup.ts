import { useEffect } from "react";
import { IS_WEB, selfHost, tokenFor } from "../lib/webApi";
import { registerServiceWorker } from "../lib/serviceWorkerRegistration";
import { publishNotice } from "../lib/notices";

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
      publishNotice({ kind: "toast", severity: "Info", id: "pwa-install-ios", title: "Install Domios", detail: "Tap Share, then Add to Home Screen for fullscreen access.",
        actions: [{ label: "Do not show again", run: () => localStorage.setItem(HINT_DISMISSED_KEY, "1") }],
      });
    }
  }, []);

  useEffect(() => {
    if (!IS_WEB || isStandalone()) return;
    const onInstallPrompt = (event: Event) => {
      event.preventDefault();
      if (localStorage.getItem(INSTALL_DISMISSED_KEY)) return;
      const prompt = event as BeforeInstallPromptEvent;
      publishNotice({ kind: "toast", severity: "Info", id: "pwa-install", title: "Install Domios", detail: "Add to your home screen or app dock for fullscreen access.",
        actions: [
          { label: "Install", run: async () => { await prompt.prompt(); await prompt.userChoice; } },
          { label: "Do not show again", run: () => localStorage.setItem(INSTALL_DISMISSED_KEY, "1") },
        ],
      });
    };
    window.addEventListener("beforeinstallprompt", onInstallPrompt);
    return () => window.removeEventListener("beforeinstallprompt", onInstallPrompt);
  }, []);
}
