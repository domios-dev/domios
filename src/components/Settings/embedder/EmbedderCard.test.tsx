import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AppConfig } from "../../../lib/types";
import { EmbedderCard } from "./EmbedderCard";
import { listingFixture } from "./embedderFixtures";

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
