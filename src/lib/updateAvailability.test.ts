import { afterEach, expect, it, vi } from "vitest";
vi.mock("./bridge", () => ({ invoke: vi.fn() }));
import { invoke } from "./bridge";
import { checkUpdateAvailability, type UpdateAvailabilitySnapshot } from "./updateAvailability";

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
