import { createHash } from "node:crypto";
import { describe, it, expect } from "vitest";
import { CodeExchange, CODE_LIFETIME_MS, sha256 } from "../src/loginCodes";
import { accountsAdapters } from "../src/adapters";
import { fakeClient } from "../../hosting/signin/adapters/fake";

const verifier = "v".repeat(43);
const challenge = {code_challenge:createHash("sha256").update(verifier).digest("base64url"),state:"s".repeat(32),redirect_uri:"domios://auth/callback"};
function fixture() {
  let now = 1000;
  const {store,tokens} = accountsAdapters("fake");
  return {store,tokens,exchange:new CodeExchange(store,tokens,() => now),advance:(ms:number) => {now += ms;}};
}
describe("vendor-independent exchange", () => {
  it("creates only hashed single-use records and mints the bound UID", async () => {
    const {store,tokens,exchange} = fixture();
    const code = await exchange.create("owner",challenge);
    const record = store.record(sha256(code))!;
    expect(record).toEqual({...challenge,uid:"owner",expires_at:1000+CODE_LIFETIME_MS,used:false});
    expect(await exchange.redeem({code,code_verifier:verifier,...challenge})).toBe("test-token:owner");
    expect(tokens.minted).toEqual(["owner"]);
    await expect(exchange.redeem({code,code_verifier:verifier,...challenge})).rejects.toThrow();
  });
  it("expires at exactly 120 seconds", async () => {
    const {exchange,advance,tokens} = fixture();
    const code = await exchange.create("owner",challenge);
    advance(CODE_LIFETIME_MS);
    await expect(exchange.redeem({...challenge,code,code_verifier:verifier})).rejects.toThrow();
    expect(tokens.minted).toEqual([]);
  });
  it("keeps the fake browser adapter behind the same interface", async () => {
    const client = fakeClient();
    await expect(client.createCode(challenge)).rejects.toThrow();
    await client.signIn("local");
    expect(await client.createCode(challenge)).toMatch(/^[A-Za-z0-9_-]{43}$/);
    await client.signOut();
    await expect(client.createCode(challenge)).rejects.toThrow();
  });
});
