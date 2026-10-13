import { readFileSync } from "node:fs";
import { createHmac } from "node:crypto";
import { initializeApp, deleteApp } from "firebase-admin/app";
import { getFirestore } from "firebase-admin/firestore";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { PaddleGateway } from "../src/billing/adapters/paddle";
import { FirestoreBillingStore } from "../src/billing/adapters/firestore";
import { BillingService } from "../src/billing/service";
import { FakeGateway } from "../src/billing/adapters/fake";
import { MemoryBillingStore } from "../src/billing/adapters/memory";
import { reduceEntitlement, type Entitlement } from "../src/billing/model";

const secret = "fixture_endpoint_secret";
const app = initializeApp({projectId:"demo-domios"},"billing-tests");
const db = getFirestore(app);
const config = {mode:"test" as const,plans:{pro:"pri_pppppppppppppppppppppppppp"},hostedOrigin:"https://demo-domios.web.app"};
const fixture = (type: string) => JSON.parse(readFileSync(new URL(`./fixtures/paddle/${type}.json`,import.meta.url),"utf8"));
const fakeRequest = vi.fn<typeof fetch>(async () => new Response(JSON.stringify({data:fixture("subscription.created").data}),{status:200}));
const gateway = new PaddleGateway("pdl_sdbx_apikey_fixture",secret,"test",config.plans,"https://domios.dev/checkout",fakeRequest);
const store = new FirestoreBillingStore(db,"test");
const service = new BillingService(gateway,store,config);
function signed(event: unknown) {
  const payload = JSON.stringify(event);
  const ts = Math.floor(Date.now()/1000);
  const h1 = createHmac("sha256",secret).update(`${ts}:${payload}`).digest("hex");
  return {body:Buffer.from(payload),signature:`ts=${ts};h1=${h1}`};
}
async function deliver(type: string) {
  const input = signed(fixture(type));
  return service.webhook(input.body,input.signature);
}
const baseEntitlement: Entitlement = {plan:"pro",status:"active",current_period_end:2000000000,cancel_at_period_end:false,source_event_id:"seed",updated_at:1800000000};
beforeAll(() => { if (!process.env.FIRESTORE_EMULATOR_HOST) throw new Error("Firestore emulator required"); });
beforeEach(async () => {
  for (const name of ["test_billingCustomers","test_entitlements","test_billingEvents","billingCustomers","entitlements","billingEvents"]) {
    await db.recursiveDelete(db.collection(name));
  }
  await db.doc("test_billing/config").set({plans:config.plans,hostedOrigin:config.hostedOrigin,checkoutUrl:"https://domios.dev/checkout",paddleClientToken:"test_public_fixture"});
  await db.doc("test_billingCustomers/owner").set({customer:"ctm_cccccccccccccccccccccccccc"});
  await db.doc("test_entitlements/owner").set(baseEntitlement);
});
afterAll(() => deleteApp(app));
describe("signed billing lifecycle persisted in the emulator", () => {
  it.each([
    ["subscription.created","active",2000000000,false],
    ["subscription.activated","active",2000000000,false],
    ["subscription.updated","active",2000000000,true],
    ["subscription.past_due","past_due",2000000000,false],
    ["transaction.completed","active",2100000000,false],
    ["transaction.payment_failed","past_due",2000000000,false],
    ["subscription.canceled","canceled",2000000000,false],
    ["adjustment.refund","revoked",2000000000,false],
    ["adjustment.chargeback","revoked",2000000000,false],
  ])("applies %s once with isolated test collections", async (type,status,end,cancel) => {
    expect(await deliver(String(type))).toBe(true);
    const ref = db.doc("test_entitlements/owner");
    const before = await ref.get();
    expect(before.data()).toEqual({plan:"pro",status,current_period_end:end,cancel_at_period_end:cancel,
      source_event_id:fixture(String(type)).event_id,updated_at:Math.floor(Date.parse(fixture(String(type)).occurred_at)/1000)});
    expect(await deliver(String(type))).toBe(false);
    const after = await ref.get();
    expect(after.data()).toEqual(before.data());
    expect(after.updateTime?.isEqual(before.updateTime!)).toBe(true);
    expect((await db.collection("test_billingEvents").get()).size).toBe(1);
    for (const name of ["billingCustomers","entitlements","billingEvents"]) expect((await db.collection(name).get()).empty).toBe(true);
    expect((await db.doc("test_entitlements/victim").get()).exists).toBe(false);
  });
  it("refuses test events at a live store boundary", async () => {
    const input = signed(fixture("adjustment.refund"));
    const event = await gateway.verifyAndParseWebhook(input.body,input.signature);
    await expect(new FirestoreBillingStore(db,"live").apply(event)).rejects.toThrow("mode mismatch");
    expect((await db.collection("billingEvents").get()).empty).toBe(true);
    expect((await db.collection("entitlements").get()).empty).toBe(true);
  });
  it("serializes concurrent duplicate deliveries", async () => {
    expect((await Promise.all([deliver("transaction.completed"),deliver("transaction.completed")])).sort()).toEqual([false,true]);
  });
  it("rejects tampered signatures without persisting anything", async () => {
    const input = signed(fixture("transaction.completed"));
    await expect(service.webhook(Buffer.from(input.body.toString()+" "),input.signature)).rejects.toThrow();
    expect((await db.collection("test_billingEvents").get()).empty).toBe(true);
    expect((await db.doc("test_entitlements/owner").get()).data()).toEqual(baseEntitlement);
  });
  it("ignores pending and rejected refunds", async () => {
    for (const status of ["pending_approval","rejected"]) {
      const event = fixture("adjustment.refund");
      const input = signed({...event,event_id:`evt_${status.padEnd(26,"0").replace("_", "0")}`,data:{...event.data,status}});
      await service.webhook(input.body,input.signature);
    }
    expect((await db.doc("test_entitlements/owner").get()).data()).toEqual(baseEntitlement);
  });
  it("retries unknown customer events without recording them", async () => {
    await db.doc("test_billingCustomers/owner").delete();
    await expect(deliver("transaction.completed")).rejects.toThrow("Unknown billing customer");
    expect((await db.collection("test_billingEvents").get()).empty).toBe(true);
  });
});
describe("authenticated billing API domain", () => {
  it("refuses unauthenticated and foreign-price checkout before side effects", async () => {
    const fake = new FakeGateway(secret);
    const billing = new BillingService(fake,new MemoryBillingStore(),config);
    await expect(billing.checkout(undefined,{plan:"pro"})).rejects.toMatchObject({code:"unauthenticated"});
    for (const data of [{plan:"price_foreign"},{plan:"pro",price:"price_foreign"},{plan:"pro",uid:"foreign"}]) {
      await expect(billing.checkout("owner",data)).rejects.toMatchObject({code:"invalid-argument"});
    }
    expect(fake.customers).toEqual([]);
  });
  it("reuses one customer and fixes checkout identity, prices and return URLs", async () => {
    const fake = new FakeGateway(secret);
    const billing = new BillingService(fake,store,config);
    await Promise.all([billing.checkout("new-owner",{plan:"pro"}),billing.checkout("new-owner",{plan:"pro"})]);
    expect(fake.customers).toEqual(["new-owner"]);
    expect(await store.customer("new-owner")).toBe("local_bmV3LW93bmVy");
    expect(new Set(fake.checkouts.map(input => input.customer)).size).toBe(1);
    expect(fake.checkouts[0]).toEqual({uid:"new-owner",customer:"local_bmV3LW93bmVy",price:"pri_pppppppppppppppppppppppppp",successUrl:"https://demo-domios.web.app/billing/success",cancelUrl:"https://demo-domios.web.app/billing/cancel"});
    await expect(billing.portal(undefined)).rejects.toMatchObject({code:"unauthenticated"});
    await expect(billing.portal("unknown")).rejects.toMatchObject({code:"failed-precondition"});
    expect((await billing.portal("new-owner")).url).toContain("local_bmV3LW93bmVy");
  });
  it("does not let older events or subscription updates undo revocation", () => {
    expect(reduceEntitlement(undefined,{id:"refund",created:1900000000,mode:"test",customer:"ctm_cccccccccccccccccccccccccc",change:{status:"revoked"}})?.status).toBe("revoked");
    expect(reduceEntitlement(baseEntitlement,{id:"old",created:1,mode:"test",customer:"ctm_cccccccccccccccccccccccccc",change:{status:"canceled"}})).toBe(baseEntitlement);
    expect(reduceEntitlement({...baseEntitlement,status:"revoked"},{id:"new",created:1900000000,mode:"test",customer:"ctm_cccccccccccccccccccccccccc",change:{status:"active"}})?.status).toBe("revoked");
  });
});

describe("deployed callable authentication", () => {
  it.each([["createCheckoutSession",null],["createCheckoutSession","forged-token"],["createPortalSession",null],["createPortalSession","forged-token"]])("requires a verified token for %s (%s)", async (name,token) => {
    const response = await fetch(`http://127.0.0.1:15001/demo-domios/us-central1/${name}`,{method:"POST",headers:{"Content-Type":"application/json",...(token ? {Authorization:`Bearer ${token}`} : {})},body:JSON.stringify({data:{plan:"pro"}})});
    expect(response.status).toBe(401);
    expect((await response.json()).error.status).toBe("UNAUTHENTICATED");
  });
});

describe("deployed webhook and checkout refusals", () => {
  const base = "http://127.0.0.1:15001/demo-domios/us-central1";
  it("returns 400 for a tampered signature and persists a signed refund exactly once", async () => {
    const input = signed(fixture("adjustment.refund"));
    const request = (body: Buffer) => fetch(`${base}/paddleWebhook`,{method:"POST",headers:{"Content-Type":"application/json","Paddle-Signature":input.signature},body:body.toString()});
    expect((await request(Buffer.from(input.body.toString()+" "))).status).toBe(400);
    expect((await db.collection("test_billingEvents").get()).empty).toBe(true);
    expect((await request(input.body)).status).toBe(200);
    const before = await db.doc("test_entitlements/owner").get();
    expect(before.data()?.status).toBe("revoked");
    expect((await request(input.body)).status).toBe(200);
    expect((await db.doc("test_entitlements/owner").get()).updateTime?.isEqual(before.updateTime!)).toBe(true);
    expect((await db.collection("entitlements").get()).empty).toBe(true);
  });
  it("refuses foreign prices for a verified Firebase account", async () => {
    const auth = await fetch(`http://${process.env.FIREBASE_AUTH_EMULATOR_HOST}/identitytoolkit.googleapis.com/v1/accounts:signUp?key=fake`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({returnSecureToken:true})});
    expect(auth.status).toBe(200);
    const {idToken,localId} = await auth.json();
    const response = await fetch(`${base}/createCheckoutSession`,{method:"POST",headers:{"Content-Type":"application/json",Authorization:`Bearer ${idToken}`},body:JSON.stringify({data:{plan:"price_foreign"}})});
    expect(response.status).toBe(400);
    expect((await response.json()).error.status).toBe("INVALID_ARGUMENT");
    expect((await db.doc(`test_billingCustomers/${localId}`).get()).exists).toBe(false);
  });
});


describe("Paddle checkout identity binding", () => {
  const binding = (uid: string) => createHmac("sha256",secret).update(`checkout:test:${uid}`).digest("hex");
  it("binds a new customer and entitlement atomically from server-authenticated custom data", async () => {
    await db.doc("test_billingCustomers/owner").delete();
    await db.doc("test_entitlements/owner").delete();
    const event = fixture("subscription.created");
    const input = signed({...event,data:{...event.data,custom_data:{uid:"owner",billing_binding:binding("owner")}}});
    expect(await service.webhook(input.body,input.signature)).toBe(true);
    expect(await store.customer("owner")).toBe(event.data.customer_id);
    expect((await db.doc("test_entitlements/owner").get()).data()).toMatchObject({plan:"pro",status:"active"});
    expect(await service.webhook(input.body,input.signature)).toBe(false);
  });
  it("refuses forged identity and conflicting customer ownership", async () => {
    const event = fixture("subscription.created");
    for (const billing_binding of ["forged",binding("owner")]) {
      const input = signed({...event,data:{...event.data,custom_data:{uid:"victim",billing_binding}}});
      await expect(service.webhook(input.body,input.signature)).rejects.toThrow("identity");
    }
    const input = signed({...event,data:{...event.data,custom_data:{uid:"victim",billing_binding:binding("victim")}}});
    await expect(service.webhook(input.body,input.signature)).rejects.toThrow("owner mismatch");
    expect((await db.doc("test_entitlements/victim").get()).exists).toBe(false);
    expect((await db.collection("test_billingEvents").get()).empty).toBe(true);
  });
  it("refuses a sandbox signature at the live endpoint", async () => {
    const input = signed(fixture("subscription.created"));
    const live = new PaddleGateway("pdl_live_apikey_fixture","fixture_live_endpoint_secret","live",config.plans,"https://domios.dev/checkout",fakeRequest);
    await expect(live.verifyAndParseWebhook(input.body,input.signature)).rejects.toThrow("signature");
    expect((await db.collection("entitlements").get()).empty).toBe(true);
  });
});


describe("public checkout configuration", () => {
  it("publishes only the public client token and environment", async () => {
    const response = await fetch("http://127.0.0.1:15001/demo-domios/us-central1/billingCheckoutConfig");
    expect(response.status).toBe(200);
    expect(response.headers.get("cache-control")).toBe("no-store");
    expect(await response.json()).toEqual({paddleEnvironment:"sandbox",paddleClientToken:"test_public_fixture"});
  });
  it("refuses an API key in public token configuration", async () => {
    await db.doc("test_billing/config").update({paddleClientToken:"pdl_sdbx_apikey_fixture"});
    const response = await fetch("http://127.0.0.1:15001/demo-domios/us-central1/billingCheckoutConfig");
    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({error:"Checkout configuration unavailable"});
  });
});
