import { describe, expect, it } from "vitest";
import { describeError } from "./describeError";
import { parseProviderNotInstalled } from "./providerNotInstalled";

describe("parseProviderNotInstalled", () => {
  it("parses preflight and wrapped runtime missing-CLI errors with install commands", () => {
    const raw =
      "Runtime could not start: Provider not installed: Opaque Agent (`opaque-id`) launches `opaque-bin`, which is not on this machine. Searched: [/usr/bin; ~/.opaque/bin]. Install command: curl -fsSL https://example.test/install | bash. fix: domios agent cli install opaque-id";
    expect(parseProviderNotInstalled(raw)).toEqual({
      providerId: "opaque-id",
      displayName: "Opaque Agent",
      binary: "opaque-bin",
      installCommand: "curl -fsSL https://example.test/install | bash",
    });

    const described = describeError(raw);
    expect(described.title).toBe("Opaque Agent isn't installed");
    expect(described.actions).toEqual(["check-executable", "retry"]);
  });

  it("handles providers without a platform installer cleanly", () => {
    const raw =
      "Provider not installed: Opaque Agent (`opaque-id`) launches `opaque-bin`, which is not on this machine. Searched: []. Install command: No install command is available for this platform.. fix: domios agent cli install opaque-id";
    expect(parseProviderNotInstalled(raw)).toEqual({
      providerId: "opaque-id",
      displayName: "Opaque Agent",
      binary: "opaque-bin",
      installCommand: null,
    });
  });

  it("returns null for unrelated errors", () => {
    expect(parseProviderNotInstalled("Runtime could not start: permission denied")).toBeNull();
  });
});
