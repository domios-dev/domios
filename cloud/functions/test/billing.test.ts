import { readFileSync } from "node:fs";
import Stripe from "stripe";
import { initializeApp, deleteApp } from "firebase-admin/app";
import { getFirestore } from "firebase-admin/firestore";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { StripeGateway } from "../src/billing/adapters/stripe";
import { FirestoreBillingStore } from "../src/billing/adapters/firestore";
import { BillingService } from "../src/billing/service";
import { FakeGateway } from "../src/billing/adapters/fake";
import { MemoryBillingStore } from "../src/billing/adapters/memory";
import { reduceEntitlement, type Entitlement } from "../src/billing/model";

const secret = "whsec_fixture_only";
const app = initializeApp({projectId:"demo-domios"},"billing-tests");
const db = getFirestore(app);
const config = {mode:"test" as const,plans:{pro:"price_pro"},hostedOrigin:"https://demo-domios.web.app"};
const fixture = (type: string) => JSON.parse(readFileSync(new URL(`./fixtures/stripe/${type}.json`,import.meta.url),"utf8"));
const sdk = new Stripe("sk_test_fixture");
const fakeClient = {
  webhooks:sdk.webhooks,
  subscriptions:{retrieve:async () => fixture("customer.subscription.created").data.object},
  charges:{retrieve:async () => ({customer:"cus_fixture"})},
} as unknown as Stripe;
const gateway = new StripeGateway(fakeClient,secret,"test",config.plans);
const store = new FirestoreBillingStore(db,"test");
const service = new BillingService(gateway,store,config);
function signed(event: unknown) {
  const payload = JSON.stringify(event);
  return {body:Buffer.from(payload),signature:sdk.webhooks.generateTestHeaderString({payload,secret})};
}
async function deliver(type: string) {
  const input = signed(fixture(type));
  return service.webhook(input.body,input.signature);
}
const baseEntitlement: Entitlement = {plan:"pro",status:"active",current_period_end:2000000000,cancel_at_period_end:false,source_event_id:"seed",updated_at:1800000000};
beforeAll(() => { if (!process.env.FIRESTORE_EMULATOR_HOST) throw new Error("Firestore emulator required"); });
beforeEach(async () => {
  for (const name of ["test_billingCustomers","test_entitlements","test_stripeEvents","billingCustomers","entitlements","stripeEvents"]) {
    await db.recursiveDelete(db.collection(name));
  }
  await db.doc("test_billing/config").set({plans:config.plans,hostedOrigin:config.hostedOrigin});
  await db.doc("test_billingCustomers/owner").set({customer:"cus_fixture"});
  await db.doc("test_entitlements/owner").set(baseEntitlement);
});
afterAll(() => deleteApp(app));
describe("signed billing lifecycle persisted in the emulator", () => {
  it.each([
    ["customer.subscription.created","active",2000000000,false],
    ["customer.subscription.updated","active",2000000000,true],
    ["checkout.session.completed","active",2000000000,false],
    ["invoice.paid","active",2100000000,false],
    ["invoice.payment_failed","past_due",2000000000,false],
    ["customer.subscription.deleted","canceled",2000000000,false],
    ["charge.refunded","revoked",2000000000,false],
    ["charge.dispute.created","revoked",2000000000,false],
  ])("applies %s once with isolated test collections", async (type,status,end,cancel) => {
    expect(await deliver(String(type))).toBe(true);
    const ref = db.doc("test_entitlements/owner");
    const before = await ref.get();
    expect(before.data()).toEqual({plan:"pro",status,current_period_end:end,cancel_at_period_end:cancel,
      source_event_id:fixture(String(type)).id,updated_at:fixture(String(type)).created});
    expect(await deliver(String(type))).toBe(false);
    const after = await ref.get();
    expect(after.data()).toEqual(before.data());
    expect(after.updateTime?.isEqual(before.updateTime!)).toBe(true);
    expect((await db.collection("test_stripeEvents").get()).size).toBe(1);
    for (const name of ["billingCustomers","entitlements","stripeEvents"]) expect((await db.collection(name).get()).empty).toBe(true);
    expect((await db.doc("test_entitlements/victim").get()).exists).toBe(false);
  });
  it("refuses test events at a live store boundary", async () => {
    const input = signed(fixture("charge.refunded"));
    const event = await gateway.verifyAndParseWebhook(input.body,input.signature);
    await expect(new FirestoreBillingStore(db,"live").apply(event)).rejects.toThrow("mode mismatch");
    expect((await db.collection("stripeEvents").get()).empty).toBe(true);
    expect((await db.collection("entitlements").get()).empty).toBe(true);
  });
  it("serializes concurrent duplicate deliveries", async () => {
    expect((await Promise.all([deliver("invoice.paid"),deliver("invoice.paid")])).sort()).toEqual([false,true]);
  });
  it("rejects tampered signatures and live events without persisting anything", async () => {
    const input = signed(fixture("invoice.paid"));
    await expect(service.webhook(Buffer.from(input.body.toString()+" "),input.signature)).rejects.toThrow();
    const live = signed({...fixture("invoice.paid"),livemode:true});
    await expect(service.webhook(live.body,live.signature)).rejects.toThrow("mode mismatch");
    expect((await db.collection("test_stripeEvents").get()).empty).toBe(true);
    expect((await db.doc("test_entitlements/owner").get()).data()).toEqual(baseEntitlement);
  });
  it("does not grant an unpaid inactive checkout", async () => {
    const client = {...fakeClient,subscriptions:{retrieve:async () => ({...fixture("customer.subscription.created").data.object,status:"incomplete"})}} as unknown as Stripe;
    const inactive = new BillingService(new StripeGateway(client,secret,"test",config.plans),store,config);
    const input = signed({...fixture("checkout.session.completed"),data:{object:{customer:"cus_fixture",subscription:"sub_fixture",payment_status:"unpaid"}}});
    await inactive.webhook(input.body,input.signature);
    expect((await db.doc("test_entitlements/owner").get()).data()).toEqual(baseEntitlement);
  });
  it("retries unknown customer events without recording them", async () => {
    await db.doc("test_billingCustomers/owner").delete();
    await expect(deliver("invoice.paid")).rejects.toThrow("Unknown billing customer");
    expect((await db.collection("test_stripeEvents").get()).empty).toBe(true);
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
    expect(fake.checkouts[0]).toEqual({uid:"new-owner",customer:"local_bmV3LW93bmVy",price:"price_pro",successUrl:"https://demo-domios.web.app/billing/success",cancelUrl:"https://demo-domios.web.app/billing/cancel"});
    await expect(billing.portal(undefined)).rejects.toMatchObject({code:"unauthenticated"});
    await expect(billing.portal("unknown")).rejects.toMatchObject({code:"failed-precondition"});
    expect((await billing.portal("new-owner")).url).toContain("local_bmV3LW93bmVy");
  });
  it("does not let older events or subscription updates undo revocation", () => {
    expect(reduceEntitlement(undefined,{id:"refund",created:1900000000,mode:"test",customer:"cus_fixture",change:{status:"revoked"}})?.status).toBe("revoked");
    expect(reduceEntitlement(baseEntitlement,{id:"old",created:1,mode:"test",customer:"cus_fixture",change:{status:"canceled"}})).toBe(baseEntitlement);
    expect(reduceEntitlement({...baseEntitlement,status:"revoked"},{id:"new",created:1900000000,mode:"test",customer:"cus_fixture",change:{status:"active"}})?.status).toBe("revoked");
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
    const input = signed(fixture("charge.refunded"));
    const request = (body: Buffer) => fetch(`${base}/stripeWebhook`,{method:"POST",headers:{"Content-Type":"application/json","Stripe-Signature":input.signature},body:body.toString()});
    expect((await request(Buffer.from(input.body.toString()+" "))).status).toBe(400);
    expect((await db.collection("test_stripeEvents").get()).empty).toBe(true);
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

describe("gateway transport contract", () => {
  it("passes server identity and fixed pricing to the provider with customer idempotency", async () => {
    const createCustomer = vi.fn().mockResolvedValue({id:"cus_owned"});
    const checkout = vi.fn().mockResolvedValue({url:"https://checkout.example/session"});
    const portal = vi.fn().mockResolvedValue({url:"https://portal.example/session"});
    const client = {customers:{create:createCustomer},checkout:{sessions:{create:checkout}},billingPortal:{sessions:{create:portal}}} as unknown as Stripe;
    const gateway = new StripeGateway(client,secret,"test",config.plans);
    expect(await gateway.createCustomer("owner","billing:test:owner")).toBe("cus_owned");
    expect(createCustomer).toHaveBeenCalledWith({metadata:{uid:"owner"}},{idempotencyKey:"billing:test:owner"});
    await gateway.createCheckout({uid:"owner",customer:"cus_owned",price:"price_pro",successUrl:"https://demo-domios.web.app/billing/success",cancelUrl:"https://demo-domios.web.app/billing/cancel"});
    expect(checkout).toHaveBeenCalledWith({mode:"subscription",customer:"cus_owned",line_items:[{price:"price_pro",quantity:1}],success_url:"https://demo-domios.web.app/billing/success",cancel_url:"https://demo-domios.web.app/billing/cancel",client_reference_id:"owner",metadata:{uid:"owner"},subscription_data:{metadata:{uid:"owner"}}});
    await gateway.createPortal("cus_owned","https://demo-domios.web.app/billing");
    expect(portal).toHaveBeenCalledWith({customer:"cus_owned",return_url:"https://demo-domios.web.app/billing"});
  });
});
