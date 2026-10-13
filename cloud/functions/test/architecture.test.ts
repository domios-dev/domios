import { readFileSync } from "node:fs";
import { expect, it } from "vitest";
import { accountsAdapters } from "../src/adapters";
it("keeps the model and page independent of vendor SDK imports", () => {
  for (const path of ["../src/billing/model.ts", "../src/billing/service.ts", "../src/loginCodes.ts", "../../shared/protocol.ts", "../../hosting/signin/main.ts", "../../hosting/signin/authClient.ts"]) {
    const source = readFileSync(new URL(path,import.meta.url),"utf8");
    const imports = [...source.matchAll(/from\s+["']([^"']+)["']/g)].map((match) => match[1]);
    expect(imports.filter((name) => name.startsWith("firebase") || name.includes("paddle"))).toEqual([]);
  }
});
it("refuses unknown configured adapters", () => {
  expect(() => accountsAdapters("unknown" as "local")).toThrow();
});
