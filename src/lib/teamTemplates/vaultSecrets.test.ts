import { describe, expect, it } from "vitest";
import { userSecretNames } from "./vaultSecrets";

describe("secret ask vault entries", () => {
  it("offers only the user's own entries, never Domios's internal ones", () => {
    expect(userSecretNames([
      { name: "SEARCH_API_KEY", owner: "user" },
      { name: "domios-account-local-refresh", owner: "system" },
      { name: "blueprint-webhook-abc", owner: "system" },
      { name: "SEARCH_API_KEY", owner: "user" },
      { name: "GITHUB_TOKEN", owner: "user" },
    ])).toEqual(["GITHUB_TOKEN", "SEARCH_API_KEY"]);
  });
});
