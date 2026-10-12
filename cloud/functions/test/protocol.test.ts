import { describe, expect, it } from "vitest";
import { callbackURL, challengeFromQuery, validRedirect } from "../../shared/protocol";
import { clientIp } from "../src/adapters/firebase/clientIp";

const challenge = {state:"s".repeat(32), code_challenge:"c".repeat(43), redirect_uri:"domios://auth/callback"};
describe("desktop request boundary", () => {
  it.each(["domios://auth/callback", "http://127.0.0.1:1/callback", "http://127.0.0.1:65535/callback"])("accepts %s", (uri) => {
    expect(validRedirect(uri)).toBe(true);
  });
  it.each([
    "https://evil.example", "http://localhost:8000/callback", "http://[::1]:8000/callback",
    "http://127.0.0.1:0/callback", "http://127.0.0.1:65536/callback", "http://127.0.0.1:080/callback",
    "http://127.0.0.1:8000/callback?x=1", "http://127.0.0.1:8000/callback#x", "http://127.0.0.1:8000/../callback",
    "http://evil@127.0.0.1:8000/callback", "http://127.1:8000/callback", "domios://auth/callback/",
    "domios://auth/callback?x=1", "domios://auth.evil/callback", "domios://auth/callback\n", "http://127.0.0.1:8000/callback\n",
  ])("refuses %s", (uri) => { expect(validRedirect(uri)).toBe(false); });
  it("refuses duplicate query values", () => {
    const query = new URLSearchParams(challenge);
    query.append("redirect_uri", "https://evil.example");
    expect(() => challengeFromQuery(query)).toThrow();
  });
  it("puts only the bound code and state in the desktop callback", () => {
    const uri = new URL(callbackURL(challenge, "d".repeat(43)));
    expect([...uri.searchParams.keys()]).toEqual(["code", "state"]);
    expect(uri.searchParams.get("state")).toBe(challenge.state);
    expect(uri.searchParams.get("code")).toBe("d".repeat(43));
  });
  it("ignores forged forwarded prefixes and canonicalizes client addresses", () => {
    expect(clientIp("1.2.3.4, 2001:db8::1, 10.0.0.1", undefined, false)).toBe("[2001:db8::1]");
    expect(clientIp("5.6.7.8, 2001:0db8:0:0:0:0:0:1, 10.0.0.1", undefined, false)).toBe("[2001:db8::1]");
    expect(clientIp("1.2.3.4", "::ffff:127.0.0.1", true)).toBe("127.0.0.1");
    expect(() => clientIp("1.2.3.4", undefined, false)).toThrow();
  });
});
