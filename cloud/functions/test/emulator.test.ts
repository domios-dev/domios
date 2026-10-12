import { createHash, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { initializeApp, deleteApp } from "firebase-admin/app";
import { getAuth } from "firebase-admin/auth";
import { getFirestore, Timestamp } from "firebase-admin/firestore";
import { initializeTestEnvironment, assertFails, assertSucceeds, type RulesTestEnvironment } from "@firebase/rules-unit-testing";
import { doc, getDoc, setDoc, updateDoc, deleteDoc, collection, getDocs } from "firebase/firestore";
import { beforeAll, beforeEach, afterAll, describe, expect, it } from "vitest";
import { LOGIN_ERROR, RATE_LIMIT, sha256 } from "../src/loginCodes";

const projectId = "demo-domios";
const base = `http://127.0.0.1:15001/${projectId}/us-central1`;
const authBase = `http://${process.env.FIREBASE_AUTH_EMULATOR_HOST}/identitytoolkit.googleapis.com/v1`;
const app = initializeApp({projectId}, "tests");
const db = getFirestore(app);
let rules: RulesTestEnvironment;
let idToken: string;
let uid: string;
const appCheck = [
  Buffer.from(JSON.stringify({alg:"none",typ:"JWT"})).toString("base64url"),
  Buffer.from(JSON.stringify({app_id:"emulator-test-app",exp:Math.floor(Date.now()/1000)+3600})).toString("base64url"), "",
].join(".");

async function callCreate(data: unknown, auth = true, appCheckToken: string | null = appCheck) {
  return fetch(`${base}/createDesktopLoginCode`, {
    method:"POST", headers:{"Content-Type":"application/json", ...(auth ? {Authorization:`Bearer ${idToken}`} : {}),
      ...(appCheckToken ? {"X-Firebase-AppCheck":appCheckToken} : {})}, body:JSON.stringify({data}),
  });
}
async function requestCode(redirect_uri = "http://127.0.0.1:45123/callback") {
  const code_verifier = randomBytes(32).toString("base64url");
  const state = randomBytes(24).toString("base64url");
  const code_challenge = createHash("sha256").update(code_verifier).digest("base64url");
  const response = await callCreate({code_challenge,state,redirect_uri});
  expect(response.status).toBe(200);
  const {result} = await response.json();
  return {code:result.code as string, code_verifier,state,redirect_uri};
}
async function redeem(data: unknown, headers: Record<string,string> = {}) {
  return fetch(`${base}/redeemDesktopLoginCode`, {method:"POST", headers:{"Content-Type":"application/json",...headers},body:JSON.stringify(data)});
}
async function refused(response: Response) {
  expect(response.status).toBe(400);
  expect(await response.json()).toEqual({error:LOGIN_ERROR});
}

beforeAll(async () => {
  if (!process.env.FIRESTORE_EMULATOR_HOST || !process.env.FIREBASE_AUTH_EMULATOR_HOST) throw new Error("Emulators required");
  const authResponse = await fetch(`${authBase}/accounts:signUp?key=fake`, {
    method:"POST", headers:{"Content-Type":"application/json"}, body:JSON.stringify({returnSecureToken:true}),
  });
  expect(authResponse.status).toBe(200);
  ({idToken,localId:uid} = await authResponse.json());
  rules = await initializeTestEnvironment({projectId, firestore:{
    host:"127.0.0.1",port:18080,rules:readFileSync(new URL("../../firestore.rules", import.meta.url), "utf8"),
  }});
});
beforeEach(async () => { await rules.clearFirestore(); });
afterAll(async () => { await rules.cleanup(); await deleteApp(app); });

describe("deployed desktop functions on emulators", () => {
  it.each(["http://127.0.0.1:45123/callback", "domios://auth/callback"])("redeems %s once and signs into the matching account", async (redirect) => {
    const input = await requestCode(redirect);
    expect(input.code).toMatch(/^[A-Za-z0-9_-]{43}$/);
    const ref = db.doc(`desktopLoginCodes/${sha256(input.code)}`);
    const stored = (await ref.get()).data()!;
    expect(stored.uid).toBe(uid);
    expect(stored.used).toBe(false);
    expect(stored.expires_at.toMillis() - Date.now()).toBeGreaterThan(110_000);
    expect(stored.expires_at.toMillis() - Date.now()).toBeLessThanOrEqual(120_000);
    expect(stored).not.toHaveProperty("code");
    expect(stored).not.toHaveProperty("code_verifier");
    const result = await redeem(input);
    expect(result.status).toBe(200);
    expect(result.headers.get("cache-control")).toBe("no-store");
    const {custom_token} = await result.json();
    const signedIn = await fetch(`${authBase}/accounts:signInWithCustomToken?key=fake`, {
      method:"POST", headers:{"Content-Type":"application/json"},body:JSON.stringify({token:custom_token,returnSecureToken:true}),
    });
    expect(signedIn.status).toBe(200);
    const session = await signedIn.json();
    expect((await getAuth(app).verifyIdToken(session.idToken)).uid).toBe(uid);
    expect((await getAuth(app).getUser(uid)).uid).toBe(uid);
    expect((await ref.get()).data()!.used).toBe(true);
    await refused(await redeem(input));
  });
  it("allows only one concurrent redemption", async () => {
    const input = await requestCode();
    const results = await Promise.all([redeem(input),redeem(input)]);
    expect(results.map((result) => result.status).sort()).toEqual([200,400]);
    await refused(results.find((result) => result.status === 400)!);
  });
  it("refuses expired codes without consuming them", async () => {
    const input = await requestCode();
    const ref = db.doc(`desktopLoginCodes/${sha256(input.code)}`);
    await ref.update({expires_at:Timestamp.fromMillis(Date.now()-1)});
    await refused(await redeem(input));
    expect((await ref.get()).data()!.used).toBe(false);
  });
  it.each(["code_verifier", "state", "redirect_uri"])("refuses a wrong %s without consuming the code", async (field) => {
    const input = await requestCode();
    const wrong = field === "redirect_uri" ? "domios://auth/callback" : "x".repeat(43);
    await refused(await redeem({...input,[field]:wrong}));
    expect((await db.doc(`desktopLoginCodes/${sha256(input.code)}`).get()).data()!.used).toBe(false);
    expect((await redeem(input)).status).toBe(200);
  });
  it.each([[], {}, {code:"x".repeat(43),code_verifier:"v".repeat(43),state:"s".repeat(32),redirect_uri:"domios://auth/callback"}])("gives the generic error for malformed or unknown codes", async (input) => {
    await refused(await redeem(input));
  });
  it.each(["https://evil.example", "http://localhost:1234/callback", "http://127.0.0.1:1234/callback?next=https://evil.example"])("refuses creation with %s", async (redirect_uri) => {
    const response = await callCreate({code_challenge:"c".repeat(43),state:"s".repeat(32),redirect_uri});
    expect(response.status).toBe(400);
    expect((await response.json()).error.message).toBe(LOGIN_ERROR);
    expect((await db.collection("desktopLoginCodes").get()).empty).toBe(true);
  });
  it("requires Firebase authentication", async () => {
    const response = await callCreate({code_challenge:"c".repeat(43),state:"s".repeat(32),redirect_uri:"domios://auth/callback"},false);
    expect(response.status).toBe(401);
    expect((await response.json()).error.status).toBe("UNAUTHENTICATED");
  });
  it("requires App Check even with Firebase authentication", async () => {
    const response = await callCreate({},true,null);
    expect(response.status).toBe(401);
    expect((await response.json()).error.status).toBe("UNAUTHENTICATED");
  });
  it("counts failures per IP and cannot be bypassed by spoofing forwarded headers", async () => {
    const input = await requestCode();
    for (let index = 0; index < RATE_LIMIT; index++) {
      await refused(await redeem({}, {"X-Forwarded-For":`192.0.2.${index+1}`}));
    }
    await refused(await redeem(input, {"X-Forwarded-For":"198.51.100.1"}));
    expect((await db.doc(`desktopLoginCodes/${sha256(input.code)}`).get()).data()!.used).toBe(false);
    const limits = await db.collection("desktopLoginRateLimits").get();
    expect(limits.size).toBe(1);
    expect(limits.docs[0].data().count).toBe(RATE_LIMIT);
    await limits.docs[0].ref.update({expires_at:Timestamp.fromMillis(Date.now()-1)});
    expect((await redeem(input)).status).toBe(200);
  });
  it("rejects non-POST and non-JSON requests with the generic error", async () => {
    await refused(await fetch(`${base}/redeemDesktopLoginCode`));
    await refused(await fetch(`${base}/redeemDesktopLoginCode`, {method:"POST",headers:{"Content-Type":"text/plain"},body:"hello"}));
  });
});

describe("deny-by-default Firestore boundaries", () => {
  it.each(["desktopLoginCodes", "desktopLoginRateLimits", "entitlements", "billing", "billingEvents", "admin", "adminAudit", "unknown"])("denies client reads and writes to %s including nested documents", async (name) => {
    await db.doc(`${name}/secret`).set({uid,secret:"server-only"});
    await db.doc(`${name}/secret/nested/secret`).set({secret:"server-only"});
    for (const client of [rules.authenticatedContext(uid),rules.unauthenticatedContext()]) {
      const store = client.firestore();
      await assertFails(getDoc(doc(store, name, "secret")));
      await assertFails(getDocs(collection(store, name)));
      await assertFails(setDoc(doc(store, name, "secret"), {uid}));
      await assertFails(updateDoc(doc(store, name, "secret"), {uid}));
      await assertFails(deleteDoc(doc(store, name, "secret")));
      await assertFails(getDoc(doc(store, name, "secret", "nested", "secret")));
      await assertFails(setDoc(doc(store, name, "secret", "nested", "new"), {uid}));
    }
  });
  it("allows only the owner's profile fields", async () => {
    const own = doc(rules.authenticatedContext(uid).firestore(),"users",uid);
    await assertSucceeds(setDoc(own,{displayName:"Person",photoURL:"https://example.com/photo.png"}));
    await assertSucceeds(getDoc(own));
    await assertSucceeds(updateDoc(own,{displayName:"Updated"}));
    await assertFails(updateDoc(own,{admin:true}));
    await assertFails(setDoc(own,{displayName:123}));
    await assertFails(setDoc(own,{displayName:"x".repeat(101)}));
    await assertFails(setDoc(own,{photoURL:"x".repeat(2049)}));
    await assertFails(getDoc(doc(rules.authenticatedContext("foreign").firestore(),"users",uid)));
    await assertFails(setDoc(doc(rules.authenticatedContext("foreign").firestore(),"users",uid),{displayName:"Intruder"}));
    await assertFails(getDoc(doc(rules.unauthenticatedContext().firestore(),"users",uid)));
    await assertFails(setDoc(doc(rules.unauthenticatedContext().firestore(),"users",uid),{displayName:"Intruder"}));
    await assertFails(setDoc(doc(rules.authenticatedContext(uid).firestore(),"users",uid,"private","secret"),{admin:true}));
    await assertSucceeds(deleteDoc(own));
  });
});
