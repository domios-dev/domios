import { describe, expect, it } from "vitest";
import { installerShellLanguage, missingInstallerPrerequisites, providerCliPresentation, providerCliView, type ProviderCliStatus } from "./providerCli";

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

describe("provider CLI view", () => {
  const prerequisite = { binary: "runner", installed: false, hint: "opaque hint" };
  it("an installed CLI carries its version and keeps the path as a hint, with no install command", () => {
    const view = providerCliView({ ...base, installed: true, detected_version: " 1.2.3 ", resolved_path: "/opaque/tool", installer_prerequisites: [prerequisite] }, null);
    expect(view).toMatchObject({ state: "installed", version: "1.2.3", pathHint: "/opaque/tool", canInstall: false, canRecheck: false, installCommand: null, prerequisites: [] });
  });
  it("a missing CLI offers its install command once with the missing prerequisites", () => {
    const view = providerCliView({ ...base, installed: false, installer_prerequisites: [prerequisite, { ...prerequisite, binary: "present", installed: true }] }, null);
    expect(view).toMatchObject({ state: "missing", canInstall: true, canRecheck: false, installCommand: "opaque installer", version: null, pathHint: null });
    expect(view.prerequisites).toEqual([{ binary: "runner", hint: "opaque hint" }]);
  });
  it("only an unknown status offers a re-check, including a failed probe", () => {
    expect(providerCliView(base, null)).toMatchObject({ state: "unknown", canRecheck: true, installCommand: null });
    expect(providerCliView(null, "opaque failure")).toMatchObject({ state: "unknown", canRecheck: true, canInstall: false });
    expect(providerCliView(null, null)).toMatchObject({ state: "checking", canRecheck: false, canInstall: false });
  });
  it("labels the install command with the shell it runs in", () => {
    expect(installerShellLanguage("irm https://example.invalid/install.ps1 | iex")).toBe("powershell");
    expect(installerShellLanguage("curl -fsSL https://example.invalid/install.sh | sh")).toBe("bash");
  });
});
