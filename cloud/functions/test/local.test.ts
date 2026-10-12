import { createHash } from "node:crypto";
import { resolve } from "node:path";
import { request as httpRequest } from "node:http";
import { once } from "node:events";
import { afterAll, beforeAll, expect, it } from "vitest";
import { localServer } from "../src/adapters/local/server";
import { LOGIN_ERROR } from "../src/loginCodes";
const server = localServer(resolve("../hosting/signin"));
let base: string;
beforeAll(async () => {
  server.listen(0,"127.0.0.1");
  await once(server,"listening");
  const address = server.address();
  if (typeof address !== "object" || address === null) throw new Error("Missing local listener");
  base = `http://127.0.0.1:${address.port}`;
});
afterAll(async () => { await new Promise<void>((resolve,reject) => { server.close((error) => error ? reject(error) : resolve()); }); });
async function post(path: string, body: unknown, session?: string, extra: Record<string,string> = {}) {
  return fetch(`${base}/api/${path}`,{method:"POST",headers:{"Content-Type":"application/json",...(session ? {Authorization:`Bearer ${session}`} : {}),...extra},body:JSON.stringify(body)});
}
it("runs the local sign-in to authenticated account journey without a vendor account", async () => {
  const page = await fetch(`${base}/signin/`);
  expect(page.status).toBe(200);
  expect(await page.text()).toContain('id="local"');
  expect((await fetch(`${base}/signin/app.js`)).status).toBe(200);
  const config = await fetch(`${base}/signin/config.json`);
  expect(await config.json()).toEqual({provider:"local"});
  const signedIn = await post("signin",{});
  const {session} = await signedIn.json();
  const verifier = "v".repeat(43);
  const challenge = {code_challenge:createHash("sha256").update(verifier).digest("base64url"),state:"s".repeat(32),redirect_uri:"domios://auth/callback"};
  expect((await post("createDesktopLoginCode",challenge)).status).toBe(400);
  const codeResponse = await post("createDesktopLoginCode",challenge,session);
  expect(codeResponse.status).toBe(200);
  const {code} = await codeResponse.json();
  await post("signout",{},session);
  const redeemed = await post("redeemDesktopLoginCode",{...challenge,code,code_verifier:verifier});
  expect(redeemed.status).toBe(200);
  const {custom_token} = await redeemed.json();
  const account = await post("account",{custom_token});
  expect(await account.json()).toEqual({uid:"local-user"});
  expect(await (await post("redeemDesktopLoginCode",{...challenge,code,code_verifier:verifier})).json()).toEqual({error:LOGIN_ERROR});
  expect((await post("account",{custom_token:custom_token+"x"})).status).toBe(400);
});
it("refuses cross-origin requests and DNS rebinding hostnames", async () => {
  expect((await post("signin",{},undefined,{Origin:"https://evil.example"})).status).toBe(400);
  const status = await new Promise<number | undefined>((resolve,reject) => {
    const request = httpRequest(`${base}/api/signin`, {method:"POST",headers:{Host:"evil.example","Content-Type":"application/json"}},(response) => {
      response.resume();
      response.on("end",() => resolve(response.statusCode));
    });
    request.on("error",reject);
    request.end("{}");
  });
  expect(status).toBe(400);
});
