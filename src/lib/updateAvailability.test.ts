import fixture from "../../tests/fixtures/update-availability/snapshot.json";
import { afterEach, expect, it, vi } from "vitest";
vi.mock("./bridge", () => ({ invoke: vi.fn() }));
import { invoke } from "./bridge";
import { checkUpdateAvailability, decodeUpdateAvailability, type UpdateAvailabilitySnapshot } from "./updateAvailability";

afterEach(() => vi.resetAllMocks());

it("requests daemon availability and preserves errors and core states", async () => {
  const snapshot: UpdateAvailabilitySnapshot = {
    items: [{ id: "core", kind: "plugin", installed: "1.0.0", latest: "1.1.0", compatible: true, state: "core_silent", reason: "newer_version", channel: "official" }],
    errors: { frontend: "unavailable" },
  };
  vi.mocked(invoke).mockResolvedValue(snapshot);
  expect(await checkUpdateAvailability(true)).toEqual(snapshot);
  expect(invoke).toHaveBeenCalledWith("check_update_availability", { force: true });
});

it("defaults to passive discovery and propagates transport failure", async () => {
  vi.mocked(invoke).mockRejectedValue(new Error("offline"));
  await expect(checkUpdateAvailability()).rejects.toThrow("offline");
  expect(invoke).toHaveBeenCalledWith("check_update_availability", { force: false });
});

it("decodes the API contract through HTTP transport for installed prereleases", async () => {
  const { createTransportClient } = await import("./transportClient");
  for (const installed of ["1.14.9-bs.3", "1.14.9-dev.4"]) {
    const snapshot = { ...fixture, items: [{ ...fixture.items[0], installed }] };
    const transport = createTransportClient({ origin: "http://127.0.0.1:7686", token: "opaque",
      fetch: vi.fn().mockResolvedValue(Response.json(snapshot)) });
    vi.mocked(invoke).mockImplementation(() => transport.json("GET", "/api/update/availability?force=false"));
    expect(await checkUpdateAvailability()).toEqual(snapshot);
  }
});

it("rejects malformed or error envelopes instead of treating them as an availability snapshot", () => {
  for (const value of [null, "", { error: "offline" }, { items: [], errors: [] }, { items: [null], errors: {} }, { items: [], errors: { app: 5 } }]) {
    expect(() => decodeUpdateAvailability(value)).toThrow();
  }
});

it("preserves HTTP failure status and JSON detail for explicit action diagnostics", async () => {
  const { createTransportClient } = await import("./transportClient");
  const transport = createTransportClient({ origin: "http://127.0.0.1:7686", token: "opaque",
    fetch: vi.fn().mockResolvedValue(Response.json({ error: "manifest unavailable" }, { status: 502 })) });
  vi.mocked(invoke).mockImplementation(() => transport.json("GET", "/api/update/availability?force=true"));
  await expect(checkUpdateAvailability(true)).rejects.toMatchObject({ status: 502, body: '{"error":"manifest unavailable"}' });
});
