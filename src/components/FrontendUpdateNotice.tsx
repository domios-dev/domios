import { useSyncExternalStore } from "react";
import { acceptFrontendUpdate, frontendUpdateOffer, subscribeFrontendUpdateOffer } from "../lib/frontendUpdate/browser";

// Shown only while someone is looking at the window; an unattended window swaps on its own.
export function FrontendUpdateNotice() {
  const offer = useSyncExternalStore(subscribeFrontendUpdateOffer, frontendUpdateOffer, () => null);
  if (!offer) return null;
  return (
    <div
      role="status"
      aria-live="polite"
      className="fixed bottom-4 left-1/2 z-[1000] flex -translate-x-1/2 items-center gap-3 rounded-lg border border-[var(--ct-border-default)] bg-[var(--ct-bg-elev)] px-4 py-2 text-sm text-[var(--ct-fg)] shadow-lg"
    >
      <span>A new version of the interface is out ({offer.version}).</span>
      <button
        type="button"
        onClick={acceptFrontendUpdate}
        className="rounded-md bg-[var(--ct-accent)] px-3 py-1 font-medium text-[var(--ct-bg)] hover:opacity-90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-border-focus)]"
      >
        Update
      </button>
    </div>
  );
}
