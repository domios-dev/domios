import { useEffect } from "react";
import { useMultiplexer } from "../store/multiplexer";
import { connectNoticeToasts, publishNotice } from "../lib/notices";
import { checkBuildMismatch, installBuildDriftTriggers, onMismatch, type BuildMismatch } from "../lib/buildMismatchGuard";
import { reloadInterface } from "../lib/serviceWorkerRegistration";
import { describeError } from "../lib/describeError";
import { acceptFrontendUpdate, frontendUpdateOffer, subscribeFrontendUpdateOffer } from "../lib/frontendUpdate/browser";
import { GATEWAY_STATE_EVENT, IS_WEB } from "../lib/webApi";
import { usePwaSetup } from "./usePwaSetup";

function mismatchDetails(mismatch: BuildMismatch) {
  return mismatch.reason === "api"
    ? `Interface API range: ${mismatch.minApiVersion}–${mismatch.maxApiVersion}; daemon API: ${mismatch.apiVersion ?? "unknown"}`
    : `Interface build: ${mismatch.frontendHash}; daemon build: ${mismatch.backendHash}`;
}

function copyDetails(raw: string) {
  return { label: "Copy details", run: async () => {
    try { await navigator.clipboard.writeText(raw); }
    catch { publishNotice({ kind: "toast", severity: "Warning", title: "Copy details unavailable", detail: "Open the notification details and copy the text manually." }); }
  } };
}

async function reload(mismatch: BuildMismatch) {
  try { await reloadInterface(mismatch.backendHash); }
  catch (error) {
    const description = describeError(error, { operation: "reloading the interface" });
    publishNotice({ kind: "toast", severity: "Error", id: "interface-reload-failure", title: description.title, detail: description.cause,
      actions: [copyDetails(description.raw), { label: "Retry", run: () => reload(mismatch) }],
    });
  }
}

export function useAppNotices() {
  const { upsertToast, dismissToast } = useMultiplexer();
  useEffect(() => connectNoticeToasts(upsertToast), [upsertToast]);
  usePwaSetup();

  useEffect(() => {
    let lastMismatch: string | null = null;
    const showMismatch = (mismatch: BuildMismatch | null) => {
      if (!mismatch) { lastMismatch = null; dismissToast("interface-build-mismatch"); return; }
      const key = JSON.stringify(mismatch);
      if (key === lastMismatch) return;
      lastMismatch = key;
      publishNotice({ kind: "toast", severity: "Warning", id: "interface-build-mismatch", title: "Interface update", detail: mismatchDetails(mismatch),
        actions: [{ label: "Reload interface", run: () => reload(mismatch) }, copyDetails(mismatchDetails(mismatch))],
      });
    };
    const unlisten = onMismatch(showMismatch);
    const uninstall = installBuildDriftTriggers();
    void checkBuildMismatch().then(showMismatch).catch(() => {});
    return () => { unlisten(); uninstall(); };
  }, [dismissToast]);

  useEffect(() => {
    let release: string | null = null;
    const update = () => {
      const offer = frontendUpdateOffer();
      if (!offer) { release = null; dismissToast("interface-update-available"); return; }
      if (release === offer.releaseId) return;
      release = offer.releaseId;
      publishNotice({ kind: "toast", severity: "Info", id: "interface-update-available", title: "Interface update available", detail: `Version ${offer.version} is ready.`,
        actions: [{ label: "Update", run: acceptFrontendUpdate }],
      });
    };
    update();
    return subscribeFrontendUpdateOffer(update);
  }, [dismissToast]);

  useEffect(() => {
    if (!IS_WEB) return;
    let hydrated = false;
    const update = () => {
      if (navigator.onLine) { dismissToast("offline"); return; }
      publishNotice({ kind: "toast", severity: "Warning", id: "offline", title: "Offline", detail: hydrated ? "Showing last saved state." : "Live data is unavailable." });
    };
    const onHydrated = () => { hydrated = true; update(); };
    const onReconciled = () => { hydrated = false; update(); };
    const gateway = (event: Event) => {
      if ((event as CustomEvent<{ available: boolean }>).detail.available) { dismissToast("gateway-unavailable"); return; }
      publishNotice({ kind: "toast", severity: "Warning", id: "gateway-unavailable", title: "Browser gateway unavailable", detail: "The current view is preserved. Live data and peer switching are paused." });
    };
    window.addEventListener("online", update);
    window.addEventListener("offline", update);
    window.addEventListener("codeterm-cache-hydrated", onHydrated);
    window.addEventListener("codeterm-cache-reconciled", onReconciled);
    window.addEventListener(GATEWAY_STATE_EVENT, gateway);
    update();
    return () => {
      window.removeEventListener("online", update);
      window.removeEventListener("offline", update);
      window.removeEventListener("codeterm-cache-hydrated", onHydrated);
      window.removeEventListener("codeterm-cache-reconciled", onReconciled);
      window.removeEventListener(GATEWAY_STATE_EVENT, gateway);
    };
  }, [dismissToast]);
}
