import { describe, expect, it } from "vitest";
import { missingInstallerPrerequisites, providerCliPresentation, type ProviderCliStatus } from "./providerCli";

const base: ProviderCliStatus = { required: true, installed: null, binary: "tool", resolved_path: null, availability: "unknown", searched: [], installer: "opaque installer", updater: null };
describe("provider CLI presentation", () => {
  it("shows only observed missing prerequisite runners", () => {
    const missing = { binary: "runner", installed: false, hint: "opaque setup guidance" };
    expect(missingInstallerPrerequisites({ ...base, installer_prerequisites: [missing, { ...missing, binary: "present", installed: true }, { ...missing, binary: "unknown", installed: null }] })).toEqual([missing]);
    expect(missingInstallerPrerequisites(null)).toEqual([]);
  });
  it("keeps missing and unknown separate and gates install on the command", () => {
    expect(providerCliPresentation(base).state).toBe("unknown");
    expect(providerCliPresentation({ ...base, installed: false }).state).toBe("missing");
    expect(providerCliPresentation({ ...base, installed: true }).canInstall).toBe(false);
    expect(providerCliPresentation({ ...base, installed: false, installer: null }).canInstall).toBe(false);
    expect(providerCliPresentation({ ...base, installed: false }).canInstall).toBe(true);
  });
  it("chat backends need no install even without an observed binary", () => {
    expect(providerCliPresentation({ ...base, required: false }).state).toBe("none");
    expect(providerCliPresentation({ ...base, required: false }).canInstall).toBe(false);
  });
});
