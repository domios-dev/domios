/**
 * Selective network severing for web-mode Playwright specs — the primitive
 * S9 (verification infrastructure) calls out as missing: today "reconnect"
 * is only testable via full page reload or daemon restart.
 *
 * Two distinct mechanisms, chosen per target because of a real CDP/Playwright
 * limitation: `page.route()` interception is checked once, at request
 * dispatch — it can abort a request that hasn't been sent yet, but it CANNOT
 * reach back into a request that is already in flight and awaiting a
 * response (an open SSE stream, a pending PTY long-poll). There is no public
 * CDP command to cancel one arbitrary already-dispatched request by id.
 *
 * - Selective targets (sse / pty-read / pty-stream): route-abort every
 *   matching request from the moment `sever()` is called onward. By default
 *   a request already in flight at that instant completes or times out on
 *   its own; the NEXT cycle of that channel (SSE reconnect, or the PTY read
 *   loop's next long-poll) is what observably dies. Other channels are
 *   completely unaffected.
 * - "all": uses `BrowserContext.setOffline(true)`, which genuinely kills
 *   already-open connections immediately (Chromium fails in-flight requests
 *   with net::ERR_INTERNET_DISCONNECTED).
 * - `sever(target, { hard: true })` on a selective target additionally kills
 *   whatever is CURRENTLY in flight for every channel via a brief (~150ms)
 *   full-connectivity blip (the same offline flip "all" uses), applied AFTER
 *   the route-abort is already registered. Non-target channels see one
 *   dropped connection and self-heal via their own reconnect logic (SSE) or
 *   next poll (PTY) within their normal backoff; the target channel stays
 *   down afterward because the route-abort keeps catching its retry. Use
 *   this when a test needs an ALREADY-OPEN connection of `target` to die
 *   immediately rather than waiting for its natural end.
 */
import type { Page } from "@playwright/test";

export type DisruptionTarget = "sse" | "pty-read" | "pty-stream" | "all";
type SelectiveTarget = Exclude<DisruptionTarget, "all">;

export interface SeverOptions {
  /** Force an immediate kill of whatever is currently in flight (see module doc). */
  hard?: boolean;
}

const ROUTE_GLOB: Record<Exclude<DisruptionTarget, "all">, string> = {
  sse: "**/api/events/stream**",
  "pty-read": "**/api/pty/*/read**",
  "pty-stream": "**/api/tabs/*/output/stream**",
};

const BLIP_MS = 150;
export const NETWORK_DISRUPTION_ABORT_REGISTRY = "__ctNetworkDisruptionAborters";

type AbortRegistryWindow = Window & {
  [key: string]: Record<SelectiveTarget, AbortController[]> | unknown;
};

export interface NetworkDisruption {
  /** Start severing `target`. Idempotent (a repeated call still honors `hard`). */
  sever(target: DisruptionTarget, opts?: SeverOptions): Promise<void>;
  /** Restore `target`. Idempotent. */
  restore(target: DisruptionTarget): Promise<void>;
  /** Restore every currently-severed target. */
  restoreAll(): Promise<void>;
  /** Targets currently severed. */
  severed(): DisruptionTarget[];
}

export function networkDisruption(page: Page): NetworkDisruption {
  const active = new Set<DisruptionTarget>();

  async function severSelective(target: Exclude<DisruptionTarget, "all">): Promise<void> {
    await page.route(ROUTE_GLOB[target], (route) => route.abort("failed"));
  }

  async function restoreSelective(target: Exclude<DisruptionTarget, "all">): Promise<void> {
    await page.unroute(ROUTE_GLOB[target]);
  }

  async function blip(): Promise<void> {
    const ctx = page.context();
    await ctx.setOffline(true);
    await new Promise((resolve) => setTimeout(resolve, BLIP_MS));
    await ctx.setOffline(false);
  }

  async function abortRegistered(target: SelectiveTarget): Promise<void> {
    await page.evaluate(
      ({ key, t }) => {
        const win = window as AbortRegistryWindow;
        const registry = win[key] as Record<SelectiveTarget, AbortController[]> | undefined;
        const controllers = registry?.[t] ?? [];
        for (const controller of controllers.splice(0)) {
          controller.abort();
        }
      },
      { key: NETWORK_DISRUPTION_ABORT_REGISTRY, t: target },
    );
  }

  const disruption: NetworkDisruption = {
    async sever(target, opts = {}) {
      if (!active.has(target)) {
        active.add(target);
        if (target === "all") {
          await page.context().setOffline(true);
          return;
        }
        await severSelective(target);
      }
      if (opts.hard && target !== "all") {
        await abortRegistered(target);
        await blip();
      }
    },

    async restore(target) {
      if (!active.has(target)) return;
      active.delete(target);
      if (target === "all") {
        await page.context().setOffline(false);
      } else {
        await restoreSelective(target);
      }
    },

    async restoreAll() {
      for (const target of Array.from(active)) {
        await disruption.restore(target);
      }
    },

    severed() {
      return Array.from(active);
    },
  };

  return disruption;
}
