import { getAuth } from "firebase-admin/auth";
import { getFirestore } from "firebase-admin/firestore";
import { defineSecret, defineString } from "firebase-functions/params";
import { HttpsError, onCall, onRequest } from "firebase-functions/v2/https";
import { paymentGateway } from "./adapters";
import { FirestoreBillingStore } from "./adapters/firestore";
import { BillingError, BillingService } from "./service";
import type { BillingMode } from "./model";

const providerParam = defineString("BILLING_PROVIDER",{default:"stripe"});
const modeParam = defineString("BILLING_MODE",{default:"test"});
const testKey = defineSecret("BILLING_TEST_API_KEY");
const liveKey = defineSecret("BILLING_LIVE_API_KEY");
const testSecret = defineSecret("BILLING_TEST_ENDPOINT_SECRET");
const liveSecret = defineSecret("BILLING_LIVE_ENDPOINT_SECRET");
const options = {region:"us-central1",maxInstances:10,secrets:[testKey,liveKey,testSecret,liveSecret]};
async function billing(): Promise<BillingService> {
  const mode = modeParam.value();
  if (mode !== "test" && mode !== "live") throw new Error("Invalid billing mode");
  const config = (await getFirestore().doc(`${mode === "test" ? "test_" : ""}billing/config`).get()).data();
  if (!config || typeof config.hostedOrigin !== "string" || !config.plans || typeof config.plans !== "object"
    || !Object.values(config.plans).every(price => typeof price === "string" && price.startsWith("price_"))) throw new Error("Billing config missing");
  const key = mode === "test" ? testKey.value() : liveKey.value();
  const provider = providerParam.value();
  if (provider !== "stripe" && process.env.FUNCTIONS_EMULATOR !== "true") throw new Error("Dev billing requires emulator");
  const gateway = paymentGateway(provider,{key,secret:mode === "test" ? testSecret.value() : liveSecret.value(),mode,plans:config.plans});
  return new BillingService(gateway,new FirestoreBillingStore(getFirestore(),mode),{mode:mode as BillingMode,plans:config.plans,hostedOrigin:config.hostedOrigin});
}
async function callable(action: (service: BillingService) => Promise<{url:string}>) {
  try { return await action(await billing()); }
  catch (error) {
    if (error instanceof BillingError) throw new HttpsError(error.code,error.message);
    throw new HttpsError("internal","Billing unavailable");
  }
}
async function verifiedUid(authorization: string | undefined): Promise<string> {
  const match = authorization?.match(/^Bearer (\S+)$/);
  if (!match) throw new HttpsError("unauthenticated","Authentication required");
  try { return (await getAuth().verifyIdToken(match[1])).uid; }
  catch { throw new HttpsError("unauthenticated","Authentication required"); }
}
export const createCheckoutSession = onCall(options, async request => {
  const uid = await verifiedUid(request.rawRequest.get("authorization"));
  return callable(service => service.checkout(uid,request.data));
});
export const createPortalSession = onCall(options, async request => {
  const uid = await verifiedUid(request.rawRequest.get("authorization"));
  return callable(service => service.portal(uid));
});
export const stripeWebhook = onRequest({...options,cors:false},async (request,response) => {
  if (request.method !== "POST") { response.status(405).end(); return; }
  try {
    const service = await billing();
    let event;
    try { event = await service.gateway.verifyAndParseWebhook(request.rawBody,request.get("stripe-signature") ?? ""); }
    catch { response.status(400).json({error:"Invalid webhook"}); return; }
    await service.store.apply(event);
    response.status(200).json({received:true});
  } catch { response.status(500).json({error:"Billing unavailable"}); }
});
