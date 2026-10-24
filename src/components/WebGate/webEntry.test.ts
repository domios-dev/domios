import { describe, expect, it } from "vitest";
import { consumeWebEntry } from "./webEntry";

describe("Web entry model", () => {
  it("prefers a fragment token, removes both token sources and keeps ordinary query parameters", () => {
    expect(consumeWebEntry(new URL("https://app.test/app?view=all&token=query#token=%20fragment%20&relay=peer"))).toEqual({
      hasEntry: true, token: "fragment", relay: "peer", cleanUrl: "/app?view=all",
    });
  });

  it("scrubs empty credentials and handles the query fallback and self relay", () => {
    expect(consumeWebEntry(new URL("https://app.test/?token=query#relay=self"))).toEqual({
      hasEntry: true, token: "query", relay: "self", cleanUrl: "/",
    });
    expect(consumeWebEntry(new URL("https://app.test/#token="))).toMatchObject({ hasEntry: true, token: "", cleanUrl: "/" });
  });

  it("leaves ordinary fragment navigation alone", () => {
    expect(consumeWebEntry(new URL("https://app.test/app?view=all#section"))).toMatchObject({
      hasEntry: false, cleanUrl: "/app?view=all#section",
    });
  });
});
