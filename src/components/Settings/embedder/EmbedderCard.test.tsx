import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AppConfig } from "../../../lib/types";
import { EmbedderCard } from "./EmbedderCard";
import { listingFixture, statusFixture } from "./embedderFixtures";

vi.mock("../../../lib/webApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/webApi")>()),
  daemonHttpBase: () => "http://127.0.0.1:7686",
  daemonAuthToken: () => "fixture-auth",
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function respond(body: unknown, status = 200) {
  return Promise.resolve(new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } }));
}

afterEach(() => vi.unstubAllGlobals());

describe("EmbedderCard", () => {
  it("keeps setup hints in a collapsed accessible disclosure", async () => {
    const listing = listingFixture(true);
    vi.stubGlobal("fetch", vi.fn(() => respond(listing)));
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<EmbedderCard config={{ api: {} } as unknown as AppConfig} />));
      const disclosure = host.querySelector("details")!;
      const summary = disclosure.querySelector("summary")!;
      expect(summary.textContent?.trim().length).toBeGreaterThan(0);
      expect(disclosure.open).toBe(false);
      const hints = [...host.querySelectorAll("code")];
      expect(hints.length).toBeGreaterThan(0);
      expect(hints.every((hint) => disclosure.contains(hint))).toBe(true);
      await act(async () => summary.click());
      expect(disclosure.open).toBe(true);
      await act(async () => summary.click());
      expect(disclosure.open).toBe(false);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("exposes the running re-embed progress value and maximum", async () => {
    const listing = listingFixture(false);
    listing.status = statusFixture({ reembed: {
      state: "running", target: null, total: 37, done: 11, failed: 0,
      current_item: { id: "fixture", done: 8, total: 20 },
    } });
    vi.stubGlobal("fetch", vi.fn(() => respond(listing)));
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<EmbedderCard config={{ api: {} } as unknown as AppConfig} />));
      const progress = host.querySelector<HTMLProgressElement>("progress[aria-label]")!;
      expect(progress.getAttribute("aria-label")?.trim().length).toBeGreaterThan(0);
      expect(progress.value).toBe(11.4);
      expect(progress.max).toBe(37);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("renders malformed refusals from preview and confirmation without throwing", async () => {
    const failures: { message: string; respond: () => Promise<Response>; returned?: unknown }[] = [
      { message: "fixture transport failure", respond: () => Promise.reject(new TypeError("fixture transport failure")) },
      { message: "fixture string failure", respond: () => respond({ error: "fixture string failure" }, 503), returned: "fixture string failure" },
      { message: "fixture missing hints", respond: () => respond({ error: { message: "fixture missing hints" } }, 409), returned: { message: "fixture missing hints" } },
    ];
    for (const phase of ["preview", "confirm", "returned"]) {
      const cases = phase === "returned" ? failures.filter((failure) => failure.returned !== undefined) : failures;
      for (const failure of cases) {
        const listing = listingFixture(false);
        listing.providers[0].models.push({ ...listing.providers[0].models[0], model: "other" });
        vi.stubGlobal("fetch", vi.fn((url: string) => {
          if (url.endsWith("/use") || (phase === "preview" && url.includes("/preview"))) return failure.respond();
          if (url.includes("/preview")) {
            return respond({ target: listing.status, remote: false, warning: null,
              plan: { items: 1, entities: 0, docs: 0 }, total: 1, refusal: phase === "returned" ? failure.returned : null });
          }
          return respond(listing);
        }));
        const host = document.createElement("div");
        document.body.append(host);
        const root = createRoot(host);
        try {
          await act(async () => root.render(<EmbedderCard config={{ api: {} } as unknown as AppConfig} />));
          const select = host.querySelector<HTMLSelectElement>('select[aria-label="Embedder"]')!;
          await act(async () => {
            select.value = "local\u0000other";
            select.dispatchEvent(new Event("change", { bubbles: true }));
          });
          if (phase === "confirm") {
            const region = host.querySelector('[role="region"][aria-label="Confirm embedder switch"]')!;
            const confirm = region.querySelector<HTMLButtonElement>("button")!;
            await act(async () => confirm.click());
          }
          const notice = host.querySelector('[role="status"]');
          expect(notice?.textContent).toContain(failure.message);
          expect(select.isConnected).toBe(true);
          expect(select.disabled).toBe(false);
        } finally {
          await act(async () => root.unmount());
          host.remove();
        }
      }
    }
  });

  it("shows status, disables a refused remote option, previews the count and starts the switch", async () => {
    const calls: string[] = [];
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      calls.push(`${init?.method ?? "GET"} ${new URL(url).pathname}`);
      if (url.includes("/preview")) {
        return respond({ target: { provider: "local", model: "l12", dimension: 384 }, remote: false, warning: null,
          plan: { items: 7, entities: 2, docs: 3 }, total: 12, refusal: null });
      }
      if (url.endsWith("/use")) return respond({ total: 12 });
      return respond(listingFixture(true));
    }));
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<EmbedderCard config={{ api: {} } as unknown as AppConfig} />));
      expect(host.querySelector('[data-testid="embedder-dimension"]')?.textContent).toBe("384");
      const select = host.querySelector<HTMLSelectElement>('select[aria-label="Embedder"]')!;
      const remote = [...select.options].find((option) => option.value.startsWith("remote-x"))!;
      expect(remote.disabled).toBe(true);
      expect(host.querySelector('[aria-label="Unavailable embedders"]')?.textContent).toContain("no key stored");

      const other = document.createElement("option");
      other.value = "local\u0000other";
      select.append(other);
      await act(async () => {
        select.value = other.value;
        select.dispatchEvent(new Event("change", { bubbles: true }));
      });
      expect(host.querySelector('[data-testid="embedder-switch-count"]')?.textContent).toContain("12 items");
      const confirm = [...host.querySelectorAll("button")].find((button) => button.textContent === "Switch and re-embed")!;
      await act(async () => confirm.click());
      expect(calls).toContain("POST /api/mem/embedder/use");
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
