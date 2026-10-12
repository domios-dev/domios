import { initializeApp, deleteApp } from "firebase-admin/app";
import { getFirestore } from "firebase-admin/firestore";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
const app = initializeApp({projectId:"demo-domios"},"entitlement-tests");
const db = getFirestore(app);
const endpoint = "http://127.0.0.1:15001/demo-domios/us-central1/getEntitlements";
let idToken: string;
let uid: string;
const paid = {plan:"pro",status:"active",current_period_end:2000000000,cancel_at_period_end:true};
async function call(data: unknown, token = idToken) {
  return fetch(endpoint,{method:"POST",headers:{"Content-Type":"application/json",...(token ? {Authorization:`Bearer ${token}`} : {})},body:JSON.stringify({data})});
}
beforeAll(async () => {
  if (!process.env.FIRESTORE_EMULATOR_HOST || !process.env.FIREBASE_AUTH_EMULATOR_HOST) throw new Error("Emulators required");
  const response = await fetch(`http://${process.env.FIREBASE_AUTH_EMULATOR_HOST}/identitytoolkit.googleapis.com/v1/accounts:signUp?key=fake`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({returnSecureToken:true})});
  expect(response.status).toBe(200);
  const auth = await response.json(); idToken = auth.idToken; uid = auth.localId;
  await db.doc(`test_entitlements/${uid}`).set({...paid,source_event_id:"private",updated_at:1});
  await db.doc("test_entitlements/foreign").set({...paid,plan:"foreign"});
});
afterAll(async () => { await db.doc(`test_entitlements/${uid}`).delete(); await db.doc("test_entitlements/foreign").delete(); await deleteApp(app); });
describe("getEntitlements callable", () => {
  it("returns only the authenticated owner's server record", async () => {
    for (const data of [{},{uid}]) {
      const response = await call(data);
      expect(response.status).toBe(200);
      expect((await response.json()).result).toEqual(paid);
    }
  });
  it("refuses foreign uid and leaves its persisted record intact", async () => {
    const response = await call({uid:"foreign"});
    expect(response.status).toBe(400);
    expect((await response.json()).error.status).toBe("INVALID_ARGUMENT");
    expect((await db.doc("test_entitlements/foreign").get()).data()?.plan).toBe("foreign");
  });
  it("requires a verified token", async () => {
    for (const token of ["","forged-token"]) expect((await call({},token)).status).toBe(401);
  });
  it("returns free when no entitlement exists", async () => {
    await db.doc(`test_entitlements/${uid}`).delete();
    const response = await call({});
    expect(response.status).toBe(200);
    expect((await response.json()).result).toEqual({plan:"free",status:"active",current_period_end:0,cancel_at_period_end:false});
  });
});
