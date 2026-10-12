import { initializeApp } from "firebase-admin/app";
import { onCall, onRequest, HttpsError } from "firebase-functions/v2/https";
import { clientIp } from "./clientIp";
import { CodeExchange, LOGIN_ERROR } from "../../loginCodes";
import { accountsAdapters } from "..";

initializeApp();
const {store,tokens} = accountsAdapters("firebase");
const exchange = new CodeExchange(store,tokens);
const options = { region: "us-central1", maxInstances: 10, timeoutSeconds: 15 };

export const createDesktopLoginCode = onCall({ ...options, enforceAppCheck: true }, async (request) => {
  if (!request.auth) throw new HttpsError("unauthenticated", LOGIN_ERROR);
  try {
    return { code: await exchange.create(request.auth.uid, request.data) };
  } catch {
    throw new HttpsError("invalid-argument", LOGIN_ERROR);
  }
});

export const redeemDesktopLoginCode = onRequest({ ...options, cors: false, invoker: "public" }, async (request, response) => {
  response.set("Cache-Control", "no-store");
  response.set("Pragma", "no-cache");
  try {
    const ip = clientIp(request.get("x-forwarded-for"), request.socket.remoteAddress, process.env.FUNCTIONS_EMULATOR === "true");
    if (!await exchange.allowAttempt(ip) || request.method !== "POST" || !request.is("application/json")
      || Buffer.byteLength(JSON.stringify(request.body ?? null)) > 4096) throw new Error(LOGIN_ERROR);
    const custom_token = await exchange.redeem(request.body);
    response.status(200).json({ custom_token });
  } catch {
    response.status(400).json({ error: LOGIN_ERROR });
  }
});

export { createCheckoutSession, createPortalSession, stripeWebhook } from "../../billing";
