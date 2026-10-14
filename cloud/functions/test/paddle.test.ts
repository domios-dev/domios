import { createHmac } from "node:crypto";
import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import { PaddleGateway } from "../src/billing/adapters/paddle";
import { BillingService } from "../src/billing/service";
import { MemoryBillingStore } from "../src/billing/adapters/memory";
import { openPaddleCheckout, checkoutTransactionId, type PaddleClient, type PaddleCheckoutEvent } from "../../checkout/paddle";
const secret = "fixture_endpoint_secret";
const price = `pri_${"p".repeat(26)}`;
const txn = `txn_${"t".repeat(26)}`;
const customer = `ctm_${"c".repeat(26)}`;
const config = {mode:"test" as const,plans:{pro:price},hostedOrigin:"https://domios.dev"};
const fixture = (name: string) => JSON.parse(readFileSync(new URL(`./fixtures/paddle/${name}.json`,import.meta.url),"utf8"));
function signed(event: unknown, ts = Math.floor(Date.now()/1000), key = secret) {
  const body = Buffer.from(JSON.stringify(event));
  return {body,signature:`ts=${ts};h1=${createHmac("sha256",key).update(`${ts}:`).update(body).digest("hex")}`};
}
describe("Paddle API boundary", () => {
  it("creates a server-priced transaction and portal session with only server credentials", async () => {
    const request = vi.fn<typeof fetch>().mockResolvedValueOnce(new Response(JSON.stringify({data:{id:txn}})))
      .mockResolvedValueOnce(new Response(JSON.stringify({data:{urls:{general:{overview:"https://customer-portal.paddle.com/session"}}}})));
    const gateway = new PaddleGateway("pdl_sdbx_apikey_fixture",secret,"test",config.plans,"https://domios.dev/checkout",request);
    const billing = new BillingService(gateway,new MemoryBillingStore(),config);
    await expect(billing.checkout(undefined,{plan:"pro"})).rejects.toMatchObject({code:"unauthenticated"});
    await expect(billing.checkout("owner",{plan:"foreign"})).rejects.toMatchObject({code:"invalid-argument"});
    expect(request).not.toHaveBeenCalled();
    expect(await billing.checkout("owner",{plan:"pro"})).toEqual({url:`https://domios.dev/checkout?_ptxn=${txn}`});
    const [url,options] = request.mock.calls[0];
    expect(url).toBe("https://sandbox-api.paddle.com/transactions");
    expect(options?.headers).toMatchObject({Authorization:"Bearer pdl_sdbx_apikey_fixture"});
    expect(JSON.parse(String(options?.body))).toEqual({items:[{price_id:price,quantity:1}],collection_mode:"automatic",
      custom_data:{uid:"owner",billing_binding:createHmac("sha256",secret).update("checkout:test:owner").digest("hex")},checkout:{url:"https://domios.dev/checkout"}});
    expect(await billing.store.customer("owner")).toBeUndefined();
    expect(await gateway.createPortal(customer,"https://domios.dev/billing")).toBe("https://customer-portal.paddle.com/session");
    expect(request.mock.calls[1][0]).toBe(`https://sandbox-api.paddle.com/customers/${customer}/portal-sessions`);
    expect(request.mock.calls[1][1]?.body).toBe("{}");
  });
  it("refuses mode-mismatched API keys, unknown prices and failed API responses", async () => {
    const request = vi.fn<typeof fetch>().mockResolvedValue(new Response("failure",{status:401}));
    expect(() => new PaddleGateway("pdl_live_apikey_fixture",secret,"test",config.plans,"https://domios.dev/checkout",request)).toThrow("mode mismatch");
    const gateway = new PaddleGateway("pdl_sdbx_apikey_fixture",secret,"test",config.plans,"https://domios.dev/checkout",request);
    await expect(gateway.createCheckout({uid:"owner",customer:"",price:"foreign",successUrl:"",cancelUrl:""})).rejects.toThrow("price");
    expect(request).not.toHaveBeenCalled();
    await expect(gateway.createPortal(customer,"")).rejects.toThrow("request failed");
  });
});
describe("Paddle signature and normalization", () => {
  const request = vi.fn<typeof fetch>(async () => new Response(JSON.stringify({data:fixture("subscription.created").data})));
  const gateway = new PaddleGateway("pdl_sdbx_apikey_fixture",secret,"test",config.plans,"https://domios.dev/checkout",request);
  it("accepts rotated h1 signatures and refuses malformed, stale and tampered input", async () => {
    const input = signed(fixture("subscription.created"));
    expect((await gateway.verifyAndParseWebhook(input.body,`${input.signature};h1=${"0".repeat(64)}`)).change?.status).toBe("active");
    for (const signature of ["", "ts=bad;h1=00", input.signature+";ts=1", signed(fixture("subscription.created"),1).signature]) {
      await expect(gateway.verifyAndParseWebhook(input.body,signature)).rejects.toThrow();
    }
    await expect(gateway.verifyAndParseWebhook(Buffer.concat([input.body,Buffer.from(" ")]),input.signature)).rejects.toThrow("signature");
  });
  it("refuses foreign subscription prices and keeps pending refunds inert", async () => {
    const event = fixture("subscription.created");
    const input = signed({...event,data:{...event.data,items:[{price:{id:"foreign"}}]}});
    await expect(gateway.verifyAndParseWebhook(input.body,input.signature)).rejects.toThrow("Unsupported subscription");
    const refund = fixture("adjustment.refund");
    for (const status of ["pending_approval","rejected","reversed"]) {
      const pending = signed({...refund,data:{...refund.data,status}});
      expect((await gateway.verifyAndParseWebhook(pending.body,pending.signature)).change).toBeNull();
    }
  });
  it("handles canceled subscriptions with no current billing period", async () => {
    const event = fixture("subscription.canceled");
    const input = signed({...event,data:{...event.data,current_billing_period:null,canceled_at:"2033-05-18T03:33:20Z"}});
    expect((await gateway.verifyAndParseWebhook(input.body,input.signature)).change).toMatchObject({status:"canceled",current_period_end:2000000000});
  });
});
describe("framework-free website checkout contract", () => {
  it.each(["sandbox","production"] as const)("initializes %s and opens the URL transaction", async environment => {
    const paddle: PaddleClient = {Environment:{set:vi.fn()},Initialize:vi.fn(),Checkout:{open:vi.fn()}};
    await openPaddleCheckout({paddleEnvironment:environment,paddleClientToken:environment === "sandbox" ? "test_public" : "live_public"},undefined,{},`https://domios.dev/checkout?_ptxn=${txn}`,async () => paddle);
    expect(paddle.Environment.set).toHaveBeenCalledWith(environment);
    expect(paddle.Initialize).toHaveBeenCalledWith({token:environment === "sandbox" ? "test_public" : "live_public",eventCallback:expect.any(Function)});
    expect(paddle.Checkout.open).toHaveBeenCalledWith({transactionId:txn});
  });
  it("accepts an explicit server transaction and refuses invalid ids and API keys", async () => {
    expect(checkoutTransactionId("https://domios.dev/checkout",txn)).toBe(txn);
    expect(() => checkoutTransactionId("https://domios.dev/checkout?_ptxn=foreign")).toThrow();
    const loader = vi.fn();
    await expect(openPaddleCheckout({paddleEnvironment:"sandbox",paddleClientToken:"pdl_sdbx_apikey_fixture"},txn,{},"https://domios.dev/checkout",loader)).rejects.toThrow("public checkout");
    expect(loader).not.toHaveBeenCalled();
  });
});

it("maps Paddle events to completion, cancellation and error signals", async () => {
  let emit: ((event: PaddleCheckoutEvent) => void) | undefined;
  const completed = vi.fn();
  const closed = vi.fn();
  const error = vi.fn();
  const paddle: PaddleClient = {Environment:{set:vi.fn()},Initialize:options => {emit = options.eventCallback;},Checkout:{open:vi.fn()}};
  const callbacks = {onCompleted:completed,onClosed:closed,onError:error};
  await openPaddleCheckout({paddleEnvironment:"sandbox",paddleClientToken:"test_public"},txn,callbacks,"https://domios.dev/checkout",async () => paddle);
  expect(completed).not.toHaveBeenCalled();
  emit!({name:"checkout.completed",data:{transaction_id:"foreign"}});
  expect(completed).not.toHaveBeenCalled();
  emit!({name:"checkout.error",data:{error:{detail:"Fixture error"}}});
  expect(error).toHaveBeenCalledWith(expect.objectContaining({message:"Fixture error"}));
  emit!({name:"checkout.completed",data:{transaction_id:txn}});
  emit!({name:"checkout.completed",data:{transaction_id:txn}});
  emit!({name:"checkout.closed"});
  expect(completed).toHaveBeenCalledExactlyOnceWith(txn);
  expect(closed).not.toHaveBeenCalled();
  await openPaddleCheckout({paddleEnvironment:"sandbox",paddleClientToken:"test_public"},txn,callbacks,"https://domios.dev/checkout",async () => paddle);
  emit!({name:"checkout.closed"});
  emit!({name:"checkout.closed"});
  expect(closed).toHaveBeenCalledTimes(1);
});
